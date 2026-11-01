//! Transfers: stock moved between the store's locations, shipped whole and received in
//! parts.

use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_core::{Ctx, LocationId, Scope, Stamped, Timestamp, TransferId, VariantId};
use hypha_inventory::{
    CreateTransfer, Transfer, TransferArrival, TransferFilter, TransferLineDraft, TransferStatus,
    TransferSummary, UpdateTransfer, cancel_transfer, create_transfer, delete_transfer,
    get_transfer, list_transfers, receive_transfer, ship_transfer, update_transfer,
};
use hypha_products::HasProducts;
use hypha_purchasing::VariantInfo;
use serde::{Deserialize, Serialize};

use super::{
    markets::Instant,
    page::{ListQuery, PageParams, PageResponse},
    purchase_orders::variant_infos,
    rfc3339, stamp_times,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, KeyedJson, PathId, idempotent_response},
    patch,
    state::{SharedState, Storage},
};

/// Where a transfer is in its life.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransferStatusDto {
    /// Being written: anything may change, and it may be deleted.
    Draft,
    /// Left the origin, not all arrived yet.
    InTransit,
    /// Every unit shipped arrived or was given up as lost.
    Received,
    /// Called off before it shipped.
    Cancelled,
}

impl From<TransferStatus> for TransferStatusDto {
    fn from(s: TransferStatus) -> Self {
        match s {
            TransferStatus::Draft => TransferStatusDto::Draft,
            TransferStatus::InTransit => TransferStatusDto::InTransit,
            TransferStatus::Received => TransferStatusDto::Received,
            TransferStatus::Cancelled => TransferStatusDto::Cancelled,
        }
    }
}

impl From<TransferStatusDto> for TransferStatus {
    fn from(s: TransferStatusDto) -> Self {
        match s {
            TransferStatusDto::Draft => TransferStatus::Draft,
            TransferStatusDto::InTransit => TransferStatus::InTransit,
            TransferStatusDto::Received => TransferStatus::Received,
            TransferStatusDto::Cancelled => TransferStatus::Cancelled,
        }
    }
}

/// One variant on a transfer, and how much of it left and arrived.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TransferLineResponse {
    variant: VariantId,
    /// The variant's product and title ("Mug — Blue") and SKU: only with
    /// `products:read`, and `null` once the variant is gone.
    product_title: Option<String>,
    variant_title: Option<String>,
    sku: Option<String>,
    quantity: u32,
    /// Units that left the origin: the whole quantity once shipped.
    shipped: u32,
    /// Units put into stock at the destination.
    received: u32,
    /// Units lost or broken on the way: in stock nowhere.
    rejected: u32,
    /// Shipped, not yet received or rejected.
    in_transit: u32,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TransferResponse {
    id: TransferId,
    /// Shown as "TR-1001".
    number: i64,
    origin: LocationId,
    destination: LocationId,
    status: TransferStatusDto,
    /// RFC 3339: when it's due at the destination.
    expected_at: Option<String>,
    /// A carrier or tracking note.
    reference: Option<String>,
    note: Option<String>,
    lines: Vec<TransferLineResponse>,
    /// RFC 3339.
    shipped_at: Option<String>,
    received_at: Option<String>,
    cancelled_at: Option<String>,
    created_at: String,
    updated_at: String,
}

fn times(t: Option<Timestamp>) -> Result<Option<String>, ApiError> {
    t.map(rfc3339).transpose()
}

impl TransferResponse {
    fn new(t: &Transfer, variants: &[VariantInfo]) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(t.stamps())?;
        Ok(Self {
            id: t.id(),
            number: t.number().value(),
            origin: t.origin(),
            destination: t.destination(),
            status: t.status().into(),
            expected_at: times(t.expected_at())?,
            reference: t.reference().map(str::to_string),
            note: t.note().map(str::to_string),
            lines: t
                .lines()
                .iter()
                .map(|l| {
                    let info = variants.iter().find(|v| v.variant == l.variant());
                    TransferLineResponse {
                        variant: l.variant(),
                        product_title: info.map(|i| i.product_title.clone()),
                        variant_title: info.and_then(|i| i.variant_title.clone()),
                        sku: info.and_then(|i| i.sku.clone()),
                        quantity: l.quantity(),
                        shipped: l.shipped(),
                        received: l.received(),
                        rejected: l.rejected(),
                        in_transit: l.in_transit(),
                    }
                })
                .collect(),
            shipped_at: times(t.shipped_at())?,
            received_at: times(t.received_at())?,
            cancelled_at: times(t.cancelled_at())?,
            created_at,
            updated_at,
        })
    }
}

