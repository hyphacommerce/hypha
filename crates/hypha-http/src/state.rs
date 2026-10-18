use std::sync::Arc;

use hypha_auth::{
    Argon2id, AuthSettings, HasApiKeys, HasAuthSettings, HashError, Password, PasswordHash,
    PasswordHasher, SealError, Sealed, SecretBox, XChaChaBox,
};
use hypha_checkout::CheckoutPorts;
use hypha_core::HasBatch;
use hypha_core::{Clock, IdGenerator, OrderId, Random, StoreId, Timestamp};
use hypha_stores::StoreRepo;
use hypha_users::StaffPorts;
use uuid::Uuid;

use crate::error::ApiError;
use crate::host::{
    AfterWrites, HostBackground, HostCalls, HostClock, HostIds, HostLimiter, HostRandom, Offload,
};

/// `HYPHA_SECRET_KEY`: a 32-byte key written as 64 hex characters.
pub fn parse_secret_key(hex: &str) -> Result<[u8; 32], String> {
    let hex = hex.trim();
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("expected 64 hex characters (32 bytes)".to_string());
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    Ok(key)
}

/// The prefix of an order's access token (`HostEnv::order_token`).
const ORDER_TOKEN_PREFIX: &str = "hypha_ord_";

/// What a host's capabilities give `HostEnv` (`host.rs`): time, IDs, randomness and
/// where CPU-heavy work runs.
#[derive(Clone)]
pub struct Runtime {
    pub clock: HostClock,
    pub ids: HostIds,
    pub random: HostRandom,
    pub offload: Offload,
}

/// Orders' access tokens (design note 0024, §7): `hypha_ord_` and a hex HMAC-SHA256 of
/// the order's ID under a key derived from `HYPHA_SECRET_KEY`. Shared by `HostEnv` (the
/// routes) and `MailHost` (the links in emails), so both make the same tokens.
#[derive(Clone)]
pub struct OrderTokens {
    key: [u8; 32],
}

impl OrderTokens {
    /// From `HYPHA_SECRET_KEY`: the same key gives the same tokens across restarts.
    pub fn new(secret_key: [u8; 32]) -> Self {
        use sha2::Digest;
        let key = sha2::Sha256::new()
            .chain_update(b"hypha order access token\0")
            .chain_update(secret_key)
            .finalize()
            .into();
        Self { key }
    }

    /// The token that lets a buyer read the order they placed. Derived rather than
    /// stored, so a replayed checkout answers with the same token, and nothing needs
    /// migrating; a new `HYPHA_SECRET_KEY` makes old tokens stop working.
    pub fn token(&self, order: OrderId) -> String {
        use hmac::Mac;
        let hex: String = self
            .mac(order)
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("{ORDER_TOKEN_PREFIX}{hex}")
    }

    /// Whether `token` is `order`'s access token, compared in constant time.
    pub fn verify(&self, order: OrderId, token: &str) -> bool {
        use hmac::Mac;
        let Some(hex) = token.strip_prefix(ORDER_TOKEN_PREFIX) else {
            return false;
        };
        let bytes: Option<Vec<u8>> = (0..hex.len())
            .step_by(2)
            .map(|i| {
                hex.get(i..i + 2)
                    .and_then(|b| u8::from_str_radix(b, 16).ok())
            })
            .collect();
        match bytes {
            Some(bytes) if hex.len() == 64 => self.mac(order).verify_slice(&bytes).is_ok(),
            _ => false,
        }
    }

    fn mac(&self, order: OrderId) -> hmac::Hmac<sha2::Sha256> {
        use hmac::{KeyInit, Mac};
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&self.key)
            .unwrap_or_else(|_| unreachable!("HMAC accepts every key length"));
        mac.update(order.to_string().as_bytes());
        mac
    }
}

/// The non-storage ports auth commands use (`hypha_auth::AuthEnv`), for real. The staff
/// flows' emails go through `crate::mail::StaffEnv`, which adds the `Notifier`.
pub struct HostEnv {
    runtime: Runtime,
    secret_box: XChaChaBox,
    /// Signs orders' access tokens (`order_token`).
    order_tokens: OrderTokens,
    hasher: Argon2id,
    /// Session, second-factor, reset and invite lifetimes (`[auth]`, design note 0017).
    auth: AuthSettings,
}

