//! Apps and their webhooks (design note 0016, section 8): an external app's record, its
//! signing secret, and the deliveries of the events it subscribes to.

use axum::{Json, extract::State, http::StatusCode};
use hypha_apps::{
    App, CreateApp, HasApps, UpdateApp, WebhookInput, create_app, delete_app, get_app, get_event,
    list_apps, list_deliveries, retry_delivery, rotate_app_secret, update_app,
};
use hypha_core::{DeliveryId, DeliveryRecord, DeliveryStatus, EventId, HasOutbox, Stamped};
use hypha_extensions::{ExtensionPoint, list_extension_calls};
use serde::{Deserialize, Serialize};

use super::{
    page::{ListQuery, PageParams, PageResponse},
    rfc3339, stamp_times,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
    webhooks::EventEnvelope,
};

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookDto {
    /// Where Hypha POSTs events: `https://`, or `http://localhost` for development.
    url: String,
    /// 1000 to 30000 milliseconds; defaults to 10000.
    #[serde(default)]
    #[schema(required = false)]
    timeout_ms: Option<u32>,
}

impl From<WebhookDto> for WebhookInput {
    fn from(w: WebhookDto) -> Self {
        Self {
            url: w.url,
            timeout_ms: w.timeout_ms,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AppResponse {
    /// `app_<name>`.
    key: String,
    name: String,
    /// A disabled app gets no webhooks and quotes no rates.
    enabled: bool,
    /// Where its events go; `null` when it takes none.
    webhook: Option<WebhookDto>,
    /// Event types it gets: exact (`order.placed`), a kind (`product.*`) or `*`.
    subscriptions: Vec<String>,
    created_at: String,
    updated_at: String,
}

impl AppResponse {
    fn new(app: &App) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(app.stamps())?;
        Ok(Self {
            key: app.key().to_string(),
            name: app.name().to_string(),
            enabled: app.enabled(),
            webhook: app.webhook().map(|w| WebhookDto {
                url: w.url.as_str().to_string(),
                timeout_ms: Some(w.timeout_ms),
            }),
            subscriptions: app
                .subscriptions()
                .iter()
                .map(|p| p.as_str().to_string())
                .collect(),
            created_at,
            updated_at,
        })
    }
}

/// An app with its signing secret: returned once, when created or rotated.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AppSecretResponse {
    app: AppResponse,
    /// Verify `Hypha-Signature` on webhooks and rate requests with it. Shown only now.
    secret: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAppRequest {
    /// `app_<name>`: lowercase letters, digits and `_`, at most 64 characters.
    key: String,
    name: String,
    #[serde(default)]
    webhook: Option<WebhookDto>,
    /// Up to 50 event types, `kind.*` or `*`.
    #[serde(default)]
    subscriptions: Vec<String>,
}

/// Registers an app. Its signing secret is returned here, once.
#[utoipa::path(
    post,
    path = "/admin/apps",
    request_body = CreateAppRequest,
    responses((status = 201, description = "Created", body = AppSecretResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateAppRequest>,
) -> Result<(StatusCode, Json<AppSecretResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateApp {
        key: req.key,
        name: req.name,
        webhook: req.webhook.map(Into::into),
        subscriptions: req.subscriptions,
    };
    let (app, secret) = create_app(&ctx, ports.apps(), &state.env, &state.env, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(AppSecretResponse {
            app: AppResponse::new(&app)?,
            secret,
        }),
    ))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AppList {
    /// By key.
    items: Vec<AppResponse>,
}

/// Every app.
#[utoipa::path(
    get,
    path = "/admin/apps",
    responses((status = 200, description = "OK", body = AppList)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<AppList>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let apps = list_apps(&ctx, ports.apps()).await?;
    Ok(Json(AppList {
        items: apps
            .iter()
            .map(AppResponse::new)
            .collect::<Result<_, _>>()?,
    }))
}

/// One app.
#[utoipa::path(
    get,
    path = "/admin/apps/{key}",
    params(("key" = String, Path, pattern = "^app_[a-z][a-z0-9_]*$", example = "app_erp")),
    responses((status = 200, description = "OK", body = AppResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<Json<AppResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let app = get_app(&ctx, ports.apps(), &key).await?;
    Ok(Json(AppResponse::new(&app)?))
}

/// Name, switch, webhook and subscriptions (JSON Merge Patch: `webhook: null` removes
/// the webhook; the others take no `null`).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAppRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    enabled: Option<bool>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<WebhookDto>, required = false)]
    webhook: Option<Option<WebhookDto>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<String>, required = false)]
    subscriptions: Option<Vec<String>>,
}

/// Changes an app.
#[utoipa::path(
    patch,
    path = "/admin/apps/{key}",
    params(("key" = String, Path, pattern = "^app_[a-z][a-z0-9_]*$", example = "app_erp")),
    request_body = UpdateAppRequest,
    responses((status = 200, description = "OK", body = AppResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
    body: JsonBody<UpdateAppRequest>,
) -> Result<Json<AppResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateApp {
        name: req.name,
        enabled: req.enabled,
        webhook: req.webhook.map(|w| w.map(Into::into)),
        subscriptions: req.subscriptions,
    };
    let app = update_app(&ctx, ports.apps(), &key, input).await?;
    Ok(Json(AppResponse::new(&app)?))
}

/// Removes an app, its rate settings with it; deliveries still due to it fail.
#[utoipa::path(
    delete,
    path = "/admin/apps/{key}",
    params(("key" = String, Path, pattern = "^app_[a-z][a-z0-9_]*$", example = "app_erp")),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn delete<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_app(&ctx, ports.apps(), &key).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Replaces the signing secret and returns the new one, once. Calls signed with the old
/// one stop verifying at once.
#[utoipa::path(
    post,
    path = "/admin/apps/{key}/secret",
    params(("key" = String, Path, pattern = "^app_[a-z][a-z0-9_]*$", example = "app_erp")),
    responses((status = 200, description = "OK", body = AppSecretResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn rotate_secret<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<Json<AppSecretResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (app, secret) = rotate_app_secret(&ctx, ports.apps(), &state.env, &state.env, &key).await?;
    Ok(Json(AppSecretResponse {
        app: AppResponse::new(&app)?,
        secret,
    }))
}

// ---------- deliveries ----------

/// Where a delivery stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = DeliveryStatus)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatusDto {
    /// Waiting for its next attempt.
    Pending,
    Succeeded,
    /// Failed for good; retry it with `POST …/retry`.
    Failed,
}

impl From<DeliveryStatusDto> for DeliveryStatus {
    fn from(s: DeliveryStatusDto) -> Self {
        match s {
            DeliveryStatusDto::Pending => DeliveryStatus::Pending,
            DeliveryStatusDto::Succeeded => DeliveryStatus::Succeeded,
            DeliveryStatusDto::Failed => DeliveryStatus::Failed,
        }
    }
}

impl From<DeliveryStatus> for DeliveryStatusDto {
    fn from(s: DeliveryStatus) -> Self {
        match s {
            DeliveryStatus::Pending => DeliveryStatusDto::Pending,
            DeliveryStatus::Succeeded => DeliveryStatusDto::Succeeded,
            DeliveryStatus::Failed => DeliveryStatusDto::Failed,
        }
    }
}

/// One event on its way to an app.
#[derive(Serialize, utoipa::ToSchema)]
pub struct DeliveryResponse {
    /// Sent as `Hypha-Delivery-Id`, the same on every attempt.
    id: DeliveryId,
    /// `GET /admin/events/{id}` has the event as sent.
    event_id: EventId,
    event_type: String,
    occurred_at: String,
    status: DeliveryStatusDto,
    /// Attempts made so far.
    attempts: u32,
    /// When it's tried next, while pending.
    next_attempt_at: String,
    /// Why the last attempt failed.
    last_error: Option<String>,
}

impl DeliveryResponse {
    fn new(d: &DeliveryRecord) -> Result<Self, ApiError> {
        Ok(Self {
            id: DeliveryId::from_uuid(d.id),
            event_id: EventId::from_uuid(d.event_id),
            event_type: d.event_type.clone(),
            occurred_at: rfc3339(d.occurred_at)?,
            status: d.status.into(),
            attempts: d.attempts,
            next_attempt_at: rfc3339(d.next_attempt_at)?,
            last_error: d.last_error.clone(),
        })
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct DeliveryFilters {
    /// Only deliveries in this status.
    status: Option<DeliveryStatusDto>,
}

/// A page of deliveries (named for the documents: utoipa's `body =` takes a plain name).
type DeliveryPage = PageResponse<DeliveryResponse>;

/// An app's webhook deliveries, newest event first, optionally of one status. Kept for
/// 30 days.
#[utoipa::path(
    get,
    path = "/admin/apps/{key}/deliveries",
    params(
        ("key" = String, Path, pattern = "^app_[a-z][a-z0-9_]*$", example = "app_erp"),
        PageParams,
        DeliveryFilters,
    ),
    responses((status = 200, description = "OK", body = DeliveryPage)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn deliveries<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
    query: ListQuery<DeliveryFilters>,
) -> Result<Json<DeliveryPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse()?;
    let status = filters.status.map(Into::into);
    let page = list_deliveries(&ctx, &ports, &key, status, page).await?;
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |d| d.event_seq,
        |d| DeliveryResponse::new(&d),
    )?))
}

