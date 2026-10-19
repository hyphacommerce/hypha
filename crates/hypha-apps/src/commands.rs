use hypha_core::{
    AppId, AuthError, Clock, Ctx, DeliveryId, DeliveryRecord, DeliveryStatus, ErrorCode, EventId,
    HasOutbox, IdGenerator, OutboxRepo, Page, PageRequest, Random, RepoError, Scope, SealError,
    Sealed, SecretBox, StoredEvent,
};
use thiserror::Error;

use crate::{App, AppError, AppKey, AppRepo, HasApps, NewApp, Webhook, repo::keys};

/// The prefix of app signing secrets: recognisable in logs and secret scanners. Apps
/// that came from shipping's rate providers keep the `hypha_shs_` secret they had.
pub const SECRET_PREFIX: &str = "hypha_as_";

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    App(#[from] AppError),
    /// The key as the caller sent it: one that can't be a key is no app either, since
    /// keys only arrive in URLs here (404, not 422).
    #[error("app {0:?} not found")]
    AppNotFound(String),
    #[error("app {0} exists already")]
    AppExists(AppKey),
    #[error("app {app} has no delivery {delivery}")]
    DeliveryNotFound { app: AppKey, delivery: DeliveryId },
    /// Not kept (pruned after 30 days) or never written.
    #[error("event {0} not found")]
    EventNotFound(EventId),
    /// The app has no action, block or other UI contribution `id` in a target this
    /// Hypha knows (design note 0029, §6).
    #[error("app {app} has no UI extension {id:?}")]
    ExtensionNotFound { app: AppKey, id: String },
    /// The app's manifest names no proxy URL for the surface asked (§8).
    #[error("app {0} has no proxy here")]
    ProxyNotFound(AppKey),
    /// The objects passed don't fit the contribution's target (`order.details needs
    /// order_id`).
    #[error("{0}")]
    InvalidContext(String),
    /// The action names an extension point (`payments.capture`): it runs through that
    /// point's own route (capturing the payment), not as an app action.
    #[error("action {id:?} of {app} runs through {point}, not as an app action")]
    ActionHasPoint {
        app: AppKey,
        id: String,
        point: String,
    },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::App(e) => e.code(),
            CommandError::AppNotFound(_) => "app_not_found",
            CommandError::AppExists(_) => "app_exists",
            CommandError::DeliveryNotFound { .. } => "delivery_not_found",
            CommandError::EventNotFound(_) => "event_not_found",
            CommandError::ExtensionNotFound { .. } => "app_extension_not_found",
            CommandError::ProxyNotFound(_) => "app_proxy_not_found",
            CommandError::InvalidContext(_) => "invalid_extension_context",
            CommandError::ActionHasPoint { .. } => "app_action_has_point",
        }
    }
}

/// A webhook endpoint as a request gives it.
pub struct WebhookInput {
    pub url: String,
    pub timeout_ms: Option<u32>,
}

pub struct CreateApp {
    /// `app_<name>`.
    pub key: String,
    pub name: String,
    pub webhook: Option<WebhookInput>,
    /// Event types, `kind.*` or `*`.
    pub subscriptions: Vec<String>,
}

/// Registers an app and returns it with its signing secret, shown this once; Hypha
/// keeps it sealed.
pub async fn create_app<R: AppRepo>(
    ctx: &Ctx,
    apps: &R,
    env: &(impl Random + IdGenerator),
    secrets: &impl SecretBox,
    input: CreateApp,
) -> Result<(App, String), CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let key = AppKey::new_app(&input.key).map_err(AppError::from)?;
    let webhook = input
        .webhook
        .map(|w| Webhook::new(&w.url, w.timeout_ms))
        .transpose()?;
    let (secret, sealed) = new_secret(&key, env, secrets);
    let mut app = App::new(NewApp {
        key: key.clone(),
        id: AppId::from_uuid(env.new_uuid()),
        name: input.name,
        secret: sealed,
        webhook,
        subscriptions: input.subscriptions,
    })?;
    if apps.get(&key).await?.is_some() {
        return Err(CommandError::AppExists(key));
    }
    match apps.insert(&mut app).await {
        Err(RepoError::Duplicate(k)) if k == keys::APP_KEY => Err(CommandError::AppExists(key)),
        written => {
            written?;
            Ok((app, secret))
        }
    }
}

pub async fn get_app<R: AppRepo>(ctx: &Ctx, apps: &R, key: &str) -> Result<App, CommandError> {
    ctx.require(Scope::AppsRead)?;
    load(apps, key).await
}

pub async fn list_apps<R: AppRepo>(ctx: &Ctx, apps: &R) -> Result<Vec<App>, CommandError> {
    ctx.require(Scope::AppsRead)?;
    Ok(apps.list().await?)
}

/// Changes only the fields that are `Some`; `webhook: Some(None)` removes the webhook.
#[derive(Default)]
pub struct UpdateApp {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub webhook: Option<Option<WebhookInput>>,
    pub subscriptions: Option<Vec<String>>,
}

