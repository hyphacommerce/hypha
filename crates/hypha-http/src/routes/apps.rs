//! Apps and their webhooks (design note 0016, section 8): an external app's record, its
//! signing secret, and the deliveries of the events it subscribes to.

use axum::{Json, extract::State, http::StatusCode};
use hypha_apps::{
    App, AppKind, CreateApp, Grant, HasApps, UpdateApp, WebhookInput, create_app, get_app,
    get_event, list_apps, list_deliveries, retry_delivery, rotate_app_secret, update_app,
};
use hypha_core::{
    AppId, DeliveryId, DeliveryRecord, DeliveryStatus, EventId, HasOutbox, OAuthRequestId, Scope,
    Stamped,
};
use hypha_extensions::{ExtensionPoint, list_extension_calls};
use hypha_oauth::{narrow_grant, request_history, uninstall};
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
    webhooks::{EventActor, EventEnvelope, actor_of},
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

/// The scopes staff approved for an app installed through OAuth (design note 0029).
#[derive(Serialize, utoipa::ToSchema)]
pub struct GrantResponse {
    /// What the app's tokens may do, at most.
    scopes: Vec<Scope>,
    /// Who approved (or last narrowed) it.
    approved_by: EventActor,
    approved_at: String,
}

impl GrantResponse {
    fn new(grant: &Grant) -> Result<Self, ApiError> {
        Ok(Self {
            scopes: grant.scopes().to_vec(),
            approved_by: actor_of(grant.approved_by()),
            approved_at: rfc3339(grant.approved_at())?,
        })
    }
}

/// Which kind of app: an external service, or a spore built into the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = AppKind)]
#[serde(rename_all = "snake_case")]
pub enum AppKindDto {
    App,
    Spore,
}