// ---------- extension calls (design note 0029) ----------

/// How an extension call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = ExtensionCallOutcome)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionCallOutcomeDto {
    /// A 2xx whose body was the point's response.
    Ok,
    /// No answer (refused, unreachable) or a status other than 2xx.
    Failed,
    /// No answer within the timeout.
    Timeout,
    /// A 2xx whose body wasn't the point's response, broke its rules, or was too big.
    Invalid,
}

impl From<hypha_extensions::CallOutcome> for ExtensionCallOutcomeDto {
    fn from(o: hypha_extensions::CallOutcome) -> Self {
        use hypha_extensions::CallOutcome as O;
        match o {
            O::Ok => Self::Ok,
            O::Failed => Self::Failed,
            O::Timeout => Self::Timeout,
            O::Invalid => Self::Invalid,
        }
    }
}

/// One call Hypha made to the app's extension point.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ExtensionCallResponse {
    id: hypha_core::ExtensionCallId,
    /// The point (`shipping.rates`, `payments.start`, …); `openapi/extensions.json`
    /// documents each.
    point: String,
    version: u16,
    started_at: String,
    duration_ms: u32,
    outcome: ExtensionCallOutcomeDto,
    /// The answer's HTTP status, when one came.
    status: Option<u16>,
    /// Why it wasn't `ok`: the app's own error `code` when it sent one.
    error: Option<String>,
    /// What the call was for: the cart, order or payment ID.
    correlation: Option<String>,
}

