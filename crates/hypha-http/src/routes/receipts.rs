//! Receiving: goods arriving against purchase orders, and taking a receipt back.

use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_core::{
    ExchangeRate, LocationId, PurchaseOrderId, ReceiptId, Stamped, UnitCost, VariantId,
};
use hypha_purchasing::{Receipt, ReceiptState, get_receipt, list_receipts};
use hypha_receiving::{Receive, ReceiveLine, cancel_receipt, receive};
use serde::{Deserialize, Serialize};

use super::{
    purchase_orders::{PurchaseOrderResponse, respond, variant_infos},
    rfc3339, stamp_times,
    stores::store_currency,
};
use crate::{
    error::ApiError,
    extract::{Caller, KeyedJson, PathId, idempotent_response},
    state::{SharedState, Storage},
};

/// Whether a receipt still counts.
#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStateDto {
    Recorded,
    /// Taken back: its units came off stock and off the order's counts.
    Cancelled,
}

/// One variant's units in a receipt.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ReceiptLineResponse {
    variant: VariantId,
    /// Into stock.
    accepted: u32,
    /// Refused: no stock.
    rejected: u32,
    /// What each accepted unit cost, in the store's currency: its order line's cost and
    /// share of the extra costs, at the receipt's exchange rate.
    landed_unit_cost: Option<UnitCost>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ReceiptResponse {
    id: ReceiptId,
    purchase_order: PurchaseOrderId,
    /// Where the goods arrived.
    location: LocationId,
    lines: Vec<ReceiptLineResponse>,
    /// The order's rate when it was received, when it has one.
    exchange_rate: Option<ExchangeRate>,
    note: Option<String>,
    status: ReceiptStateDto,
    /// RFC 3339.
    cancelled_at: Option<String>,
    /// RFC 3339: when it was received.
    created_at: String,
    updated_at: String,
}

impl ReceiptResponse {
    fn new(r: &Receipt) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(r.stamps())?;
        let (status, cancelled_at) = match r.state() {
            ReceiptState::Recorded => (ReceiptStateDto::Recorded, None),
            ReceiptState::Cancelled { at } => (ReceiptStateDto::Cancelled, Some(rfc3339(at)?)),
        };
        Ok(Self {
            id: r.id(),
            purchase_order: r.purchase_order(),
            location: r.location(),
            lines: r
                .lines()
                .iter()
                .map(|l| ReceiptLineResponse {
                    variant: l.variant,
                    accepted: l.accepted,
                    rejected: l.rejected,
                    landed_unit_cost: l.landed_unit_cost,
                })
                .collect(),
            exchange_rate: r.exchange_rate(),
            note: r.note().map(str::to_string),
            status,
            cancelled_at,
            created_at,
            updated_at,
        })
    }
}

/// A receipt and its order after it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ReceivedResponse {
    receipt: ReceiptResponse,
    purchase_order: PurchaseOrderResponse,
    /// Variants that had no stock record and got one now, untracked, so whether they sell
    /// didn't change: turn tracking on to count them for sale. Empty on a replay.
    new_items: Vec<VariantId>,
}

/// A receipt taken back and its order after it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CancelledReceiptResponse {
    receipt: ReceiptResponse,
    purchase_order: PurchaseOrderResponse,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ReceiptList {
    /// Oldest first, those taken back too.
    items: Vec<ReceiptResponse>,
}

/// One variant's units arriving.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceiveLineRequest {
    variant: VariantId,
    /// Into stock.
    #[serde(default)]
    accepted: u32,
    /// Refused (damaged, wrong): recorded, no stock.
    #[serde(default)]
    rejected: u32,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceiveRequest {
    /// Where the goods arrived; the order's destination when absent.
    #[serde(default)]
    location: Option<LocationId>,
    /// One per variant, each with at least one unit accepted or rejected.
    lines: Vec<ReceiveLineRequest>,
    /// Take more than a line has outstanding (`409 over_received` otherwise).
    #[serde(default)]
    allow_over: bool,
    #[serde(default)]
    note: Option<String>,
}

