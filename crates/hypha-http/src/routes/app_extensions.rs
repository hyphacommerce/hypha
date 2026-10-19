//! What installed apps contribute to admins and storefronts, for any frontend (design
//! note 0029, §5–§8):
//!
//! - discovery: `GET /admin/app-extensions?target=…` (pages, blocks, actions, settings
//!   forms) and `GET /storefront/app-extensions?target=…` (elements, proxies);
//! - session tokens for embedded pages: `POST /admin/app-extensions/{app}/session`;
//! - actions: `POST /admin/app-extensions/actions/{app}/{id}` calls the app
//!   (`admin.action` v1, in the call log);
//! - blocks: `GET /admin/app-extensions/blocks/{app}/{id}` fetches the app's UI tree,
//!   signed, and checks it against the block format v1 (in the call log as
//!   `admin.block`);
//! - the app proxy: `/admin/apps/{app}/proxy/{path}` and
//!   `/storefront/apps/{app}/proxy/{path}`, any of GET, POST, PUT, PATCH and DELETE;
//! - `POST /admin/apps/{key}/refresh`: the manifest fetched again.
//!
//! Calls Hypha forwards to an app (blocks, the proxy) carry `Hypha-Signature` over the
//! body, as every call, and `Hypha-Context-Signature` over the method and the `Hypha-*`
//! context headers (`outbound::context_signature`; `@hypha/app`'s
//! `verifyForwardedRequest`). The context is IDs only: staff IDs, customer and cart IDs
//! aren't personal data (AP10), and no name, email, phone or address is forwarded.

use std::time::Duration;

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use hypha_apps::{
    App, ExtensionContext, Surface, Target, admin_contributions, check_block, find_action,
    find_block, find_proxy, may_use_on, open_secret, scopes_for, session_app, spore_contributes,
    spore_for_ui, storefront_contributions,
};
use hypha_core::{
    Actor, AppKey, AppKind, CartId, Clock, Ctx, CustomerId, FulfillmentId, IdGenerator,
    OAuthRequestId, OrderId, PaymentId, ProductId, Scope,
};
use hypha_extensions::{
    ActionContext, AdminActionRequest, CallOutcome, Endpoint, ExtensionError, HasExtensionCalls,
    RunAdminAction, call,
};
use hypha_oauth::{Refreshed, refresh_manifest};
use serde::{Deserialize, Serialize};

use super::{apps::AppResponse, oauth::Issuer, rfc3339};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId, Query, StaffCaller, Storefront},
    outbound::{CallError, ForwardCall, Method, prepare_forward},
    session_token::{SESSION_TOKEN_SECONDS, SessionClaims, sign_session_token},
    state::{AppState, SharedState, Storage},
};

/// The most a proxied request or answer may weigh.
pub const MAX_PROXY_BYTES: usize = 1024 * 1024;
/// How long a proxied call may take.
pub const PROXY_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a block may take to arrive.
pub const BLOCK_TIMEOUT: Duration = Duration::from_secs(5);
/// The call log's name for block fetches.
pub const BLOCK_POINT: &str = "admin.block";
/// The proxy's name in `app_unavailable`'s details.
pub const PROXY_POINT: &str = "proxy";

// ---------- targets ----------

/// An admin target (design note 0029, §5): a slot any admin may implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub enum AdminTarget {
    /// A full page under the app (an embedded page).
    #[serde(rename = "apps.page")]
    AppsPage,
    /// The app's settings: an embedded page (`nav: "settings"`) or the manifest's form.
    #[serde(rename = "settings.app")]
    SettingsApp,
    /// Blocks and actions on an order (`order_id`).
    #[serde(rename = "order.details")]
    OrderDetails,
    /// On an order's payment (`order_id`, `payment_id`).
    #[serde(rename = "order.payment")]
    OrderPayment,
    /// On an order's fulfilment (`order_id`, `fulfillment_id`).
    #[serde(rename = "order.fulfillment")]
    OrderFulfillment,
    /// On a product (`product_id`).
    #[serde(rename = "product.details")]
    ProductDetails,
    /// On a customer (`customer_id`).
    #[serde(rename = "customer.details")]
    CustomerDetails,
    /// Actions on a payment (`payment_id`, `order_id`).
    #[serde(rename = "payment")]
    Payment,
}

/// A storefront target (design note 0029, §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub enum StorefrontTarget {
    /// The payment step (`cart_id`, `order_id`).
    #[serde(rename = "checkout.payment")]
    CheckoutPayment,
    /// On a product page (`product_id`).
    #[serde(rename = "product.block")]
    ProductBlock,
    /// On the cart (`cart_id`).
    #[serde(rename = "cart.block")]
    CartBlock,
    /// On an order's confirmation (`order_id`).
    #[serde(rename = "order.confirmation")]
    OrderConfirmation,
}

impl From<AdminTarget> for Target {
    fn from(t: AdminTarget) -> Self {
        match t {
            AdminTarget::AppsPage => Target::AppsPage,
            AdminTarget::SettingsApp => Target::SettingsApp,
            AdminTarget::OrderDetails => Target::OrderDetails,
            AdminTarget::OrderPayment => Target::OrderPayment,
            AdminTarget::OrderFulfillment => Target::OrderFulfillment,
            AdminTarget::ProductDetails => Target::ProductDetails,
            AdminTarget::CustomerDetails => Target::CustomerDetails,
            AdminTarget::Payment => Target::Payment,
        }
    }
}

impl From<StorefrontTarget> for Target {
    fn from(t: StorefrontTarget) -> Self {
        match t {
            StorefrontTarget::CheckoutPayment => Target::CheckoutPayment,
            StorefrontTarget::ProductBlock => Target::ProductBlock,
            StorefrontTarget::CartBlock => Target::CartBlock,
            StorefrontTarget::OrderConfirmation => Target::OrderConfirmation,
        }
    }
}

/// The DTO of an admin target from the catalog (every contribution listed is in it).
fn admin_target(t: Target) -> Result<AdminTarget, ApiError> {
    Ok(match t {
        Target::AppsPage => AdminTarget::AppsPage,
        Target::SettingsApp => AdminTarget::SettingsApp,
        Target::OrderDetails => AdminTarget::OrderDetails,
        Target::OrderPayment => AdminTarget::OrderPayment,
        Target::OrderFulfillment => AdminTarget::OrderFulfillment,
        Target::ProductDetails => AdminTarget::ProductDetails,
        Target::CustomerDetails => AdminTarget::CustomerDetails,
        Target::Payment => AdminTarget::Payment,
        other => return Err(ApiError::internal(format!("{other} is no admin target"))),
    })
}