impl ExtensionCallResponse {
    fn new(c: &hypha_extensions::ExtensionCall) -> Result<Self, ApiError> {
        Ok(Self {
            id: c.id,
            point: c.point.clone(),
            version: c.version,
            started_at: rfc3339(c.started_at)?,
            duration_ms: c.duration_ms,
            outcome: c.outcome.into(),
            status: c.status,
            error: c.error.clone(),
            correlation: c.correlation.clone(),
        })
    }
}

/// A page of extension calls.
type ExtensionCallPage = PageResponse<ExtensionCallResponse>;

/// The calls Hypha made to the app's extension points, newest first: what each was for,
/// how long it took and how it ended. Kept for 30 days unless configured
/// (`jobs.prune.extension_calls_after`).
#[utoipa::path(
    get,
    path = "/admin/apps/{key}/extension-calls",
    params(
        ("key" = String, Path, pattern = "^app_[a-z][a-z0-9_]*$", example = "app_erp"),
        PageParams,
    ),
    responses((status = 200, description = "OK", body = ExtensionCallPage)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn extension_calls<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
    query: ListQuery,
) -> Result<Json<ExtensionCallPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, _, scope) = query.parse()?;
    // The app first: an unknown one is `404 app_not_found`, not an empty list.
    let app = get_app(&ctx, ports.apps(), &key).await?;
    let page = list_extension_calls(&ctx, &ports, app.key(), page).await?;
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |c| c.id,
        |c| ExtensionCallResponse::new(&c),
    )?))
}