/// Records goods arriving against an ordered purchase order: accepted units go on hand at
/// the location (the order's destination unless the body names another) at their landed
/// cost, which joins each item's moving average; rejected ones are only counted. A
/// variant with no stock record gets one, untracked (listed in `new_items`); a variant
/// removed from the catalogue since is counted but goes into no stock. Units over what a
/// line ordered come in at their unit cost alone, without a share of the extra costs. The
/// order closes once every line is accounted for. More than a line has outstanding is
/// `409 over_received` (with each line's outstanding in `details.lines`) unless
/// `allow_over`; the store needs its base currency (`409 store_currency_required`).
/// Takes an `Idempotency-Key` header: a retry returns the receipt the first request
/// recorded.
#[utoipa::path(
    post,
    path = "/admin/purchase-orders/{id}/receipts",
    params(("id" = PurchaseOrderId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Makes the request safe to retry: a retry with the same key and body replays the first answer (marked `Idempotent-Replayed: true`) instead of repeating the effect. Same key with another body: `422 idempotency_key_reused`.")),
    request_body = ReceiveRequest,
    responses((status = 201, description = "Created", body = ReceivedResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`.")))),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PurchaseOrderId>,
    req: KeyedJson<ReceiveRequest>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = req.parse()?;
    let body = req.body;
    let store = store_currency(&state, &ports).await?;
    // A line's variant may have been removed since it was ordered: its units are counted
    // but go into no stock.
    let asked: Vec<VariantId> = body.lines.iter().map(|l| l.variant).collect();
    let still: Vec<VariantId> = variant_infos(&ports, &asked)
        .await?
        .into_iter()
        .map(|info| info.variant)
        .collect();
    let removed_variants = asked.into_iter().filter(|v| !still.contains(v)).collect();
    let input = Receive {
        purchase_order: id,
        location: body.location,
        lines: body
            .lines
            .iter()
            .map(|l| ReceiveLine {
                variant: l.variant,
                accepted: l.accepted,
                rejected: l.rejected,
            })
            .collect(),
        allow_over: body.allow_over,
        removed_variants,
        note: body.note,
        idempotency: req.idempotency,
    };
    let done = receive(&ctx, &ports, &state.ids, &state.clock, store, input).await?;
    let received = done.value;
    let response = ReceivedResponse {
        receipt: ReceiptResponse::new(&received.receipt)?,
        purchase_order: respond(&state, &ports, &received.purchase_order).await?,
        new_items: received.new_items,
    };
    Ok(idempotent_response(
        StatusCode::CREATED,
        response,
        done.replayed,
    ))
}

/// The order's receipts, oldest first, those taken back too.
#[utoipa::path(
    get,
    path = "/admin/purchase-orders/{id}/receipts",
    params(("id" = PurchaseOrderId, Path)),
    responses((status = 200, description = "OK", body = ReceiptList)),
    extensions(("x-hypha-scopes" = json!(["purchasing:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PurchaseOrderId>,
) -> Result<Json<ReceiptList>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let receipts = list_receipts(&ctx, &ports, id).await?;
    Ok(Json(ReceiptList {
        items: receipts
            .iter()
            .map(ReceiptResponse::new)
            .collect::<Result<_, _>>()?,
    }))
}

#[utoipa::path(
    get,
    path = "/admin/receipts/{id}",
    params(("id" = ReceiptId, Path)),
    responses((status = 200, description = "OK", body = ReceiptResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ReceiptId>,
) -> Result<Json<ReceiptResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let receipt = get_receipt(&ctx, &ports, id).await?;
    Ok(Json(ReceiptResponse::new(&receipt)?))
}

/// Takes back a receipt recorded by mistake: its counts come off the order's lines (a
/// closed order opens again) and its accepted units off stock at its location (which may
/// go below zero), their value out of each item's average cost. A receipt taken back
/// already is `409 receipt_cancelled`.
#[utoipa::path(
    post,
    path = "/admin/receipts/{id}/cancel",
    params(("id" = ReceiptId, Path)),
    responses((status = 200, description = "OK", body = CancelledReceiptResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn cancel<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ReceiptId>,
) -> Result<Json<CancelledReceiptResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let store = store_currency(&state, &ports).await?;
    let taken = cancel_receipt(&ctx, &ports, &state.clock, store, id).await?;
    Ok(Json(CancelledReceiptResponse {
        receipt: ReceiptResponse::new(&taken.receipt)?,
        purchase_order: respond(&state, &ports, &taken.purchase_order).await?,
    }))
}