/// A transfer as its response shows it: its lines named when `ctx` reads products.
async fn respond<P: HasProducts>(
    ctx: &Ctx,
    ports: &P,
    t: &Transfer,
) -> Result<TransferResponse, ApiError> {
    let variants = if ctx.has_scope(Scope::ProductsRead) {
        let ids: Vec<VariantId> = t.lines().iter().map(|l| l.variant()).collect();
        variant_infos(ports, &ids).await?
    } else {
        Vec::new()
    };
    TransferResponse::new(t, &variants)
}

/// A transfer as a list shows it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TransferSummaryResponse {
    id: TransferId,
    /// Shown as "TR-1001".
    number: i64,
    origin: LocationId,
    destination: LocationId,
    status: TransferStatusDto,
    line_count: usize,
    /// Units over every line.
    units: u64,
    /// RFC 3339.
    expected_at: Option<String>,
    shipped_at: Option<String>,
    received_at: Option<String>,
    created_at: Option<String>,
}

impl TransferSummaryResponse {
    fn new(s: &TransferSummary) -> Result<Self, ApiError> {
        Ok(Self {
            id: s.id,
            number: s.number.value(),
            origin: s.origin,
            destination: s.destination,
            status: s.status.into(),
            line_count: s.line_count,
            units: s.units,
            expected_at: times(s.expected_at)?,
            shipped_at: times(s.shipped_at)?,
            received_at: times(s.received_at)?,
            created_at: times(s.created_at)?,
        })
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct TransferFilters {
    #[param(inline)]
    status: Option<TransferStatusDto>,
    /// Transfers leaving this location.
    origin: Option<LocationId>,
    /// Transfers going to this location.
    destination: Option<LocationId>,
    /// Transfers with a line for this variant.
    variant: Option<VariantId>,
}

type TransferPage = PageResponse<TransferSummaryResponse>;

/// Transfers, newest first, narrowed by the filters.
#[utoipa::path(
    get,
    path = "/admin/transfers",
    params(PageParams, TransferFilters),
    responses((status = 200, description = "OK", body = TransferPage)),
    extensions(("x-hypha-scopes" = json!(["inventory:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<TransferFilters>,
) -> Result<Json<TransferPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse()?;
    let filter = TransferFilter {
        status: filters.status.map(Into::into),
        origin: filters.origin,
        destination: filters.destination,
        variant: filters.variant,
    };
    let page = list_transfers(&ctx, &ports, &filter, page).await?;
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |s: &TransferSummary| s.id,
        |s| TransferSummaryResponse::new(&s),
    )?))
}

/// One variant and how many to move.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TransferLineRequest {
    variant: VariantId,
    /// At least 1.
    quantity: u32,
}

impl From<TransferLineRequest> for TransferLineDraft {
    fn from(l: TransferLineRequest) -> Self {
        TransferLineDraft {
            variant: l.variant,
            quantity: l.quantity,
        }
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTransferRequest {
    /// Where the stock leaves from: one of the store's locations.
    origin: LocationId,
    /// Where it goes: another of them.
    destination: LocationId,
    /// RFC 3339: when it's due at the destination.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = DateTime)]
    expected_at: Option<Instant>,
    /// A carrier or tracking note.
    #[serde(default)]
    reference: Option<String>,
    /// At most 5,000 characters.
    #[serde(default)]
    note: Option<String>,
    /// One per variant with a stock record, at most 1,000. A draft may have none yet;
    /// shipping needs one.
    #[serde(default)]
    lines: Vec<TransferLineRequest>,
}

/// Drafts a transfer between two of the store's locations. Each line's variant needs a
/// stock record (`422 variant_not_stocked`): there's nothing at the origin to move
/// otherwise.
#[utoipa::path(
    post,
    path = "/admin/transfers",
    request_body = CreateTransferRequest,
    responses((status = 201, description = "Created", body = TransferResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateTransferRequest>,
) -> Result<(StatusCode, Json<TransferResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateTransfer {
        origin: req.origin,
        destination: req.destination,
        expected_at: req.expected_at.map(|i| i.0),
        reference: req.reference,
        note: req.note,
        lines: req.lines.into_iter().map(Into::into).collect(),
    };
    let transfer = create_transfer(&ctx, &ports, &state.ids, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(respond(&ctx, &ports, &transfer).await?),
    ))
}

#[utoipa::path(
    get,
    path = "/admin/transfers/{id}",
    params(("id" = TransferId, Path)),
    responses((status = 200, description = "OK", body = TransferResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<TransferId>,
) -> Result<Json<TransferResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let transfer = get_transfer(&ctx, &ports, id).await?;
    Ok(Json(respond(&ctx, &ports, &transfer).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateTransferRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = LocationId, required = false)]
    origin: Option<LocationId>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = LocationId, required = false)]
    destination: Option<LocationId>,
    /// RFC 3339; `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, format = DateTime, required = false)]
    expected_at: Option<Option<Instant>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    reference: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    note: Option<Option<String>>,
    /// Replaces them all.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<TransferLineRequest>, required = false)]
    lines: Option<Vec<TransferLineRequest>>,
}