impl From<AppKind> for AppKindDto {
    fn from(kind: AppKind) -> Self {
        match kind {
            AppKind::App => AppKindDto::App,
            AppKind::Spore => AppKindDto::Spore,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AppResponse {
    /// `app_<name>`, or `spore_<name>` for a spore.
    key: String,
    /// An external `app`, or a `spore` built into the API (design note 0030): staff
    /// only enable or disable a spore.
    kind: AppKindDto,
    name: String,
    /// A disabled app gets no webhooks and quotes no rates.
    enabled: bool,
    /// What it acts as (`appid_…`): events it causes name it as their actor.
    id: AppId,
    /// Installed through OAuth, with a grant in force.
    installed: bool,
    /// The approved scopes; `null` for an app made by hand, and after an uninstall.
    grant: Option<GrantResponse>,
    /// When it was uninstalled (it then only gets `app.uninstalled`); `DELETE` again
    /// removes it.
    uninstalled_at: Option<String>,
    /// Where its events go; `null` when it takes none.
    webhook: Option<WebhookDto>,
    /// Event types it gets: exact (`order.placed`), a kind (`product.*`) or `*`.
    subscriptions: Vec<String>,
    created_at: String,
    updated_at: String,
}

impl AppResponse {
    pub(crate) fn new(app: &App) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(app.stamps())?;
        Ok(Self {
            key: app.key().to_string(),
            kind: app.kind().into(),
            name: app.name().to_string(),
            enabled: app.enabled(),
            id: app.id(),
            installed: app.is_installed(),
            grant: app.grant().map(GrantResponse::new).transpose()?,
            uninstalled_at: app.uninstalled_at().map(rfc3339).transpose()?,
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

/// The manifest an app was installed from.
#[derive(Serialize, utoipa::ToSchema)]
pub struct InstalledManifestResponse {
    /// Its URL: the app's OAuth client ID.
    url: String,
    fetched_at: String,
    /// The manifest as checked, every URL absolute (`docs/apps/manifest.schema.json`).
    #[schema(value_type = Object)]
    document: serde_json::Value,
}

/// What an installed app registers, from its manifest, in force with its grant.
#[derive(Serialize, utoipa::ToSchema)]
pub struct RegistrationsResponse {
    /// The webhook URL and events (`webhook` and `subscriptions` above).
    #[schema(value_type = Option<Object>)]
    webhooks: Option<serde_json::Value>,
    /// `[{point, version, url}]`.
    #[schema(value_type = Vec<Object>)]
    extension_points: Vec<serde_json::Value>,
    /// `{pages, blocks, actions}`.
    #[schema(value_type = Option<Object>)]
    admin: Option<serde_json::Value>,
    /// `{elements, proxy}`.
    #[schema(value_type = Option<Object>)]
    storefront: Option<serde_json::Value>,
}

/// One authorization request in an app's history.
#[derive(Serialize, utoipa::ToSchema)]
pub struct GrantHistoryEntry {
    id: OAuthRequestId,
    /// `pending`, `approved` or `denied`.
    status: &'static str,
    /// What it asked for.
    scopes: Vec<Scope>,
    /// The grant when it asked; `null` for a first install.
    previous_scopes: Option<Vec<Scope>>,
    /// Who answered (an app answering for itself: no new scope, approved at once).
    decided_by: Option<EventActor>,
    decided_at: Option<String>,
    created_at: String,
}

/// An app with its grant, manifest, registrations and authorization history.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AppDetailResponse {
    #[serde(flatten)]
    app: AppResponse,
    /// `null` for an app made by hand.
    manifest: Option<InstalledManifestResponse>,
    /// `null` unless installed.
    registrations: Option<RegistrationsResponse>,
    /// Its authorization requests, newest first (at most 50).
    history: Vec<GrantHistoryEntry>,
}

impl AppDetailResponse {
    fn new(app: &App, history: &[hypha_oauth::AuthorizationRequest]) -> Result<Self, ApiError> {
        let manifest = app
            .manifest()
            .map(|m| {
                Ok::<_, ApiError>(InstalledManifestResponse {
                    url: m.url.clone(),
                    fetched_at: rfc3339(m.fetched_at)?,
                    document: m.manifest.to_value(),
                })
            })
            .transpose()?;
        let registrations = app.manifest().filter(|_| app.is_installed()).map(|m| {
            let doc = m.manifest.document();
            RegistrationsResponse {
                webhooks: doc.webhooks.as_ref().map(json),
                extension_points: doc.extension_points.iter().map(json).collect(),
                admin: doc.admin.as_ref().map(json),
                storefront: doc.storefront.as_ref().map(json),
            }
        });
        Ok(Self {
            app: AppResponse::new(app)?,
            manifest,
            registrations,
            history: history
                .iter()
                .map(|r| {
                    Ok::<_, ApiError>(GrantHistoryEntry {
                        id: r.id(),
                        status: r.status().as_str(),
                        scopes: r.scopes().to_vec(),
                        previous_scopes: r.previous_scopes().map(<[Scope]>::to_vec),
                        decided_by: r.decided_by().map(actor_of),
                        decided_at: r.decided_at().map(rfc3339).transpose()?,
                        created_at: rfc3339(r.created_at())?,
                    })
                })
                .collect::<Result<_, _>>()?,
        })
    }
}

/// A manifest part as JSON (plain data: it always serializes).
fn json(part: &impl Serialize) -> serde_json::Value {
    serde_json::to_value(part).unwrap_or(serde_json::Value::Null)
}

/// One app, with its grant, the manifest it was installed from, what it registers and
/// its authorization history.
#[utoipa::path(
    get,
    path = "/admin/apps/{key}",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_erp")),
    responses((status = 200, description = "OK", body = AppDetailResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<Json<AppDetailResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let app = get_app(&ctx, ports.apps(), &key).await?;
    let history = request_history(&ctx, &ports, app.key()).await?;
    Ok(Json(AppDetailResponse::new(&app, &history)?))
}

/// The scopes to keep.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NarrowGrantRequest {
    /// Some of the grant's scopes; the rest are taken away at once.
    scopes: Vec<Scope>,
}

/// Narrows an installed app's grant (design note 0029, §4.1): the scopes left out stop
/// working at once, for tokens held already too, and the app is sent
/// `app.grant_changed`. Adding a scope takes a new authorization staff approve
/// (`grant_not_narrower`, 422); an app that isn't installed is `app_not_installed`, 409.
#[utoipa::path(
    patch,
    path = "/admin/apps/{key}/grant",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_erp")),
    request_body = NarrowGrantRequest,
    responses((status = 200, description = "OK", body = AppResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn narrow<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
    body: JsonBody<NarrowGrantRequest>,
) -> Result<Json<AppResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let app = narrow_grant(&ctx, &ports, &state.clock, &key, &req.scopes).await?;
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
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_erp")),
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

/// Uninstalls an app installed through OAuth (design note 0029, §4.1): every token is
/// revoked, its registrations removed, and it's sent `app.uninstalled` as its last
/// webhook; it's listed as uninstalled until a second `DELETE`. Any other app (made by
/// hand, or uninstalled already) is removed, its rate settings with it; deliveries still
/// due to it fail.
#[utoipa::path(
    delete,
    path = "/admin/apps/{key}",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_erp")),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn delete<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    uninstall(&ctx, &ports, &state.clock, &key).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Replaces the signing secret and returns the new one, once. Calls signed with the old
/// one stop verifying at once.
#[utoipa::path(
    post,
    path = "/admin/apps/{key}/secret",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_erp")),
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
        ("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_erp"),
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
        ("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_erp"),
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
        ("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_erp"),
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
    let mut envelope = EventEnvelope::new(&event)?;
    // As an app would receive it (design note 0029, AP10).
    if !ctx.is_staff() {
        envelope.data = crate::redact::event_data(|s| ctx.has_scope(s), &event.data);
    }
    Ok(Json(envelope))
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
        .filter(|k| k.name != hypha_extensions::ShippingRates::NAME)
        .map(|k| hypha_apps::KnownPoint {
            name: k.name,
            version: k.version,
            scope: k.scope,
        })
        .collect()
}

/// Where the app answers extension points: the URLs Hypha calls for a payment method
/// it collects (`payments.*`) or the store's searches (`search.query`). For an app
/// installed from its manifest, the manifest's registrations (called only while its
/// grant has each point's scope).
#[utoipa::path(
    get,
    path = "/admin/apps/{key}/extension-points",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_stripe")),
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
/// (`unknown_extension_point`). Calls are signed with the app's secret. An app
/// installed from its manifest registers its points there instead
/// (`endpoints_from_manifest`, 409).
#[utoipa::path(
    put,
    path = "/admin/apps/{key}/extension-points",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_stripe")),
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