/// Sends a delivery again when the dispatcher next runs (within seconds): a failed one
/// gets one more attempt, a pending one stops waiting. One being sent right now is
/// returned as it is.
#[utoipa::path(
    post,
    path = "/admin/apps/{key}/deliveries/{id}/retry",
    params(
        ("key" = String, Path, pattern = "^app_[a-z][a-z0-9_]*$", example = "app_erp"),
        ("id" = DeliveryId, Path),
    ),
    responses((status = 200, description = "OK", body = DeliveryResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn retry<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((key, id)): PathId<(String, DeliveryId)>,
) -> Result<Json<DeliveryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let delivery = retry_delivery(&ctx, &ports, &state.clock, &key, id).await?;
    Ok(Json(DeliveryResponse::new(&delivery)?))
}

/// One event as apps receive it, for debugging a delivery. Kept for 30 days.
#[utoipa::path(
    get,
    path = "/admin/events/{id}",
    params(("id" = EventId, Path)),
    responses((status = 200, description = "OK", body = EventEnvelope)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn event<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<EventId>,
) -> Result<Json<EventEnvelope>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let event = get_event(&ctx, ports.outbox(), id).await?;
    Ok(Json(EventEnvelope::new(&event)?))
}

// ---------- extension endpoints (design note 0029, §3) ----------

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtensionEndpointDto {
    /// A point this Hypha calls apps through: `payments.start`, `payments.capture`,
    /// `payments.void`, `payments.refund` or `search.query` (`unknown_extension_point`
    /// otherwise). Shipping rates are registered as a shipping provider's
    /// `callback_url`.
    point: String,
    /// The version the app answers (1).
    version: u16,
    /// Where Hypha POSTs the point's request, signed: `https://`, or `http://localhost`
    /// for development.
    url: String,
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtensionEndpointsDto {
    /// By point name.
    extension_points: Vec<ExtensionEndpointDto>,
}

impl ExtensionEndpointsDto {
    fn of(endpoints: Vec<hypha_apps::ExtensionEndpoint>) -> Self {
        Self {
            extension_points: endpoints
                .into_iter()
                .map(|e| ExtensionEndpointDto {
                    point: e.point,
                    version: e.version,
                    url: e.url.as_str().to_string(),
                })
                .collect(),
        }
    }
}

/// The points an app may register here: every point this build knows but
/// `shipping.rates`, whose URL is the shipping provider's `callback_url`.
fn registrable() -> Vec<hypha_apps::KnownPoint> {
    hypha_extensions::KNOWN
        .iter()
        .filter(|(name, _)| *name != hypha_extensions::ShippingRates::NAME)
        .map(|&(name, version)| hypha_apps::KnownPoint { name, version })
        .collect()
}

/// Where the app answers extension points: the URLs Hypha calls for a payment method
/// it collects (`payments.*`) or the store's searches (`search.query`).
#[utoipa::path(
    get,
    path = "/admin/apps/{key}/extension-points",
    params(("key" = String, Path, pattern = "^app_[a-z][a-z0-9_]*$", example = "app_stripe")),
    responses((status = 200, description = "OK", body = ExtensionEndpointsDto)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn extension_points<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<Json<ExtensionEndpointsDto>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let endpoints = hypha_apps::list_extension_endpoints(&ctx, &ports, &key).await?;
    Ok(Json(ExtensionEndpointsDto::of(endpoints)))
}

/// Sets where a private app (one without a manifest) answers extension points,
/// replacing what it had; an empty list stops every call. Each point once
/// (`duplicate_extension_point`), a point and version this Hypha knows
/// (`unknown_extension_point`). Calls are signed with the app's secret.
#[utoipa::path(
    put,
    path = "/admin/apps/{key}/extension-points",
    params(("key" = String, Path, pattern = "^app_[a-z][a-z0-9_]*$", example = "app_stripe")),
    request_body = ExtensionEndpointsDto,
    responses((status = 200, description = "OK", body = ExtensionEndpointsDto)),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn set_extension_points<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
    body: JsonBody<ExtensionEndpointsDto>,
) -> Result<Json<ExtensionEndpointsDto>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let inputs = req
        .extension_points
        .into_iter()
        .map(|e| hypha_apps::EndpointInput {
            point: e.point,
            version: e.version,
            url: e.url,
        })
        .collect();
    let endpoints =
        hypha_apps::set_extension_endpoints(&ctx, &ports, &key, inputs, &registrable()).await?;
    Ok(Json(ExtensionEndpointsDto::of(endpoints)))
}