fn storefront_target(t: Target) -> Result<StorefrontTarget, ApiError> {
    Ok(match t {
        Target::CheckoutPayment => StorefrontTarget::CheckoutPayment,
        Target::ProductBlock => StorefrontTarget::ProductBlock,
        Target::CartBlock => StorefrontTarget::CartBlock,
        Target::OrderConfirmation => StorefrontTarget::OrderConfirmation,
        other => {
            return Err(ApiError::internal(format!(
                "{other} is no storefront target"
            )));
        }
    })
}

// ---------- discovery ----------

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct AdminTargetQuery {
    /// Only this target's contributions; every admin target's when absent.
    target: Option<AdminTarget>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct StorefrontTargetQuery {
    /// Only this target's elements; every storefront target's when absent.
    target: Option<StorefrontTarget>,
}

/// An app that contributes, for labels.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ExtensionApp {
    /// `app_<name>`.
    key: String,
    name: String,
    /// A square image's URL.
    icon: Option<String>,
}

impl ExtensionApp {
    fn new(app: &App) -> Self {
        let manifest = app.manifest().map(|m| &m.manifest);
        Self {
            key: app.key().to_string(),
            name: app.name().to_string(),
            icon: manifest.and_then(|m| m.icon()).map(str::to_string),
        }
    }
}

/// An embedded page: load `url` in a sandboxed iframe with the bridge
/// (`docs/apps/bridge.md`).
#[derive(Serialize, utoipa::ToSchema)]
pub struct PageExtension {
    app: String,
    id: String,
    title: String,
    /// Absolute: the iframe's `src`.
    url: String,
    /// Where an admin lists it (`apps`, `settings`).
    nav: Option<String>,
    target: AdminTarget,
}

/// A declarative block: fetch its tree from
/// `GET /admin/app-extensions/blocks/{app}/{id}` with the target's context.
#[derive(Serialize, utoipa::ToSchema)]
pub struct BlockExtension {
    app: String,
    id: String,
    target: AdminTarget,
    /// The app's own URL for it (Hypha fetches it; frontends use the blocks route).
    url: String,
}

/// A button: run it with `POST /admin/app-extensions/actions/{app}/{id}`.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ActionExtension {
    app: String,
    id: String,
    label: String,
    target: AdminTarget,
    /// A JSON Schema for a form to show before running it, when it takes input.
    #[schema(value_type = Option<Object>)]
    input_schema: Option<serde_json::Value>,
    /// The extension point it stands for (`payments.capture`), when it names one
    /// instead of a URL: run it through that point's route (capturing the payment),
    /// not the actions route.
    point: Option<String>,
}

/// An app's settings form (§6.1): render the JSON Schema, save through the admin proxy.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SettingsExtension {
    app: String,
    #[schema(value_type = Object)]
    schema: serde_json::Value,
    /// Fields to show masked and never echo back.
    secret_fields: Vec<String>,
    /// Whether the app has an admin proxy to save to.
    proxy: bool,
}

/// Every installed app's admin contributions to a target.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AdminExtensionsResponse {
    /// The apps below, by key.
    apps: Vec<ExtensionApp>,
    pages: Vec<PageExtension>,
    blocks: Vec<BlockExtension>,
    actions: Vec<ActionExtension>,
    settings: Vec<SettingsExtension>,
}

/// What installed apps add to an admin (design note 0029, §6), for `target` or every
/// admin target: embedded pages, declarative blocks, actions and settings forms. Only
/// apps installed through OAuth and enabled contribute, and only where their grant
/// reads the target's object (`orders:read` for order targets, …).
#[utoipa::path(
    get,
    path = "/admin/app-extensions",
    params(AdminTargetQuery),
    responses((status = 200, description = "OK", body = AdminExtensionsResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn list_admin<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: Query<AdminTargetQuery>,
) -> Result<Json<AdminExtensionsResponse>, ApiError> {
    use hypha_apps::HasApps;
    let (ports, ctx) = caller.authenticate(&state).await?;
    let q = query.parse()?;
    let found = admin_contributions(&ctx, ports.apps(), q.target.map(Into::into)).await?;
    let mut response = AdminExtensionsResponse {
        apps: Vec::new(),
        pages: Vec::new(),
        blocks: Vec::new(),
        actions: Vec::new(),
        settings: Vec::new(),
    };
    add_spores(
        &state,
        &ctx,
        &ports,
        q.target.map(Into::into),
        &mut response,
    )
    .await?;
    for c in found {
        let key = c.app.key().to_string();
        response.apps.push(ExtensionApp::new(&c.app));
        for p in c.pages {
            response.pages.push(PageExtension {
                app: key.clone(),
                id: p.page.id,
                title: p.page.title,
                url: p.page.url,
                nav: p.page.nav,
                target: admin_target(p.target)?,
            });
        }
        for (b, t) in c.blocks {
            response.blocks.push(BlockExtension {
                app: key.clone(),
                id: b.id,
                target: admin_target(t)?,
                url: b.url,
            });
        }
        for (a, t) in c.actions {
            response.actions.push(ActionExtension {
                app: key.clone(),
                id: a.id,
                label: a.label,
                target: admin_target(t)?,
                input_schema: a.input_schema,
                point: a.point.filter(|_| a.url.is_none()),
            });
        }
        if let Some(settings) = c.settings {
            response.settings.push(SettingsExtension {
                app: key.clone(),
                schema: settings.schema,
                secret_fields: settings.secret_fields,
                proxy: c.proxy,
            });
        }
    }
    Ok(Json(response))
}

/// What the spores built into the API add (design note 0030, §7), merged into the
/// apps' answer: blocks, actions and settings forms of enabled spores whose declared
/// scopes read the target's object. A spore's block is fetched through the same blocks
/// route (`url` names it), its settings saved through `PUT /admin/apps/{key}/settings`
/// (`proxy` is false).
async fn add_spores<S: Storage>(
    state: &AppState<S>,
    ctx: &Ctx,
    ports: &S::Ports<'_>,
    target: Option<Target>,
    response: &mut AdminExtensionsResponse,
) -> Result<(), ApiError> {
    use hypha_apps::{AppRepo, HasApps};
    use hypha_spore_core::SporeRuntime;
    let spores = state.storage.spores();
    for c in SporeRuntime::<S::Ports<'static>>::contributions(spores) {
        let key = AppKey::of_spore(c.name).map_err(ApiError::internal)?;
        let Some(app) = ports.apps().get(&key).await.map_err(ApiError::internal)? else {
            continue;
        };
        let wanted = |t: Target| target.is_none_or(|w| w == t) && spore_contributes(&app, t);
        let blocks: Vec<BlockExtension> = c
            .blocks
            .iter()
            .filter(|b| b.target.takes_blocks() && wanted(b.target))
            .map(|b| {
                Ok(BlockExtension {
                    app: key.to_string(),
                    id: b.id.to_string(),
                    target: admin_target(b.target)?,
                    url: format!("/admin/app-extensions/blocks/{key}/{}", b.id),
                })
            })
            .collect::<Result<_, ApiError>>()?;
        let actions: Vec<ActionExtension> = c
            .actions
            .iter()
            .filter(|a| a.target.takes_actions() && wanted(a.target))
            .map(|a| {
                Ok(ActionExtension {
                    app: key.to_string(),
                    id: a.id.to_string(),
                    label: a.label.to_string(),
                    target: admin_target(a.target)?,
                    input_schema: a.input_schema.clone(),
                    point: None,
                })
            })
            .collect::<Result<_, ApiError>>()?;
        let settings = SporeRuntime::<S::Ports<'static>>::settings_schema(spores, &key)
            .filter(|_| app.enabled() && target.is_none_or(|t| t == Target::SettingsApp))
            .map(|schema| SettingsExtension {
                app: key.to_string(),
                secret_fields: schema.secrets().map(str::to_string).collect(),
                schema: schema.json().clone(),
                proxy: false,
            });
        if blocks.is_empty() && actions.is_empty() && settings.is_none() {
            continue;
        }
        // Spores' blocks and actions need the caller to read their apps (`apps:read`),
        // as apps' do.
        ctx.require(Scope::AppsRead)
            .map_err(hypha_apps::CommandError::from)?;
        response.apps.push(ExtensionApp {
            key: key.to_string(),
            name: app.name().to_string(),
            icon: None,
        });
        response.blocks.extend(blocks);
        response.actions.extend(actions);
        response.settings.extend(settings);
    }
    Ok(())
}