impl HostEnv {
    /// `secret_key` seals TOTP secrets (`HYPHA_SECRET_KEY`); losing it makes every
    /// authenticator unreadable, so it must stay the same across restarts.
    pub fn new(secret_key: [u8; 32], auth: AuthSettings, runtime: Runtime) -> Self {
        Self {
            runtime,
            secret_box: XChaChaBox::new(secret_key),
            order_tokens: OrderTokens::new(secret_key),
            hasher: Argon2id::OWASP,
            auth,
        }
    }

    /// The host's runtime: its clock, IDs, randomness and where CPU-heavy work runs.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// Other Argon2 parameters (tests use cheap ones).
    pub fn with_hasher(mut self, hasher: Argon2id) -> Self {
        self.hasher = hasher;
        self
    }

    /// What signs orders' access tokens, for the emails' links (`MailHost`).
    pub fn order_tokens(&self) -> &OrderTokens {
        &self.order_tokens
    }

    /// The token that lets a buyer read the order they placed (`OrderTokens::token`).
    pub fn order_token(&self, order: OrderId) -> String {
        self.order_tokens.token(order)
    }

    /// Whether `token` is `order`'s access token (`OrderTokens::verify`).
    pub fn is_order_token(&self, order: OrderId, token: &str) -> bool {
        self.order_tokens.verify(order, token)
    }
}

impl HasAuthSettings for HostEnv {
    fn auth_settings(&self) -> &AuthSettings {
        &self.auth
    }
}

impl IdGenerator for HostEnv {
    fn new_uuid(&self) -> Uuid {
        self.runtime.ids.new_uuid()
    }
}

impl Clock for HostEnv {
    fn now(&self) -> Timestamp {
        self.runtime.clock.now()
    }
}

impl Random for HostEnv {
    fn fill(&self, bytes: &mut [u8]) {
        self.runtime.random.fill(bytes);
    }
}

/// Argon2id (OWASP's parameters unless a test chose others), where the host runs
/// CPU-heavy work (`Offload`): a hash takes tens of milliseconds of CPU, which on a
/// shared runtime would stall the requests waiting behind it.
impl PasswordHasher for HostEnv {
    async fn hash_password(
        &self,
        password: &Password,
        salt: [u8; 16],
    ) -> Result<PasswordHash, HashError> {
        let (password, hasher) = (password.clone(), self.hasher);
        self.runtime
            .offload
            .run(move || hasher.hash_now(&password, salt))
            .await
            .map_err(|e| HashError(e.to_string()))?
    }

    async fn verify_password(
        &self,
        password: &Password,
        hash: &PasswordHash,
    ) -> Result<bool, HashError> {
        let (password, hash, hasher) = (password.clone(), hash.clone(), self.hasher);
        self.runtime
            .offload
            .run(move || hasher.verify_now(&password, &hash))
            .await
            .map_err(|e| HashError(e.to_string()))?
    }
}

impl SecretBox for HostEnv {
    fn seal(&self, plaintext: &[u8], context: &[u8], nonce: [u8; 24]) -> Sealed {
        self.secret_box.seal(plaintext, context, nonce)
    }

    fn open(&self, sealed: &Sealed, context: &[u8]) -> Result<Vec<u8>, SealError> {
        self.secret_box.open(sealed, context)
    }
}

/// One deployment's state: its storage, the host's capabilities (`host.rs`) and the
/// settings, shared by every request.
pub struct AppState<S> {
    pub storage: S,
    pub ids: HostIds,
    pub clock: HostClock,
    pub env: HostEnv,
    /// Work after the response (a password reset).
    pub background: HostBackground,
    /// Limits on the public routes (design note 0009).
    pub limiter: HostLimiter,
    /// Publishable keys authenticated in the last half minute (`key_cache.rs`).
    pub publishable_keys: crate::key_cache::KeyCache,
    /// Told when a write gives the dispatcher work (`hypha_core::SignalsDispatch`):
    /// wakes the jobs.
    pub after_writes: AfterWrites,
    /// The one store this deployment serves (design note 0006, model A), set at startup.
    pub store: StoreId,
    /// Its name at startup, for labels that shouldn't cost a query (the entry in an
    /// authenticator app). A rename shows there after a restart.
    pub store_name: String,
    /// File storage and asset URLs (design note 0013); `None` without
    /// `media.s3.endpoint`, and the asset routes then answer 503. Shared with `mail`,
    /// for the logo's URL.
    pub media: Option<Arc<crate::media::Media>>,
    /// Sending email (design note 0028): the sender, the from address and the links,
    /// the same one the host's `Mailer` handler holds.
    pub mail: Arc<crate::mail::MailHost>,
    /// Calls to merchant-configured URLs (rate apps, webhooks; design notes 0015, 0016),
    /// signed by `outbound::prepare`; each call sets its own timeout.
    pub calls: HostCalls,
    /// The settings in effect (design note 0017), the one copy: the body limit, the
    /// trusted proxies, the outbound policy, the jobs' timers and the dispatcher's
    /// batch, lease, retries and retention are read from here.
    pub config: hypha_config::Config,
}

