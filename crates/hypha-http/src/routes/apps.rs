//! Apps and their webhooks (design note 0016, section 8): an external app's record, its
//! signing secret, and the deliveries of the events it subscribes to.

use axum::{Json, extract::State, http::StatusCode};
use hypha_apps::{
    App, CreateApp, HasApps, UpdateApp, WebhookInput, create_app, delete_app, get_app, get_event,
    list_apps, list_deliveries, retry_delivery, rotate_app_secret, update_app,
};
use hypha_core::{DeliveryId, DeliveryRecord, DeliveryStatus, EventId, HasOutbox, Stamped};
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