/// Whether `key` names a spore (`spore_<name>`), whose blocks and actions are answered
/// in-process.
fn is_spore(key: &str) -> bool {
    AppKey::new(key).is_ok_and(|k| k.kind() == AppKind::Spore)
}

/// The spore `app` acting for the caller: its app, with its scopes as far as the caller
/// holds them, as an app's session token carries.
fn spore_ctx(ctx: &Ctx, app: &App) -> Ctx {
    Ctx::new(Actor::App(app.id()), scopes_for(ctx, app))
}

/// A spore's block (design note 0030, §7): asked in-process, checked as an app's.
async fn spore_block<S: Storage>(
    state: &AppState<S>,
    ctx: &Ctx,
    ports: &S::Ports<'_>,
    key: &str,
    id: &str,
    context: &ExtensionContext,
) -> Result<BlockResponse, ApiError> {
    use hypha_apps::HasApps;
    use hypha_spore_core::{SporeRuntime, UiCall, UiError};
    let app = spore_for_ui(ctx, ports.apps(), key).await?;
    let not_found = || hypha_apps::CommandError::ExtensionNotFound {
        app: app.key().clone(),
        id: id.to_string(),
    };
    let spores = state.storage.spores();
    let target = SporeRuntime::<S::Ports<'static>>::contributions(spores)
        .into_iter()
        .filter(|c| app.key().as_str().strip_prefix("spore_") == Some(c.name))
        .flat_map(|c| c.blocks)
        .find(|b| b.id == id && b.target.takes_blocks())
        .map(|b| b.target)
        .ok_or_else(not_found)?;
    may_use_on(ctx, &app, target, context)?;
    let acting = spore_ctx(ctx, &app);
    let call = UiCall {
        target,
        context,
        ctx: &acting,
        staff: staff_of(ctx),
    };
    let answer = SporeRuntime::block(spores, app.key(), id, call, ports)
        .await
        .ok_or_else(|| hypha_apps::CommandError::AppNotFound(key.to_string()))?;
    let unavailable = |reason: String| ExtensionError::Unavailable {
        app: app.key().clone(),
        point: BLOCK_POINT,
        reason,
    };
    let tree = match answer {
        Ok(tree) => tree,
        Err(UiError::NotFound) => return Err(not_found().into()),
        Err(UiError::Failed(reason)) => return Err(unavailable(reason).into()),
    };
    let check = check_block(&tree);
    if let Some(problem) = check.errors.first() {
        let reason = format!(
            "not a block v1: {}: {} ({} problem(s))",
            problem.path,
            problem.message,
            check.errors.len()
        );
        tracing::warn!(app = %app.key(), "blocks: a spore's invalid tree: {reason}");
        return Err(ExtensionError::Invalid {
            app: app.key().clone(),
            point: BLOCK_POINT,
            reason,
        }
        .into());
    }
    Ok(BlockResponse {
        app: app.key().to_string(),
        id: id.to_string(),
        target: admin_target(target)?,
        tree,
    })
}