/// Changes a draft (JSON Merge Patch): a shipped transfer is on its way
/// (`409 transfer_not_draft`).
#[utoipa::path(
    patch,
    path = "/admin/transfers/{id}",
    params(("id" = TransferId, Path)),
    request_body = UpdateTransferRequest,
    responses((status = 200, description = "OK", body = TransferResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<TransferId>,
    body: JsonBody<UpdateTransferRequest>,
) -> Result<Json<TransferResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateTransfer {
        origin: req.origin,
        destination: req.destination,
        expected_at: req.expected_at.map(|e| e.map(|i| i.0)),
        reference: req.reference,
        note: req.note,
        lines: req.lines.map(|l| l.into_iter().map(Into::into).collect()),
    };
    let transfer = update_transfer(&ctx, &ports, id, input).await?;
    Ok(Json(respond(&ctx, &ports, &transfer).await?))
}

/// Deletes a draft (`409 transfer_not_draft` otherwise).
#[utoipa::path(
    delete,
    path = "/admin/transfers/{id}",
    params(("id" = TransferId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub async fn remove<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<TransferId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_transfer(&ctx, &ports, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Nothing to say yet: shipping sends every line whole.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipTransferRequest {}

/// Ships a draft whole: every line's units leave the origin now, each a `transfer_out`
/// row in its item's history. The origin's count may go below zero, as fulfilling can. A
/// draft with no line is `409 transfer_empty`. The body is optional. Takes an
/// `Idempotency-Key` header: a retry returns the transfer without shipping it again.
#[utoipa::path(
    post,
    path = "/admin/transfers/{id}/ship",
    params(("id" = TransferId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Makes the request safe to retry: a retry with the same key and body replays the first answer (marked `Idempotent-Replayed: true`) instead of repeating the effect. Same key with another body: `422 idempotency_key_reused`.")),
    request_body(content = Option<ShipTransferRequest>, description = "Optional"),
    responses((status = 200, description = "OK", body = TransferResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`.")))),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub async fn ship<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<TransferId>,
    req: KeyedJson<ShipTransferRequest>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = req.parse_optional()?;
    let done = ship_transfer(&ctx, &ports, &state.clock, id, req.idempotency).await?;
    let response = respond(&ctx, &ports, &done.value).await?;
    Ok(idempotent_response(StatusCode::OK, response, done.replayed))
}

/// One variant's units arriving.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TransferArrivalRequest {
    variant: VariantId,
    /// Into stock at the destination.
    #[serde(default)]
    accepted: u32,
    /// Lost or broken on the way: recorded, in stock nowhere.
    #[serde(default)]
    rejected: u32,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceiveTransferRequest {
    /// One per variant, each with at least one unit accepted or rejected.
    lines: Vec<TransferArrivalRequest>,
}

/// Records units arriving at the destination: accepted ones go on hand there (a
/// `transfer_in` row), rejected ones were lost on the way (a `shrinkage` row with no
/// delta). No more than a line has in transit (`409 over_received`, with each such
/// line's `in_transit` in `details.lines`). Once every line is accounted for, the
/// transfer is `received`. Takes an `Idempotency-Key` header: a retry returns the
/// transfer without receiving again.
#[utoipa::path(
    post,
    path = "/admin/transfers/{id}/receive",
    params(("id" = TransferId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Makes the request safe to retry: a retry with the same key and body replays the first answer (marked `Idempotent-Replayed: true`) instead of repeating the effect. Same key with another body: `422 idempotency_key_reused`.")),
    request_body = ReceiveTransferRequest,
    responses((status = 200, description = "OK", body = TransferResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`.")))),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub async fn receive<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<TransferId>,
    req: KeyedJson<ReceiveTransferRequest>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = req.parse()?;
    let arrivals: Vec<TransferArrival> = req
        .body
        .lines
        .iter()
        .map(|l| TransferArrival {
            variant: l.variant,
            accepted: l.accepted,
            rejected: l.rejected,
        })
        .collect();
    let done = receive_transfer(&ctx, &ports, &state.clock, id, &arrivals, req.idempotency).await?;
    let response = respond(&ctx, &ports, &done.value).await?;
    Ok(idempotent_response(StatusCode::OK, response, done.replayed))
}

/// Calls off a draft (`409 transfer_not_draft` once shipped: bringing stock back is
/// another transfer).
#[utoipa::path(
    post,
    path = "/admin/transfers/{id}/cancel",
    params(("id" = TransferId, Path)),
    responses((status = 200, description = "OK", body = TransferResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub async fn cancel<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<TransferId>,
) -> Result<Json<TransferResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let transfer = cancel_transfer(&ctx, &ports, &state.clock, id).await?;
    Ok(Json(respond(&ctx, &ports, &transfer).await?))
}