pub async fn update_app<R: AppRepo>(
    ctx: &Ctx,
    apps: &R,
    key: &str,
    input: UpdateApp,
) -> Result<App, CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let mut app = load(apps, key).await?;
    if let Some(name) = &input.name {
        app.rename(name)?;
    }
    if let Some(enabled) = input.enabled {
        app.set_enabled(enabled);
    }
    if let Some(webhook) = input.webhook {
        let webhook = webhook
            .map(|w| Webhook::new(&w.url, w.timeout_ms))
            .transpose()?;
        app.set_webhook(webhook);
    }
    if let Some(subscriptions) = input.subscriptions {
        app.set_subscriptions(subscriptions)?;
    }
    apps.update(&mut app).await?;
    Ok(app)
}

/// Replaces the signing secret and returns the new one, shown this once. Webhooks and
/// rate callbacks signed with the old one stop verifying at once.
pub async fn rotate_app_secret<R: AppRepo>(
    ctx: &Ctx,
    apps: &R,
    random: &impl Random,
    secrets: &impl SecretBox,
    key: &str,
) -> Result<(App, String), CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let mut app = load(apps, key).await?;
    let (secret, sealed) = new_secret(app.key(), random, secrets);
    app.set_secret(sealed);
    apps.update(&mut app).await?;
    Ok((app, secret))
}

/// Removes an app, and with it its rate settings (shipping options that name it stop
/// being offered until changed) and any deliveries still due to it, which fail.
pub async fn delete_app<R: AppRepo>(ctx: &Ctx, apps: &R, key: &str) -> Result<(), CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let app = load(apps, key).await?;
    Ok(apps.delete(app.key()).await?)
}

/// An app's webhook deliveries, newest event first, only those in `status` when given.
/// `page.after` is the last delivery's `event_seq`.
pub async fn list_deliveries<S: HasApps + HasOutbox>(
    ctx: &Ctx,
    ports: &S,
    key: &str,
    status: Option<DeliveryStatus>,
    page: PageRequest<i64>,
) -> Result<Page<DeliveryRecord>, CommandError> {
    ctx.require(Scope::AppsRead)?;
    let app = load(ports.apps(), key).await?;
    Ok(ports
        .outbox()
        .deliveries_to(&app.key().destination(), status, page)
        .await?)
}

/// Sends a delivery again as soon as the dispatcher runs (a failed one included, with
/// one more attempt; design note 0016, Q6). One that's running right now is left alone
/// and returned as it is.
pub async fn retry_delivery<S: HasApps + HasOutbox>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    key: &str,
    delivery: DeliveryId,
) -> Result<DeliveryRecord, CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let app = load(ports.apps(), key).await?;
    ports
        .outbox()
        .retry_delivery(&app.key().destination(), delivery.as_uuid(), clock.now())
        .await?
        .ok_or_else(|| CommandError::DeliveryNotFound {
            app: app.key().clone(),
            delivery,
        })
}

/// One event from the outbox, for debugging a delivery. Under `apps:read`: it's what
/// apps are sent, and what an app may be sent is what reading apps covers.
pub async fn get_event<O: OutboxRepo>(
    ctx: &Ctx,
    outbox: &O,
    id: EventId,
) -> Result<StoredEvent, CommandError> {
    ctx.require(Scope::AppsRead)?;
    outbox
        .get_event(id.as_uuid())
        .await?
        .ok_or(CommandError::EventNotFound(id))
}

/// The signing secret in plain text, for the host's webhook and rate clients. Never
/// returned by an API: only `create_app` and `rotate_app_secret` show it.
pub fn open_secret(secrets: &impl SecretBox, app: &App) -> Result<String, SealError> {
    let plain = secrets.open(app.secret(), app.key().as_str().as_bytes())?;
    String::from_utf8(plain).map_err(|_| SealError("secret isn't UTF-8"))
}

async fn load<R: AppRepo>(apps: &R, key: &str) -> Result<App, CommandError> {
    let not_found = || CommandError::AppNotFound(key.to_string());
    let parsed = AppKey::new(key).map_err(|_| not_found())?;
    apps.get(&parsed).await?.ok_or_else(not_found)
}

/// 32 random bytes as hex after the prefix, and that string sealed with the key as
/// context (as shipping sealed its providers' secrets, so those carried over as is).
pub fn new_secret(
    key: &AppKey,
    random: &impl Random,
    secrets: &impl SecretBox,
) -> (String, Sealed) {
    let mut bytes = [0u8; 32];
    random.fill(&mut bytes);
    let mut secret = String::with_capacity(SECRET_PREFIX.len() + 64);
    secret.push_str(SECRET_PREFIX);
    for b in bytes {
        secret.push_str(&format!("{b:02x}"));
    }
    let mut nonce = [0u8; 24];
    random.fill(&mut nonce);
    let sealed = secrets.seal(secret.as_bytes(), key.as_str().as_bytes(), nonce);
    (secret, sealed)
}