/// A spore's action (design note 0030, §7): run in-process.
async fn spore_action<S: Storage>(
    state: &AppState<S>,
    ctx: &Ctx,
    ports: &S::Ports<'_>,
    key: &str,
    id: &str,
    context: &ExtensionContext,
    input: Option<serde_json::Value>,
) -> Result<ActionResultResponse, ApiError> {
    use hypha_apps::HasApps;
    use hypha_extensions::ExtensionPoint;
    use hypha_spore_core::{SporeRuntime, UiCall, UiError};
    let app = spore_for_ui(ctx, ports.apps(), key).await?;
    let not_found = || hypha_apps::CommandError::ExtensionNotFound {
        app: app.key().clone(),
        id: id.to_string(),
    };
    let spores = state.storage.spores();
    let target = SporeRuntime::<S::Ports<'static>>::contributions(spores)
        .into_iter()
        .filter(|c| app.key().as_str().strip_prefix("spore_") == Some(c.name))
        .flat_map(|c| c.actions)
        .find(|a| a.id == id && a.target.takes_actions())
        .map(|a| a.target)
        .ok_or_else(not_found)?;
    may_use_on(ctx, &app, target, context)?;
    let acting = spore_ctx(ctx, &app);
    let call = UiCall {
        target,
        context,
        ctx: &acting,
        staff: staff_of(ctx),
    };
    let answer = SporeRuntime::action(spores, app.key(), id, call, input, ports)
        .await
        .ok_or_else(|| hypha_apps::CommandError::AppNotFound(key.to_string()))?;
    match answer {
        Ok(answer) => Ok(ActionResultResponse {
            message: answer.message,
            refresh: answer.refresh,
            navigate: answer.navigate,
        }),
        Err(UiError::NotFound) => Err(not_found().into()),
        Err(UiError::Failed(reason)) => Err(ExtensionError::Unavailable {
            app: app.key().clone(),
            point: RunAdminAction::NAME,
            reason,
        }
        .into()),
    }
}

/// A web component an app adds to a storefront (`docs/apps/elements.md`).
#[derive(Serialize, utoipa::ToSchema)]
pub struct ElementExtension {
    app: String,
    id: String,
    target: StorefrontTarget,
    /// The custom element's tag (`stripe-payment`).
    tag: String,
    /// The ES module that defines it: an absolute URL (`https://`, or `http://localhost`
    /// in development). List its origin in the storefront's `script-src`.
    module: String,
}

/// An app with a storefront proxy (`/storefront/apps/{app}/proxy/…`).
#[derive(Serialize, utoipa::ToSchema)]
pub struct ProxyExtension {
    app: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct StorefrontExtensionsResponse {
    elements: Vec<ElementExtension>,
    /// The apps a storefront may call through the app proxy.
    proxies: Vec<ProxyExtension>,
}

/// What installed apps add to a storefront (design note 0029, §7): elements for
/// `target` (or every storefront target), and which apps have a proxy.
#[utoipa::path(
    get,
    path = "/storefront/app-extensions",
    params(StorefrontTargetQuery),
    responses((status = 200, description = "OK", body = StorefrontExtensionsResponse)),
    extensions(("x-hypha-scopes" = json!(["stores:read"]))),
)]
pub async fn list_storefront<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    query: Query<StorefrontTargetQuery>,
) -> Result<Json<StorefrontExtensionsResponse>, ApiError> {
    use hypha_apps::HasApps;
    let (ports, ctx) = caller.authenticate(&state).await?;
    let q = query.parse()?;
    let found = storefront_contributions(&ctx, ports.apps(), q.target.map(Into::into)).await?;
    let mut response = StorefrontExtensionsResponse {
        elements: Vec::new(),
        proxies: Vec::new(),
    };
    for c in found {
        let key = c.app.key().to_string();
        for (e, t) in c.elements {
            response.elements.push(ElementExtension {
                app: key.clone(),
                id: e.id,
                target: storefront_target(t)?,
                tag: e.tag,
                module: e.module,
            });
        }
        if c.proxy {
            response.proxies.push(ProxyExtension { app: key });
        }
    }
    Ok(Json(response))
}

/// The origins an app's embedded pages may be framed by.
#[derive(Serialize, utoipa::ToSchema)]
pub struct OriginsResponse {
    /// `scheme://host[:port]`: set `Content-Security-Policy: frame-ancestors` to these.
    origins: Vec<String>,
}

/// The store's admin origins, from configuration (`server.admin_url`), for an app's
/// `frame-ancestors` (design note 0029, §6.3). Any admin caller, an app's token
/// included.
#[utoipa::path(
    get,
    path = "/admin/app-extensions/origins",
    responses((status = 200, description = "OK", body = OriginsResponse)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn origins<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<OriginsResponse>, ApiError> {
    let _ = caller.authenticate(&state).await?;
    let origins = state
        .config
        .server
        .admin_url
        .as_deref()
        .and_then(|u| url::Url::parse(u).ok())
        .map(|u| u.origin().ascii_serialization())
        .filter(|o| o != "null")
        .into_iter()
        .collect();
    Ok(Json(OriginsResponse { origins }))
}

// ---------- session tokens ----------

/// A session token for an app's embedded page.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SessionTokenResponse {
    /// A JWT (HS256, the app's secret): pass it to the app through the bridge's
    /// `session_token` answer. The app verifies it with `@hypha/app`'s
    /// `verifySessionToken` (`docs/apps/bridge.md`).
    token: String,
    /// A minute from now (RFC 3339): ask for a new one per call.
    expires_at: String,
}