/// What a host gives `AppState::new` besides its storage, its store and its settings
/// (`host.rs`; design note 0026).
pub struct HostParts {
    pub env: HostEnv,
    pub background: HostBackground,
    pub limiter: HostLimiter,
    pub after_writes: AfterWrites,
    pub calls: HostCalls,
    /// Shared with `mail` (`MailHost::new`).
    pub media: Option<Arc<crate::media::Media>>,
    /// What the host's `Mailer` holds too (it builds its handlers with it, before the
    /// state).
    pub mail: Arc<crate::mail::MailHost>,
    /// Publishable keys authenticated recently (`key_cache.rs`): one for the process
    /// natively, one per isolate on Workers.
    pub publishable_keys: crate::key_cache::KeyCache,
}

impl<S> AppState<S> {
    /// The state for the store `store` named `store_name`. Its clock and IDs are the
    /// `HostEnv`'s runtime's, so the state and the auth commands can't read different
    /// ones.
    pub fn new(
        storage: S,
        store: StoreId,
        store_name: String,
        host: HostParts,
        config: hypha_config::Config,
    ) -> Self {
        let runtime = host.env.runtime().clone();
        Self {
            storage,
            ids: runtime.ids,
            clock: runtime.clock,
            env: host.env,
            background: host.background,
            limiter: host.limiter,
            publishable_keys: host.publishable_keys,
            after_writes: host.after_writes,
            store,
            store_name,
            media: host.media,
            mail: host.mail,
            calls: host.calls,
            config,
        }
    }

    /// Whether calls to apps may reach private addresses (`outbound.allow_private`).
    pub fn outbound(&self) -> crate::outbound::OutboundPolicy {
        crate::outbound::OutboundPolicy {
            allow_private: self.config.allow_private_callbacks,
        }
    }
}

impl<S: Storage> AppState<S> {
    /// Ports scoped to the store this request is for, without credentials: only for
    /// the routes that need none (health, login, password reset, invite acceptance) and
    /// for startup. Every other handler gets its ports from `Caller::authenticate`.
    ///
    /// With one store per deployment the store is always `self.store`; a multi-store
    /// host would resolve it from the request (API key, hostname) here, and nothing else
    /// would change.
    ///
    /// The ports tell `after_writes` when a write gives the dispatcher work (events
    /// committed, a delivery retried), so writes without events don't wake the jobs.
    pub async fn public_ports(&self) -> Result<S::Ports<'_>, ApiError> {
        let ports = self.storage.ports(self.store).await?;
        hypha_core::SignalsDispatch::on_dispatch_work(&ports, self.after_writes.hook());
        Ok(ports)
    }
}

impl<S> AppState<S> {
    /// The shipping rate providers for a request's ports (design note 0015), whose
    /// calls to rate apps are recorded in the call log (design note 0029).
    pub fn rates<'a, P>(&'a self, ports: &'a P) -> crate::shipping::HostRates<'a, P>
    where
        P: hypha_shipping::HasAppProviders
            + hypha_apps::HasApps
            + hypha_extensions::HasExtensionCalls,
    {
        crate::shipping::HostRates {
            ports,
            calls: &self.calls,
            outbound: self.outbound(),
            env: &self.env,
            clock: &self.clock,
            ids: &self.ids,
            store: self.store,
            cart: None,
        }
    }

    /// `rates` for a cart's shipping: each call is recorded with the cart.
    pub fn cart_rates<'a, P>(
        &'a self,
        ports: &'a P,
        cart: hypha_core::CartId,
    ) -> crate::shipping::HostRates<'a, P>
    where
        P: hypha_shipping::HasAppProviders
            + hypha_apps::HasApps
            + hypha_extensions::HasExtensionCalls,
    {
        crate::shipping::HostRates {
            cart: Some(cart),
            ..self.rates(ports)
        }
    }

    /// The caller of apps' extension points (design note 0029) for this store.
    pub fn extensions(&self) -> crate::extensions::HostExtensions<'_> {
        crate::extensions::HostExtensions {
            calls: &self.calls,
            outbound: self.outbound(),
            clock: &self.clock,
            store: self.store,
        }
    }
}

