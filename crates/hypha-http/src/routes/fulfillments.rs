//! Fulfilments: recording what left a location for an order, its tracking, and taking it
//! back.

use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_core::{FulfillmentId, LocationId, OrderId, OrderLineId};
use hypha_fulfillment::{
    Fulfillment, FulfillmentState, HasFulfillments, TrackingInput, get_fulfillment,
    list_order_fulfillments, update_tracking,
};
use hypha_order_actions::{Fulfil, FulfilLine, cancel_fulfillment, fulfill};
use hypha_orders::{HasOrders, get_order};
use serde::{Deserialize, Serialize};

use super::stamp_times;
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, KeyedJson, PathId, idempotent_response},
    patch,
    state::{SharedState, Storage},
};

/// A tracking number, with the carrier and a URL to follow it when known.
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackingDto {
    /// "DHL Express"; free text.
    #[serde(default)]
    carrier: Option<String>,
    number: String,
    /// `http://` or `https://`, at most 2048 characters.
    #[serde(default)]
    url: Option<String>,
}

impl TrackingDto {
    fn input(tracking: Vec<TrackingDto>) -> Vec<TrackingInput> {
        tracking
            .into_iter()
            .map(|t| TrackingInput {
                carrier: t.carrier,
                number: t.number,
                url: t.url,
            })
            .collect()
    }
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FulfillmentLineDto {
    /// An order line of the order in the URL.
    line: OrderLineId,
    quantity: u32,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateFulfillmentRequest {
    /// Where the units leave from. Another location than the one holding them is fine:
    /// its stock goes down, and the reservation elsewhere is released.
    location: LocationId,
    /// Each order line once; at most what it has left to ship.
    lines: Vec<FulfillmentLineDto>,
    /// At most 10.
    #[serde(default)]
    tracking: Vec<TrackingDto>,
    /// Email the buyer a shipping confirmation (`false` if absent).
    #[serde(default)]
    notify: bool,
}

/// `FulfillmentState` on the wire.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FulfillmentStateDto {
    /// It left, and counts on the order.
    Shipped,
    /// Taken back: its units are in stock again.
    Cancelled,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct FulfillmentResponse {
    id: FulfillmentId,
    order: OrderId,
    location: LocationId,
    status: FulfillmentStateDto,
    /// RFC 3339, UTC; `null` unless cancelled.
    cancelled_at: Option<String>,
    lines: Vec<FulfillmentLineDto>,
    tracking: Vec<TrackingDto>,
    /// When it was recorded, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

impl FulfillmentResponse {
    pub(super) fn new(f: &Fulfillment) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(hypha_core::Stamped::stamps(f))?;
        let (status, cancelled_at) = match f.state() {
            FulfillmentState::Shipped => (FulfillmentStateDto::Shipped, None),
            FulfillmentState::Cancelled { at } => {
                (FulfillmentStateDto::Cancelled, Some(super::rfc3339(at)?))
            }
        };
        Ok(Self {
            id: f.id(),
            order: f.order(),
            location: f.location(),
            status,
            cancelled_at,
            lines: f
                .lines()
                .iter()
                .map(|l| FulfillmentLineDto {
                    line: l.line,
                    quantity: l.quantity.value(),
                })
                .collect(),
            tracking: f
                .tracking()
                .iter()
                .map(|t| TrackingDto {
                    carrier: t.carrier().map(str::to_string),
                    number: t.number().to_string(),
                    url: t.url().map(str::to_string),
                })
                .collect(),
            created_at,
            updated_at,
        })
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct FulfillmentList {
    /// Newest first, cancelled ones too.
    items: Vec<FulfillmentResponse>,
}

/// Records that lines of the order left a location: the order counts them as
/// fulfilled, and their units come out of the location's stock. Takes an
/// `Idempotency-Key` header: a retry returns the fulfilment the first request recorded.
#[utoipa::path(
    post,
    path = "/admin/orders/{id}/fulfillments",
    params(("id" = OrderId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Makes the request safe to retry: a retry with the same key and body replays the first answer (marked `Idempotent-Replayed: true`) instead of repeating the effect. Same key with another body: `422 idempotency_key_reused`.")),
    request_body = CreateFulfillmentRequest,
    responses((status = 201, description = "Created", body = FulfillmentResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`.")))),
    extensions(("x-hypha-scopes" = json!(["fulfillments:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(order): PathId<OrderId>,
    req: KeyedJson<CreateFulfillmentRequest>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = req.parse()?;
    let body = req.body;
    let input = Fulfil {
        order,
        location: body.location,
        lines: body
            .lines
            .iter()
            .map(|l| FulfilLine {
                line: l.line,
                quantity: l.quantity,
            })
            .collect(),
        tracking: TrackingDto::input(body.tracking),
        // The event carries it; the mailer answers `fulfillment.created`. A replay records
        // no second event, so the buyer is told once.
        notify_customer: body.notify,
        idempotency: req.idempotency,
    };
    let done = fulfill(&ctx, &ports, &state.ids, &state.clock, input).await?;
    Ok(idempotent_response(
        StatusCode::CREATED,
        FulfillmentResponse::new(&done.value.fulfillment)?,
        done.replayed,
    ))
}

/// The order's fulfilments, newest first, cancelled ones too.
#[utoipa::path(
    get,
    path = "/admin/orders/{id}/fulfillments",
    params(("id" = OrderId, Path)),
    responses((status = 200, description = "OK", body = FulfillmentList)),
    extensions(("x-hypha-scopes" = json!(["fulfillments:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(order): PathId<OrderId>,
) -> Result<Json<FulfillmentList>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let fulfillments = list_order_fulfillments(&ctx, ports.fulfillments(), order).await?;
    // An empty list for an order that exists, 404 for one that doesn't; the order is
    // read only when there's nothing else to tell them apart by, as the system, since
    // `fulfillments:read` doesn't grant `orders:read` and the order isn't shown.
    if fulfillments.is_empty() {
        let system = hypha_core::Ctx::system();
        get_order(&system, ports.orders(), order).await?;
    }
    Ok(Json(FulfillmentList {
        items: fulfillments
            .iter()
            .map(FulfillmentResponse::new)
            .collect::<Result<_, _>>()?,
    }))
}

/// A fulfilment.
#[utoipa::path(
    get,
    path = "/admin/fulfillments/{id}",
    params(("id" = FulfillmentId, Path)),
    responses((status = 200, description = "OK", body = FulfillmentResponse)),
    extensions(("x-hypha-scopes" = json!(["fulfillments:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<FulfillmentId>,
) -> Result<Json<FulfillmentResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let fulfillment = get_fulfillment(&ctx, ports.fulfillments(), id).await?;
    Ok(Json(FulfillmentResponse::new(&fulfillment)?))
}

/// A JSON Merge Patch of a fulfilment: absent fields stay as they are.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateFulfillmentRequest {
    /// Replaces every tracking number (at most 10; `[]` removes them).
    #[serde(default, deserialize_with = "patch::non_null")]
    // Optional, but `null` is refused (`patch::non_null`): not nullable in the spec.
    #[schema(value_type = Vec<TrackingDto>, required = false)]
    tracking: Option<Vec<TrackingDto>>,
    /// Email the buyer the new tracking, if it changed (`false` if absent).
    #[serde(default)]
    notify: bool,
}

/// Changes a standing fulfilment's tracking. A cancelled one is refused
/// (`fulfillment_cancelled`, 409).
#[utoipa::path(
    patch,
    path = "/admin/fulfillments/{id}",
    params(("id" = FulfillmentId, Path)),
    request_body = UpdateFulfillmentRequest,
    responses((status = 200, description = "OK", body = FulfillmentResponse)),
    extensions(("x-hypha-scopes" = json!(["fulfillments:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<FulfillmentId>,
    body: JsonBody<UpdateFulfillmentRequest>,
) -> Result<Json<FulfillmentResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let fulfillment = match req.tracking {
        Some(tracking) => {
            let tracking = TrackingDto::input(tracking);
            // Unchanged tracking records no event, so the buyer isn't told.
            let (after, _changed) =
                update_tracking(&ctx, ports.fulfillments(), id, &tracking, req.notify).await?;
            after
        }
        // Nothing to change: still the caller's to read.
        None => get_fulfillment(&ctx, ports.fulfillments(), id).await?,
    };
    Ok(Json(FulfillmentResponse::new(&fulfillment)?))
}

/// Takes back a fulfilment recorded by mistake: its lines count as unfulfilled again,
/// and its units are back in stock at its location, reserved for their lines.
/// Cancelling it again changes nothing.
#[utoipa::path(
    post,
    path = "/admin/fulfillments/{id}/cancel",
    params(("id" = FulfillmentId, Path)),
    responses((status = 200, description = "OK", body = FulfillmentResponse)),
    extensions(("x-hypha-scopes" = json!(["fulfillments:write"]))),
)]
pub async fn cancel<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<FulfillmentId>,
) -> Result<Json<FulfillmentResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let done = cancel_fulfillment(&ctx, &ports, &state.clock, id).await?;
    Ok(Json(FulfillmentResponse::new(&done.fulfillment)?))
}