/// A staff member's session token for an installed app (design note 0029, §6.3): its
/// claims are the API's base URL (`iss`), the app (`aud`), the staff member (`sub`), the
/// store, and the app's scopes as far as the staff member holds them (`scope`). Staff
/// sessions only (API keys: `401 session_required`).
#[utoipa::path(
    post,
    path = "/admin/app-extensions/{app}/session",
    params(("app" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_stripe")),
    responses((status = 200, description = "OK", body = SessionTokenResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn session<S: Storage>(
    State(state): State<SharedState<S>>,
    Issuer(issuer): Issuer,
    caller: StaffCaller,
    PathId(key): PathId<String>,
) -> Result<Json<SessionTokenResponse>, ApiError> {
    use hypha_apps::HasApps;
    let (ports, staff) = caller.authenticate(&state).await?;
    let (app, scopes) = session_app(&staff.ctx, ports.apps(), &key).await?;
    let (token, expires_at) =
        session_token(&state, &issuer, &app, staff.user.id().to_string(), &scopes)?;
    Ok(Json(SessionTokenResponse {
        token,
        expires_at: rfc3339(expires_at)?,
    }))
}

/// A session token for `sub` and the app, and when it expires.
fn session_token<S>(
    state: &AppState<S>,
    iss: &str,
    app: &App,
    sub: String,
    scopes: &[Scope],
) -> Result<(String, hypha_core::Timestamp), ApiError> {
    let secret = open_secret(&state.env, app).map_err(ApiError::internal)?;
    let now = state.clock.now();
    let iat = now.unix_secs();
    let claims = SessionClaims {
        iss: iss.to_string(),
        aud: app.key().to_string(),
        sub,
        store: state.store.to_string(),
        scope: scope_string(scopes),
        iat,
        exp: iat + SESSION_TOKEN_SECONDS,
        jti: state.ids.new_uuid().simple().to_string(),
    };
    Ok((
        sign_session_token(&claims, &secret),
        now.plus_secs(SESSION_TOKEN_SECONDS),
    ))
}

fn scope_string(scopes: &[Scope]) -> String {
    scopes
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------- context ----------

/// The objects a block or action is used on (checked against its target: an order
/// block needs `order_id`, and takes no `product_id`).
#[derive(Debug, Default, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ExtensionContextDto {
    order_id: Option<OrderId>,
    payment_id: Option<PaymentId>,
    fulfillment_id: Option<FulfillmentId>,
    product_id: Option<ProductId>,
    customer_id: Option<CustomerId>,
    cart_id: Option<CartId>,
}

impl ExtensionContextDto {
    fn domain(&self) -> ExtensionContext {
        ExtensionContext {
            order: self.order_id,
            payment: self.payment_id,
            fulfillment: self.fulfillment_id,
            product: self.product_id,
            customer: self.customer_id,
            cart: self.cart_id,
        }
    }
}

/// The staff member behind `ctx`, when it's a staff session.
fn staff_of(ctx: &Ctx) -> Option<hypha_core::UserId> {
    match ctx.actor() {
        Actor::User(user) => Some(user),
        _ => None,
    }
}

// ---------- actions ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RunActionRequest {
    /// The objects it was pressed on.
    #[serde(default)]
    #[schema(required = false)]
    context: ExtensionContextDto,
    /// The form's values when the action has an `input_schema`: an object, passed to
    /// the app as is (the app checks it).
    #[serde(default)]
    #[schema(required = false, value_type = Option<std::collections::HashMap<String, serde_json::Value>>)]
    input: Option<serde_json::Value>,
}

/// What the app answered.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ActionResultResponse {
    /// Plain text to show (a toast).
    message: Option<String>,
    /// Reload what the action was pressed on.
    refresh: bool,
    /// A path within the app's page to go to.
    navigate: Option<String>,
}

/// Runs an app's admin action (design note 0029, §6.1): Hypha calls the action's URL
/// (`admin.action` v1, `openapi/extensions.json`; in the app's call log) and returns
/// the app's answer. 502 `app_unavailable` when the app fails or answers something
/// else. The caller needs `apps:read` and to read the target's object; an action that
/// names an extension point is `409 app_action_has_point`.
#[utoipa::path(
    post,
    path = "/admin/app-extensions/actions/{app}/{id}",
    params(
        ("app" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_stripe"),
        ("id" = String, Path, example = "capture"),
    ),
    request_body = RunActionRequest,
    responses((status = 200, description = "OK", body = ActionResultResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn run_action<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((key, id)): PathId<(String, String)>,
    body: JsonBody<RunActionRequest>,
) -> Result<Json<ActionResultResponse>, ApiError> {
    use hypha_apps::HasApps;
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    if req.input.as_ref().is_some_and(|i| !i.is_object()) {
        return Err(hypha_apps::CommandError::InvalidContext(
            "input is an object of the form's values".to_string(),
        )
        .into());
    }
    let context = req.context.domain();
    if is_spore(&key) {
        let answer = spore_action(&state, &ctx, &ports, &key, &id, &context, req.input).await?;
        return Ok(Json(answer));
    }
    let (app, url, target) = find_action(&ctx, ports.apps(), &key, &id, &context).await?;
    let secret = open_secret(&state.env, &app).map_err(ApiError::internal)?;
    let request = AdminActionRequest {
        action: id,
        target: target.as_str().to_string(),
        context: ActionContext {
            order_id: context.order,
            payment_id: context.payment,
            fulfillment_id: context.fulfillment,
            product_id: context.product,
            customer_id: context.customer,
            cart_id: context.cart,
        },
        input: req.input,
        staff: staff_of(&ctx),
        scope: scope_string(&scopes_for(&ctx, &app)),
    };
    let endpoint = Endpoint {
        app: app.key(),
        url: &url,
        secret: &secret,
        timeout: None,
        headers: &[],
    };
    // Every press is its own piece of work.
    let idempotency = state.ids.new_uuid().simple().to_string();
    let answer = call::<RunAdminAction>(
        &state.extensions(),
        ports.extension_calls(),
        &state.env,
        &endpoint,
        &request,
        &idempotency,
        context.correlation().as_deref(),
    )
    .await?;
    Ok(Json(ActionResultResponse {
        message: answer.message,
        refresh: answer.refresh,
        navigate: answer.navigate,
    }))
}

// ---------- blocks ----------

/// A block's UI tree, checked against the block format v1
/// (`packages/app-blocks/schema/block.v1.json`).
#[derive(Serialize, utoipa::ToSchema)]
pub struct BlockResponse {
    app: String,
    id: String,
    target: AdminTarget,
    /// `{version: 1, root}`: render it with your own components (`@hypha/app-blocks`
    /// for React). Elements of types newer than v1 may appear: skip them.
    #[schema(value_type = Object)]
    tree: serde_json::Value,
}

/// An app's block for the objects in the query (design note 0029, §6.2): Hypha fetches
/// the block's URL from the app (a signed `GET` with the context in the query and in
/// `Hypha-*` headers, and a session token in `Authorization` for staff), checks the
/// tree, and returns it; nothing is cached. 502 `app_unavailable` when the app fails or
/// answers something that isn't a v1 tree (the call log says which).
#[utoipa::path(
    get,
    path = "/admin/app-extensions/blocks/{app}/{id}",
    params(
        ("app" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_stripe"),
        ("id" = String, Path, example = "payment"),
        ExtensionContextDto,
    ),
    responses((status = 200, description = "OK", body = BlockResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn fetch_block<S: Storage>(
    State(state): State<SharedState<S>>,
    Issuer(issuer): Issuer,
    caller: Caller,
    PathId((key, id)): PathId<(String, String)>,
    query: Query<ExtensionContextDto>,
) -> Result<Json<BlockResponse>, ApiError> {
    use hypha_apps::HasApps;
    let (ports, ctx) = caller.authenticate(&state).await?;
    let context = query.parse()?.domain();
    if is_spore(&key) {
        let block = spore_block(&state, &ctx, &ports, &key, &id, &context).await?;
        return Ok(Json(block));
    }
    let (app, block, target) = find_block(&ctx, ports.apps(), &key, &id, &context).await?;
    let secret = open_secret(&state.env, &app).map_err(ApiError::internal)?;
    let pairs: Vec<(&str, String)> = context
        .pairs()
        .into_iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    let mut url = url::Url::parse(&block.url).map_err(ApiError::internal)?;
    if !pairs.is_empty() {
        url.query_pairs_mut()
            .extend_pairs(pairs.iter().map(|(k, v)| (*k, v.as_str())));
    }
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs.iter().map(|(k, v)| (*k, v.as_str())))
        .finish();
    let mut forwarded = context_headers(&state, &ctx, &app);
    forwarded.push(("Hypha-Target".to_string(), target.as_str().to_string()));
    forwarded.push(("Hypha-Block".to_string(), block.id.clone()));
    forwarded.push(("Hypha-Context".to_string(), encoded));
    forwarded.push(("accept".to_string(), "application/json".to_string()));
    if let Some(user) = staff_of(&ctx) {
        let scopes = scopes_for(&ctx, &app);
        let (token, _) = session_token(&state, &issuer, &app, user.to_string(), &scopes)?;
        forwarded.push(("authorization".to_string(), format!("Bearer {token}")));
    }
    let started = state.clock.now();
    let sent = send_forward(
        &state,
        ForwardCall {
            method: Method::Get,
            url: url.as_str(),
            timeout: BLOCK_TIMEOUT,
            secret: &secret,
            at: started.unix_secs(),
            content_type: None,
            headers: forwarded,
            body: Vec::new(),
            max_answer: hypha_extensions::MAX_ANSWER_BYTES,
        },
    )
    .await;
    let ended = state.clock.now();
    let (status, judged) = judge_block(&app, sent);
    let (outcome, error) = match &judged {
        Ok(_) => (CallOutcome::Ok, None),
        Err((outcome, reason)) => (*outcome, Some(reason.clone())),
    };
    let correlation = context.correlation();
    hypha_extensions::record_call(
        ports.extension_calls(),
        &state.ids,
        hypha_extensions::Finished {
            app: app.key(),
            point: BLOCK_POINT,
            version: 1,
            started_at: started,
            ended_at: ended,
            outcome,
            status,
            error,
            correlation: correlation.as_deref(),
        },
    )
    .await;
    let tree = judged.map_err(|(outcome, reason)| {
        let app = app.key().clone();
        ApiError::from(match outcome {
            CallOutcome::Timeout => ExtensionError::Timeout {
                app,
                point: BLOCK_POINT,
            },
            CallOutcome::Invalid => ExtensionError::Invalid {
                app,
                point: BLOCK_POINT,
                reason,
            },
            _ => ExtensionError::Unavailable {
                app,
                point: BLOCK_POINT,
                reason,
            },
        })
    })?;
    Ok(Json(BlockResponse {
        app: app.key().to_string(),
        id: block.id,
        target: admin_target(target)?,
        tree,
    }))
}

/// What a block's answer means: its HTTP status when there was an answer (whatever
/// it was, for the call log), and the checked tree, or the outcome and reason to record.
#[allow(clippy::type_complexity)]
fn judge_block(
    app: &App,
    sent: Result<crate::outbound::OutboundAnswer, CallError>,
) -> (
    Option<u16>,
    Result<serde_json::Value, (CallOutcome, String)>,
) {
    let answer = match sent {
        Ok(answer) => answer,
        Err(CallError::Timeout) => {
            let reason = format!("no answer within {} ms", BLOCK_TIMEOUT.as_millis());
            return (None, Err((CallOutcome::Timeout, reason)));
        }
        Err(CallError::TooLarge) => {
            let reason = format!(
                "answered more than {} bytes",
                hypha_extensions::MAX_ANSWER_BYTES
            );
            return (None, Err((CallOutcome::Invalid, reason)));
        }
        Err(CallError::Failed(reason)) => return (None, Err((CallOutcome::Failed, reason))),
    };
    let status = Some(answer.status);
    if !(200..300).contains(&answer.status) {
        let reason = format!("answered HTTP {}", answer.status);
        return (status, Err((CallOutcome::Failed, reason)));
    }
    let tree = match serde_json::from_slice::<serde_json::Value>(&answer.body) {
        Ok(tree) => tree,
        Err(e) => {
            return (
                status,
                Err((CallOutcome::Invalid, format!("not JSON: {e}"))),
            );
        }
    };
    let check = check_block(&tree);
    if let Some(problem) = check.errors.first() {
        let reason = format!(
            "not a block v1: {}: {} ({} problem(s))",
            problem.path,
            problem.message,
            check.errors.len()
        );
        tracing::info!(app = %app.key(), "blocks: an invalid tree: {reason}");
        return (status, Err((CallOutcome::Invalid, reason)));
    }
    (status, Ok(tree))
}

// ---------- the proxy ----------

/// The context every forwarded admin-side call carries: the store, the caller's
/// staff ID (sessions only) and the app's scopes as far as the caller holds them.
fn context_headers<S>(state: &AppState<S>, ctx: &Ctx, app: &App) -> Vec<(String, String)> {
    let mut headers = vec![
        ("Hypha-Store".to_string(), state.store.to_string()),
        (
            "Hypha-Scopes".to_string(),
            scope_string(&scopes_for(ctx, app)),
        ),
    ];
    if let Some(user) = staff_of(ctx) {
        headers.push(("Hypha-Staff".to_string(), user.to_string()));
    }
    headers
}

/// Prepares (signs, checks the URL) and sends a forwarded call.
async fn send_forward<S>(
    state: &AppState<S>,
    call: ForwardCall<'_>,
) -> Result<crate::outbound::OutboundAnswer, CallError> {
    let prepared = prepare_forward(state.outbound(), call)?;
    state.calls.send(prepared).await
}

/// The part of `uri`'s path after `/proxy/`, as sent (still percent-encoded), when no
/// segment climbs out (`.`, `..`, in any encoding) or holds a backslash.
fn proxied_path(uri: &Uri) -> Result<String, ApiError> {
    let path = uri.path();
    let rest = path
        .split_once("/proxy/")
        .map(|(_, rest)| rest)
        .ok_or_else(ApiError::invalid_proxy_path)?;
    for segment in rest.split('/') {
        let decoded = percent_decode(segment);
        if decoded == "." || decoded == ".." || decoded.contains('\\') {
            return Err(ApiError::invalid_proxy_path());
        }
    }
    Ok(rest.to_string())
}

/// `segment` with its `%XX` escapes decoded (lossily: only for checking it).
fn percent_decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Forwards one request to the app's proxy URL (design note 0029, §8) and passes its
/// answer back: the status, the content type and the body, nothing else (no cookies).
#[allow(clippy::too_many_arguments)]
async fn forward<S: Storage>(
    state: &AppState<S>,
    ctx: &Ctx,
    app: &App,
    base: &str,
    method: Method,
    uri: &Uri,
    headers: &HeaderMap,
    mut context: Vec<(String, String)>,
    body: Body,
) -> Result<Response, ApiError> {
    let rest = proxied_path(uri)?;
    let body = axum::body::to_bytes(body, MAX_PROXY_BYTES)
        .await
        .map_err(|_| ApiError::body_too_large(MAX_PROXY_BYTES))?;
    let secret = open_secret(&state.env, app).map_err(ApiError::internal)?;
    let tail = match uri.query() {
        Some(q) => format!("{rest}?{q}"),
        None => rest,
    };
    let url = format!("{}/{tail}", base.trim_end_matches('/'));
    context.push(("Hypha-Path".to_string(), format!("/{tail}")));
    if let Some(accept) = headers.get("accept").and_then(|v| v.to_str().ok()) {
        context.push(("accept".to_string(), accept.to_string()));
    }
    let _ = ctx;
    let content_type = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok());
    let sent = send_forward(
        state,
        ForwardCall {
            method,
            url: &url,
            timeout: PROXY_TIMEOUT,
            secret: &secret,
            at: state.clock.now().unix_secs(),
            content_type,
            headers: context,
            body: body.to_vec(),
            max_answer: MAX_PROXY_BYTES,
        },
    )
    .await;
    let answer = sent.map_err(|e| {
        let app = app.key().clone();
        ApiError::from(match e {
            CallError::Timeout => ExtensionError::Timeout {
                app,
                point: PROXY_POINT,
            },
            CallError::TooLarge => ExtensionError::Invalid {
                app,
                point: PROXY_POINT,
                reason: format!("answered more than {MAX_PROXY_BYTES} bytes"),
            },
            CallError::Failed(reason) => ExtensionError::Unavailable {
                app,
                point: PROXY_POINT,
                reason,
            },
        })
    })?;
    let status = StatusCode::from_u16(answer.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut response = (status, answer.body).into_response();
    if let Some(value) = answer
        .content_type
        .as_deref()
        .and_then(|v| HeaderValue::from_str(v).ok())
    {
        response.headers_mut().insert(CONTENT_TYPE, value);
    }
    Ok(response)
}

/// A request for an app's proxy, as its handler took it apart.
struct ProxyRequest {
    /// The API's public base URL, for staff's session tokens.
    issuer: String,
    key: String,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
}

async fn admin_proxy<S: Storage>(
    state: SharedState<S>,
    caller: Caller,
    req: ProxyRequest,
) -> Result<Response, ApiError> {
    use hypha_apps::HasApps;
    let ProxyRequest {
        issuer,
        key,
        method,
        uri,
        headers,
        body,
    } = req;
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (app, base) = find_proxy(&ctx, ports.apps(), &key, Surface::Admin).await?;
    drop(ports);
    let mut context = context_headers(&state, &ctx, &app);
    if let Some(user) = staff_of(&ctx) {
        let scopes = scopes_for(&ctx, &app);
        let (token, _) = session_token(&state, &issuer, &app, user.to_string(), &scopes)?;
        context.push(("authorization".to_string(), format!("Bearer {token}")));
    }
    forward(
        &state, &ctx, &app, &base, method, &uri, &headers, context, body,
    )
    .await
}

/// The header a storefront names the shopper's cart in.
pub const CART_HEADER: &str = "x-hypha-cart";

async fn storefront_proxy<S: Storage>(
    state: SharedState<S>,
    caller: Caller<Storefront>,
    req: ProxyRequest,
) -> Result<Response, ApiError> {
    use hypha_apps::HasApps;
    // Shoppers get no session token: the issuer isn't needed.
    let ProxyRequest {
        key,
        method,
        uri,
        headers,
        body,
        ..
    } = req;
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (app, base) = find_proxy(&ctx, ports.apps(), &key, Surface::Storefront).await?;
    let mut context = vec![("Hypha-Store".to_string(), state.store.to_string())];
    if let Some(channel) = ctx.channel() {
        context.push(("Hypha-Channel".to_string(), channel.to_string()));
    }
    if let Some(raw) = headers.get(CART_HEADER) {
        let cart: CartId = raw
            .to_str()
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(ApiError::invalid_cart_header)?;
        let cart = hypha_cart::find_cart(&ctx, &ports, cart).await?;
        context.push(("Hypha-Cart".to_string(), cart.id().to_string()));
        context.push(("Hypha-Country".to_string(), cart.country().to_string()));
        if let Some(customer) = cart.customer() {
            context.push(("Hypha-Customer".to_string(), customer.to_string()));
        }
    }
    drop(ports);
    forward(
        &state, &ctx, &app, &base, method, &uri, &headers, context, body,
    )
    .await
}

/// The proxy's handlers, one per method and surface (the route table names one handler
/// per row, and each operation is documented).
macro_rules! proxy_handlers {
    ($($name:ident, $proxy:ident, $caller:ty, $verb:ident, $method:ident, $path:literal, $scope:literal, $doc:literal;)*) => {$(
        #[doc = $doc]
        #[utoipa::path(
            $verb,
            path = $path,
            params(
                ("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_reviews"),
                ("path" = String, Path, example = "settings", description = "The rest of the path, sent on to the app (`a/b?c=d` too); it may hold slashes."),
            ),
            responses(
                (status = 200, description = "The app's answer: its status, `Content-Type` and body (at most 1 MiB), nothing else"),
                (status = 502, description = "The app didn't answer in 10 s, or couldn't be reached (`app_unavailable`, `details: {app, point: \"proxy\"}`)", body = crate::error::ApiErrorBody),
            ),
            extensions(("x-hypha-scopes" = json!([$scope]))),
        )]
        pub async fn $name<S: Storage>(
            State(state): State<SharedState<S>>,
            Issuer(issuer): Issuer,
            caller: $caller,
            PathId((key, _path)): PathId<(String, String)>,
            uri: Uri,
            headers: HeaderMap,
            body: Body,
        ) -> Result<Response, ApiError> {
            let req = ProxyRequest { issuer, key, method: Method::$method, uri, headers, body };
            $proxy(state, caller, req).await
        }
    )*};
}

proxy_handlers! {
    admin_proxy_get, admin_proxy, Caller, get, Get, "/admin/apps/{key}/proxy/{*path}", "apps:read",
        "Forwards a `GET` to the app's admin proxy (design note 0029, §8), signed, with `Hypha-Store`, `Hypha-Staff` (staff sessions), `Hypha-Scopes`, `Hypha-Path` and a session token in `Authorization` for staff.";
    admin_proxy_post, admin_proxy, Caller, post, Post, "/admin/apps/{key}/proxy/{*path}", "apps:read",
        "Forwards a `POST` (any content type, at most 1 MiB) to the app's admin proxy, as `GET` does.";
    admin_proxy_put, admin_proxy, Caller, put, Put, "/admin/apps/{key}/proxy/{*path}", "apps:read",
        "Forwards a `PUT` to the app's admin proxy, as `POST` does.";
    admin_proxy_patch, admin_proxy, Caller, patch, Patch, "/admin/apps/{key}/proxy/{*path}", "apps:read",
        "Forwards a `PATCH` to the app's admin proxy, as `POST` does.";
    admin_proxy_delete, admin_proxy, Caller, delete, Delete, "/admin/apps/{key}/proxy/{*path}", "apps:read",
        "Forwards a `DELETE` to the app's admin proxy, as `POST` does.";
    storefront_proxy_get, storefront_proxy, Caller<Storefront>, get, Get, "/storefront/apps/{key}/proxy/{*path}", "stores:read",
        "Forwards a `GET` to the app's storefront proxy (design note 0029, §8), signed, with `Hypha-Store`, `Hypha-Channel` and `Hypha-Path`; with an `X-Hypha-Cart: cart_…` header (a cart of the key's channel), also `Hypha-Cart`, `Hypha-Country` and `Hypha-Customer` (when the cart has one). No personal data is forwarded.";
    storefront_proxy_post, storefront_proxy, Caller<Storefront>, post, Post, "/storefront/apps/{key}/proxy/{*path}", "stores:read",
        "Forwards a `POST` (any content type, at most 1 MiB) to the app's storefront proxy, as `GET` does.";
    storefront_proxy_put, storefront_proxy, Caller<Storefront>, put, Put, "/storefront/apps/{key}/proxy/{*path}", "stores:read",
        "Forwards a `PUT` to the app's storefront proxy, as `POST` does.";
    storefront_proxy_patch, storefront_proxy, Caller<Storefront>, patch, Patch, "/storefront/apps/{key}/proxy/{*path}", "stores:read",
        "Forwards a `PATCH` to the app's storefront proxy, as `POST` does.";
    storefront_proxy_delete, storefront_proxy, Caller<Storefront>, delete, Delete, "/storefront/apps/{key}/proxy/{*path}", "stores:read",
        "Forwards a `DELETE` to the app's storefront proxy, as `POST` does.";
}

// ---------- refreshing a manifest ----------

/// Whether a refresh needs staff.
#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefreshStatus {
    /// The manifest's registrations are in force; scopes it dropped left the grant.
    Updated,
    /// It also asks for new scopes: staff approve them at `consent_url`.
    ConsentRequired,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RefreshResponse {
    status: RefreshStatus,
    app: AppResponse,
    /// With `consent_required`: the authorization request staff answer.
    request: Option<OAuthRequestId>,
    /// With `consent_required`: the admin's consent page for it.
    consent_url: Option<String>,
}

/// Fetches an installed app's manifest again (design note 0029, §4.2) and applies its
/// registrations (webhooks, UI). Scopes it no longer asks for leave the grant at once;
/// new ones make an authorization request staff approve (`consent_required`): approving
/// widens the grant, and the app then gets tokens with them from `/oauth/authorize`
/// without asking again. Staff with `apps:write`, or the app itself with its token.
/// `manifest_unreachable` / `manifest_invalid` (422) when the manifest can't be used;
/// `app_not_installed` (409) for an app not installed through OAuth.
#[utoipa::path(
    post,
    path = "/admin/apps/{key}/refresh",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_erp")),
    responses((status = 200, description = "OK", body = RefreshResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn refresh<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<Json<RefreshResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let refreshed = refresh_manifest(&ctx, &ports, &state.manifests(), &state.env, &key).await?;
    Ok(Json(match refreshed {
        Refreshed::Updated(app) => RefreshResponse {
            status: RefreshStatus::Updated,
            app: AppResponse::new(&app)?,
            request: None,
            consent_url: None,
        },
        Refreshed::ConsentRequired(app, request) => {
            let admin = state
                .config
                .server
                .admin_url
                .clone()
                .ok_or_else(ApiError::admin_url_missing)?;
            RefreshResponse {
                status: RefreshStatus::ConsentRequired,
                app: AppResponse::new(&app)?,
                request: Some(request.id()),
                consent_url: Some(format!("{admin}/apps/consent?request={}", request.id())),
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn a_proxied_path_is_the_rest_after_proxy_as_sent() {
        assert_eq!(
            proxied_path(&uri("/admin/apps/app_x/proxy/a/b%20c?q=1")).unwrap(),
            "a/b%20c"
        );
    }

    #[test]
    fn a_proxied_path_cannot_climb_out() {
        for bad in [
            "/admin/apps/app_x/proxy/../admin",
            "/admin/apps/app_x/proxy/a/%2e%2e/b",
            "/admin/apps/app_x/proxy/%2E",
            "/admin/apps/app_x/proxy/a%5cb",
        ] {
            assert!(proxied_path(&uri(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn every_catalog_target_has_its_surfaces_dto() {
        for target in Target::ALL {
            match target.surface() {
                Surface::Admin => assert_eq!(Target::from(admin_target(target).unwrap()), target),
                Surface::Storefront => {
                    assert_eq!(Target::from(storefront_target(target).unwrap()), target)
                }
            }
        }
    }
}