impl<S> AppState<S> {
    /// The staff flows' environment for a request's `ports` (`crate::mail::StaffEnv`):
    /// the auth ports, and emails sent with the store's templates and recorded there.
    pub fn staff_env<'a, P>(&'a self, ports: &'a P) -> crate::mail::StaffEnv<'a, P> {
        crate::mail::StaffEnv::new(&self.env, &self.mail, ports)
    }
}

impl<S> AppState<S> {
    /// Fetches apps' manifests (design note 0029) through the host's outbound client.
    pub fn manifests(&self) -> crate::manifests::HostManifests<'_> {
        crate::manifests::HostManifests {
            calls: &self.calls,
            outbound: self.outbound(),
        }
    }

    /// The webhook sender the dispatcher delivers to apps with (design note 0016).
    pub fn webhooks(&self) -> crate::webhooks::HostWebhooks<'_> {
        crate::webhooks::HostWebhooks {
            calls: &self.calls,
            outbound: self.outbound(),
            env: &self.env,
            clock: &self.clock,
        }
    }
}

pub type SharedState<S> = Arc<AppState<S>>;

/// Where the repositories come from. Handlers ask for `ports()` once per request and
/// pass them to commands; `CheckoutPorts` (which includes `CartPorts`), `StaffPorts`
/// and `HasApiKeys` give access to every repository and the batch.
///
/// Handlers are generic over this, but axum can only prove their futures are `Send` for
/// a concrete storage type (the repo traits deliberately have no `Send` bound), so the
/// router is built per concrete type (see `routes::router!`).
#[allow(async_fn_in_trait)]
pub trait Storage: Send + Sync + 'static {
    type Ports<'a>: CheckoutPorts
        + hypha_order_actions::OrderActionPorts
        + StaffPorts
        + hypha_users::HasStaffSessions
        + HasApiKeys
        + hypha_media::HasAssets
        + hypha_media::HasAssetUsages
        + hypha_merchandising::MerchandisingPorts
        + hypha_shipping::HasShippingZones
        + hypha_shipping::HasAppProviders
        + hypha_core::HasOutbox
        + hypha_core::ActingAs
        + hypha_core::SignalsDispatch
        + hypha_apps::HasApps
        + hypha_extensions::HasExtensionCalls
        + hypha_oauth::HasOAuthRequests
        + hypha_oauth::HasOAuthCodes
        + hypha_oauth::HasOAuthTokens
        + hypha_search_index::HasSearchQueue
        + hypha_search_index::HasSearchSettings
        + hypha_channels::HasChannels
        + hypha_categories::HasCategories
        + hypha_auth::ChannelDirectory
        + hypha_mail::MailPorts
        + HasBatch<
            Batch: hypha_core::StageEvents
                       + hypha_markets::StageCatalogProducts
                       + hypha_markets::StageFixedPrices
                       + hypha_tax::StageTaxAssignments
                       + hypha_channels::StagePublications
                       + hypha_categories::StageCategories
                       + hypha_notifications::StageNotificationSettings
                       + hypha_apps::StageApps
                       + hypha_oauth::StageOAuth,
        >
    where
        Self: 'a;
    type Directory<'a>: StoreRepo
    where
        Self: 'a;
    /// The search engine a request's ports search with (design note 0019).
    type Search<'p>: hypha_search_index::SearchIndex
    where
        Self: 'p;
    /// The in-process event handlers this host runs (design note 0019, section 5.1): a
    /// tuple the host lists in its `handlers.rs`, its composition root. The dispatcher
    /// runs whatever is here (`jobs::dispatch`).
    type Handlers: for<'a> hypha_core::EventHandlers<Self::Ports<'a>> + Send + Sync + 'static;
    /// The payment providers this host was built with (design note 0027, PA5): a tuple
    /// the host lists in its `payments.rs`, next to `handlers.rs`. Checkout and the
    /// method commands find one by key.
    type Payments: hypha_payments::PaymentProviders + Send + Sync + 'static;

    /// Whether a dispatch step keeps one set of ports for all its work (`jobs::dispatch`)
    /// rather than getting them for each delivery and letting them go before a webhook
    /// is sent (design note 0016, EV9). `false`, the default, for a pool: a connection
    /// held while another server answers is one the requests can't have. A host whose
    /// every `ports()` opens a new connection that nothing else shares (Workers through
    /// Hyperdrive, design note 0026, W4) says `true`: there, holding one idle costs
    /// nothing, and opening one per delivery costs a connection's setup each time.
    const KEEPS_PORTS_ACROSS_DELIVERIES: bool = false;

    /// Repositories that only see `store`'s data (design note 0006). With one store per
    /// deployment (model A) the database *is* the store, so implementations ignore it;
    /// shared tables (B) would filter by it, a database per store (C) would pick by it.
    async fn ports(&self, store: StoreId) -> Result<Self::Ports<'_>, ApiError>;

    /// The stores this deployment knows (in model C, a control plane).
    async fn directory(&self) -> Result<Self::Directory<'_>, ApiError>;

    /// The directory through a request's own `ports`, when the store's database holds
    /// it too (one store per deployment, model A: both hosts today), which saves a
    /// second connection. `None`, the default, when it lives elsewhere (model C's
    /// control plane): callers then take `directory()`.
    fn directory_in<'p>(&'p self, ports: &'p Self::Ports<'_>) -> Option<&'p Self::Directory<'p>> {
        let _ = ports;
        None
    }

    /// The search engine, borrowing `ports` when the engine is the database (the
    /// built-in Postgres one). An external engine (Meilisearch, design note 0019 step 7)
    /// will be held by the storage and picked by configuration here.
    fn search<'p>(&'p self, ports: &'p Self::Ports<'_>) -> Self::Search<'p>;

    /// The handlers (`Self::Handlers`), which the host built at startup (some hold its
    /// environment: the `Mailer` its `MailHost`) and keeps in its storage.
    fn handlers(&self) -> &Self::Handlers;

    /// The payment providers (`Self::Payments`); usually `&PAYMENT_PROVIDERS`, a
    /// `const`.
    fn payment_providers(&self) -> &Self::Payments;

    /// Runs a password reset request as background work (`AppState::background`), so the
    /// response doesn't wait for it (design note 0008, section 5); dropped and logged
    /// when the host takes no more. Here rather than in the handler because background
    /// work must be a `Send` future, which the compiler can only prove for a concrete
    /// storage (the same reason as `routes::router!`).
    fn spawn_password_reset(state: SharedState<Self>, email: String)
    where
        Self: Sized;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_key_is_64_hex_characters() {
        assert_eq!(parse_secret_key(&"0f".repeat(32)), Ok([0x0f; 32]));
        assert!(parse_secret_key(&"0f".repeat(31)).is_err());
        assert!(parse_secret_key(&format!("{}zz", "0f".repeat(31))).is_err());
    }

    /// A runtime that's enough for what `HostEnv` does without storage.
    fn runtime() -> Runtime {
        struct Epoch;
        impl Clock for Epoch {
            fn now(&self) -> Timestamp {
                Timestamp::UNIX_EPOCH
            }
        }
        struct Nil;
        impl IdGenerator for Nil {
            fn new_uuid(&self) -> Uuid {
                Uuid::nil()
            }
        }
        Runtime {
            clock: HostClock::new(Epoch),
            ids: HostIds::new(Nil),
            random: HostRandom::new(|bytes| bytes.fill(7)),
            offload: Offload::inline(),
        }
    }

    fn order(n: u128) -> OrderId {
        OrderId::from_uuid(uuid::Uuid::from_u128(n))
    }

    #[test]
    fn an_order_token_opens_its_own_order_only() {
        let env = HostEnv::new([7; 32], AuthSettings::default(), runtime());
        let token = env.order_token(order(1));
        assert!(token.starts_with("hypha_ord_") && token.len() == "hypha_ord_".len() + 64);
        assert_eq!(env.order_token(order(1)), token, "the same every time");
        assert!(env.is_order_token(order(1), &token));
        assert!(!env.is_order_token(order(2), &token), "another order's");
        let other_key = HostEnv::new([8; 32], AuthSettings::default(), runtime());
        assert!(
            !other_key.is_order_token(order(1), &token),
            "under another secret key"
        );
        for bad in [
            "",
            "hypha_ord_",
            &token[..token.len() - 2],
            &token.replace("hypha_ord_", "x"),
        ] {
            assert!(!env.is_order_token(order(1), bad), "{bad:?}");
        }
        let flipped = format!("{}0", &token[..token.len() - 1]);
        if flipped != token {
            assert!(!env.is_order_token(order(1), &flipped));
        }
    }
}
