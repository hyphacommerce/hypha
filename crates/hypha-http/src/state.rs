use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use hypha_auth::{
    Argon2id, AuthSettings, HasApiKeys, HasAuthSettings, HashError, Password, PasswordHash,
    PasswordHasher, SealError, Sealed, SecretBox, XChaChaBox,
};
use hypha_checkout::CheckoutPorts;
use hypha_core::HasBatch;
use hypha_core::{Clock, IdGenerator, Notifier, NotifyError, OrderId, Random, StoreId, Timestamp};
use hypha_store_postgres::{CORE, PgConnection, PgStore, StatementCache, database_config, migrate};
use hypha_stores::StoreRepo;
use hypha_users::{StaffEmail, StaffPorts};
use uuid::Uuid;

use crate::error::ApiError;

/// Real IDs: UUIDv7 (time-ordered), generated from the system clock and RNG.
#[derive(Default)]
pub struct UuidV7Ids;

impl IdGenerator for UuidV7Ids {
    fn new_uuid(&self) -> Uuid {
        Uuid::now_v7()
    }
}

/// The real clock. Only hosts read it; commands get it as a `Clock`.
#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        // A clock set before 1970 reads as 1970; i64 microseconds last until year 294247.
        let micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_micros()).unwrap_or(i64::MAX)
            });
        Timestamp::from_unix_micros(micros)
    }
}

/// The prefix of an order's access token (`HostEnv::order_token`).
const ORDER_TOKEN_PREFIX: &str = "hypha_ord_";

/// The non-storage ports auth commands use (`hypha_auth::AuthEnv`) and the staff
/// emails' `Notifier`, for real.
pub struct HostEnv {
    secret_box: XChaChaBox,
    /// Signs orders' access tokens (`order_token`): derived from `HYPHA_SECRET_KEY`, so
    /// the same key gives the same tokens across restarts.
    order_key: [u8; 32],
    hasher: Argon2id,
    /// Where the admin panel lives, for links in emails. Unset: the email shows the
    /// bare token.
    admin_url: Option<String>,
    /// Session, second-factor, reset and invite lifetimes (`[auth]`, design note 0017).
    auth: AuthSettings,
}

impl HostEnv {
    /// `secret_key` seals TOTP secrets (`HYPHA_SECRET_KEY`); losing it makes every
    /// authenticator unreadable, so it must stay the same across restarts.
    /// `admin_url` is without a trailing `/`, as `hypha-config` gives it.
    pub fn new(secret_key: [u8; 32], admin_url: Option<String>, auth: AuthSettings) -> Self {
        use sha2::Digest;
        let order_key = sha2::Sha256::new()
            .chain_update(b"hypha order access token\0")
            .chain_update(secret_key)
            .finalize()
            .into();
        Self {
            secret_box: XChaChaBox::new(secret_key),
            order_key,
            hasher: Argon2id::OWASP,
            admin_url,
            auth,
        }
    }

    /// Other Argon2 parameters (tests use cheap ones).
    #[cfg(test)]
    pub fn with_hasher(mut self, hasher: Argon2id) -> Self {
        self.hasher = hasher;
        self
    }

    /// The token that lets a buyer read the order they placed (design note 0024, §7):
    /// `hypha_ord_` and a hex HMAC-SHA256 of the order's ID. Derived rather than stored,
    /// so a replayed checkout answers with the same token, and nothing needs migrating;
    /// a new `HYPHA_SECRET_KEY` makes old tokens stop working.
    pub fn order_token(&self, order: OrderId) -> String {
        use hmac::Mac;
        let hex: String = self
            .order_mac(order)
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("{ORDER_TOKEN_PREFIX}{hex}")
    }

    /// Whether `token` is `order`'s access token, compared in constant time.
    pub fn is_order_token(&self, order: OrderId, token: &str) -> bool {
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
            Some(bytes) if hex.len() == 64 => self.order_mac(order).verify_slice(&bytes).is_ok(),
            _ => false,
        }
    }

    fn order_mac(&self, order: OrderId) -> hmac::Hmac<sha2::Sha256> {
        use hmac::{KeyInit, Mac};
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&self.order_key)
            .unwrap_or_else(|_| unreachable!("HMAC accepts every key length"));
        mac.update(order.to_string().as_bytes());
        mac
    }

    fn link(&self, path: &str, token: &str) -> String {
        match &self.admin_url {
            Some(base) => format!("{base}{path}?token={token}"),
            None => format!("token {token} (set HYPHA_ADMIN_URL for a link)"),
        }
    }
}

impl HasAuthSettings for HostEnv {
    fn auth_settings(&self) -> &AuthSettings {
        &self.auth
    }
}

impl IdGenerator for HostEnv {
    fn new_uuid(&self) -> Uuid {
        UuidV7Ids.new_uuid()
    }
}

impl Clock for HostEnv {
    fn now(&self) -> Timestamp {
        SystemClock.now()
    }
}

impl Random for HostEnv {
    fn fill(&self, bytes: &mut [u8]) {
        // Without the OS generator no token or key can be made safely, and there is no
        // sensible way to continue: stop, like the standard library does when it can't
        // seed its hash maps.
        if let Err(e) = getrandom::fill(bytes) {
            panic!("the operating system's random number generator failed: {e}");
        }
    }
}

/// Argon2id (OWASP's parameters unless a test chose others), on Tokio's blocking
/// threads: a hash takes tens of milliseconds of CPU, which would otherwise stall every
/// request on that worker.
impl PasswordHasher for HostEnv {
    async fn hash_password(
        &self,
        password: &Password,
        salt: [u8; 16],
    ) -> Result<PasswordHash, HashError> {
        let (password, hasher) = (password.clone(), self.hasher);
        tokio::task::spawn_blocking(move || hasher.hash_now(&password, salt))
            .await
            .map_err(|e| HashError(e.to_string()))?
    }

    async fn verify_password(
        &self,
        password: &Password,
        hash: &PasswordHash,
    ) -> Result<bool, HashError> {
        let (password, hash, hasher) = (password.clone(), hash.clone(), self.hasher);
        tokio::task::spawn_blocking(move || hasher.verify_now(&password, &hash))
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

/// No email provider yet (design note 0008, A10): the rendered email goes to the log.
/// That puts invite and reset tokens in the log, which is fine for development only;
/// startup warns about it.
impl Notifier<StaffEmail> for HostEnv {
    async fn notify(&self, message: StaffEmail) -> Result<(), NotifyError> {
        let (to, subject, body) = match &message {
            StaffEmail::Invite { to, token, .. } => (
                to,
                "You're invited to run the store",
                self.link("/invite", token),
            ),
            StaffEmail::PasswordReset { to, token, .. } => (
                to,
                "Reset your password",
                self.link("/reset-password", token),
            ),
        };
        tracing::info!(%to, subject, body, "email (logged, not sent)");
        Ok(())
    }
}

impl Notifier<hypha_order_actions::BuyerEmail> for HostEnv {
    async fn notify(&self, message: hypha_order_actions::BuyerEmail) -> Result<(), NotifyError> {
        use hypha_order_actions::BuyerEmail as B;
        let tracked = |tracking: &[hypha_fulfillment::Tracking]| {
            tracking
                .iter()
                .map(|t| {
                    let link = t.url().map(|u| format!(" ({u})")).unwrap_or_default();
                    format!("{} {}{link}", t.carrier().unwrap_or("tracking"), t.number())
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let (to, subject, body) = match &message {
            B::Shipped {
                to,
                order,
                tracking,
            } => (
                to,
                format!("Order {order} is on its way"),
                tracked(tracking),
            ),
            B::TrackingUpdated {
                to,
                order,
                tracking,
            } => (
                to,
                format!("New tracking for order {order}"),
                tracked(tracking),
            ),
            B::Cancelled { to, order, reason } => (
                to,
                format!("Order {order} is cancelled"),
                format!("reason: {reason}"),
            ),
        };
        tracing::info!(%to, subject, body, "email (logged, not sent)");
        Ok(())
    }
}

/// Work that runs after its response went out (password reset requests), with a fixed
/// number of slots. When they are full, new work is dropped and logged: anonymous
/// requests can't pile up tasks that each wait for a pooled connection, and the client
/// gets no signal it could probe with. Shutdown waits for the work in flight (`drain`).
pub struct Background {
    slots: Arc<tokio::sync::Semaphore>,
}

impl Background {
    /// Few on purpose: each slot may hold a pooled connection.
    pub const SLOTS: u32 = 4;

    /// Spawns `work` if a slot is free; `false` (and nothing runs) if not.
    pub fn spawn(&self, work: impl Future<Output = ()> + Send + 'static) -> bool {
        let Ok(slot) = Arc::clone(&self.slots).try_acquire_owned() else {
            return false;
        };
        tokio::spawn(async move {
            work.await;
            drop(slot);
        });
        true
    }

    /// Waits up to `limit` for the work in flight to finish; `false` if it didn't. Takes
    /// every slot, so nothing new starts meanwhile.
    pub async fn drain(&self, limit: std::time::Duration) -> bool {
        tokio::time::timeout(limit, self.slots.acquire_many(Self::SLOTS))
            .await
            .is_ok_and(|permits| permits.is_ok())
    }
}

impl Default for Background {
    fn default() -> Self {
        Self {
            slots: Arc::new(tokio::sync::Semaphore::new(Self::SLOTS as usize)),
        }
    }
}

pub struct AppState<S> {
    pub storage: S,
    pub ids: UuidV7Ids,
    pub clock: SystemClock,
    pub env: HostEnv,
    pub background: Background,
    /// Limits on the public routes (design note 0009). In memory: one instance's counts.
    pub limiter: crate::rate_limit::MemoryRateLimiter,
    /// Publishable keys authenticated in the last half minute (`key_cache.rs`).
    pub publishable_keys: crate::key_cache::KeyCache,
    /// Wakes the jobs when there's work, so they needn't look often (`jobs.rs`).
    pub wake: crate::jobs::Wake,
    /// The one store this deployment serves (design note 0006, model A), set at startup.
    pub store: StoreId,
    /// Its name at startup, for labels that shouldn't cost a query (the entry in an
    /// authenticator app). A rename shows there after a restart.
    pub store_name: String,
    /// File storage and asset URLs (design note 0013); `None` without
    /// `media.s3.endpoint`, and the asset routes then answer 503.
    pub media: Option<crate::media::Media>,
    /// For rate callbacks to external apps (design note 0015); each call sets its own
    /// timeout, the provider's. Built by `outbound::client` under `outbound()`.
    pub http: reqwest::Client,
    /// The settings in effect (design note 0017), the one copy: the body limit, the
    /// trusted proxies, the outbound policy, the jobs' timers and the dispatcher's
    /// batch, lease, retries and retention are read from here.
    pub config: hypha_config::Config,
}

impl<S> AppState<S> {
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
    pub async fn public_ports(&self) -> Result<S::Ports<'_>, ApiError> {
        self.storage.ports(self.store).await
    }
}

impl<S> AppState<S> {
    /// The shipping rate providers for a request's ports (design note 0015).
    pub fn rates<'a, P: hypha_shipping::HasAppProviders + hypha_apps::HasApps>(
        &'a self,
        ports: &'a P,
    ) -> crate::shipping::HostRates<'a, P> {
        crate::shipping::HostRates {
            ports,
            http: &self.http,
            outbound: self.outbound(),
            env: &self.env,
            clock: &self.clock,
        }
    }
}

impl<S> AppState<S> {
    /// The webhook sender the dispatcher delivers to apps with (design note 0016).
    pub fn webhooks(&self) -> crate::webhooks::HostWebhooks<'_> {
        crate::webhooks::HostWebhooks {
            http: &self.http,
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
        + HasApiKeys
        + hypha_media::HasAssets
        + hypha_media::HasAssetUsages
        + hypha_merchandising::MerchandisingPorts
        + hypha_shipping::HasShippingZones
        + hypha_shipping::HasAppProviders
        + hypha_core::HasOutbox
        + hypha_core::ActingAs
        + hypha_apps::HasApps
        + hypha_search_index::HasSearchQueue
        + hypha_search_index::HasSearchSettings
        + hypha_channels::HasChannels
        + hypha_categories::HasCategories
        + hypha_auth::ChannelDirectory
        + HasBatch<
            Batch: hypha_core::StageEvents
                       + hypha_markets::StageCatalogProducts
                       + hypha_markets::StageFixedPrices
                       + hypha_tax::StageTaxAssignments
                       + hypha_channels::StagePublications
                       + hypha_categories::StageCategories,
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

    /// Repositories that only see `store`'s data (design note 0006). With one store per
    /// deployment (model A) the database *is* the store, so implementations ignore it;
    /// shared tables (B) would filter by it, a database per store (C) would pick by it.
    async fn ports(&self, store: StoreId) -> Result<Self::Ports<'_>, ApiError>;

    /// The stores this deployment knows (in model C, a control plane).
    async fn directory(&self) -> Result<Self::Directory<'_>, ApiError>;

    /// The search engine, borrowing `ports` when the engine is the database (the
    /// built-in Postgres one). An external engine (Meilisearch, design note 0019 step 7)
    /// will be held by the storage and picked by configuration here.
    fn search<'p>(&'p self, ports: &'p Self::Ports<'_>) -> Self::Search<'p>;

    /// Runs a password reset request in a `Background` slot, so the response doesn't
    /// wait for it (design note 0008, section 5); dropped and logged when every slot is
    /// busy. Here rather than in the handler because `tokio::spawn` needs a `Send`
    /// future, which the compiler can only prove for a concrete storage (the same reason
    /// as `routes::router!`).
    fn spawn_password_reset(state: SharedState<Self>, email: String)
    where
        Self: Sized;

    /// Starts the background jobs (`jobs.rs`: delivering events, pruning, the media
    /// sweep). Concrete per storage for the same `Send` reason as
    /// `spawn_password_reset`.
    fn start_jobs(state: SharedState<Self>) -> crate::jobs::Jobs
    where
        Self: Sized;
}

// ---------- Postgres ----------

/// A pooled connection: deadpool-postgres's client, with the statements prepared on it
/// (design note 0025, §6), kept as long as the connection.
pub struct PgConn {
    client: deadpool_postgres::ClientWrapper,
    /// `None` with `database.statement_cache = false`.
    statements: Option<StatementCache>,
}

/// deadpool-postgres's manager (connecting, TLS, recycling), keeping a statement cache
/// with each connection it makes.
pub struct PgManager {
    inner: deadpool_postgres::Manager,
    statement_cache: bool,
}

impl deadpool::managed::Manager for PgManager {
    type Type = PgConn;
    type Error = tokio_postgres::Error;

    async fn create(&self) -> Result<PgConn, tokio_postgres::Error> {
        Ok(PgConn {
            client: self.inner.create().await?,
            statements: self.statement_cache.then(StatementCache::default),
        })
    }

    async fn recycle(
        &self,
        conn: &mut PgConn,
        metrics: &deadpool::managed::Metrics,
    ) -> deadpool::managed::RecycleResult<tokio_postgres::Error> {
        self.inner.recycle(&mut conn.client, metrics).await
    }

    fn detach(&self, conn: &mut PgConn) {
        self.inner.detach(&mut conn.client);
    }
}

/// A connection checked out of the pool; it goes back when the request's store is dropped.
pub struct PooledClient(deadpool::managed::Object<PgManager>);

impl PgConnection for PooledClient {
    fn client(&mut self) -> &mut tokio_postgres::Client {
        // PgConn -> deadpool's ClientWrapper -> tokio_postgres::Client
        &mut self.0.client
    }

    fn parts(&mut self) -> (&mut tokio_postgres::Client, Option<&StatementCache>) {
        let conn = &mut *self.0;
        (&mut conn.client, conn.statements.as_ref())
    }
}

/// TLS for Postgres, verified against the operating system's trust store (as reqwest's
/// is). The URL's `sslmode` decides whether it's used: `prefer` (the default) tries TLS
/// and falls back to plain, which the compose database needs; `require` refuses to
/// connect without it, as managed databases (Neon, RDS, Supabase) expect. A private CA
/// (RDS's) goes into the system's trust store.
fn postgres_tls() -> Result<tokio_postgres_rustls::MakeRustlsConnect, String> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let provider = std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(|b| b.with_platform_verifier())
        .map_err(|e| format!("cannot set up TLS for Postgres: {e}"))?
        .with_no_client_auth();
    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}

pub struct PgStorage {
    pool: deadpool::managed::Pool<PgManager>,
    /// What the stores stamp writes with (design note 0016, Q3).
    clock: Arc<SystemClock>,
}

impl PgStorage {
    /// Opens a connection pool (`[database]`) and applies pending migrations.
    pub async fn connect(
        url: &str,
        settings: &hypha_config::DatabaseConfig,
    ) -> Result<Self, String> {
        use deadpool::{Runtime, managed::Pool};
        use deadpool_postgres::{Manager, ManagerConfig, RecyclingMethod};

        let config = database_config(url).map_err(|e| format!("invalid DATABASE_URL: {e}"))?;
        let manager = PgManager {
            inner: Manager::from_config(
                config,
                postgres_tls()?,
                ManagerConfig {
                    recycling_method: RecyclingMethod::Fast,
                },
            ),
            statement_cache: settings.statement_cache,
        };
        // A request waits at most `pool_timeout` for a free connection or a new one,
        // then fails with a 500 instead of hanging while the pool is exhausted or
        // Postgres is down.
        let timeout = Some(settings.pool_timeout);
        let mut builder = Pool::builder(manager)
            .runtime(Runtime::Tokio1)
            .wait_timeout(timeout)
            .create_timeout(timeout)
            .recycle_timeout(timeout);
        if let Some(max) = settings.max_connections {
            builder = builder.max_size(max);
        }
        let pool: Pool<PgManager> = builder
            .build()
            .map_err(|e| format!("cannot build the connection pool: {e}"))?;
        let mut conn = pool
            .get()
            .await
            .map_err(|e| format!("cannot connect to Postgres: {e}"))?;
        migrate(&mut conn.client, &[CORE])
            .await
            .map_err(|e| format!("migrations failed: {e}"))?;
        Ok(Self {
            pool,
            clock: Arc::new(SystemClock),
        })
    }
}

impl PgStorage {
    async fn store(&self) -> Result<PgStore<PooledClient>, ApiError> {
        let conn = self.pool.get().await.map_err(ApiError::internal)?;
        Ok(PgStore::new(PooledClient(conn), self.clock.clone()))
    }
}

impl Storage for PgStorage {
    type Ports<'a> = PgStore<PooledClient>;
    type Directory<'a> = PgStore<PooledClient>;
    type Search<'p> = &'p PgStore<PooledClient>;

    async fn ports(&self, _store: StoreId) -> Result<PgStore<PooledClient>, ApiError> {
        self.store().await
    }

    async fn directory(&self) -> Result<PgStore<PooledClient>, ApiError> {
        self.store().await
    }

    fn search<'p>(&'p self, ports: &'p PgStore<PooledClient>) -> &'p PgStore<PooledClient> {
        ports
    }

    fn spawn_password_reset(state: SharedState<Self>, email: String) {
        let background = Arc::clone(&state);
        let work = crate::routes::password_reset_in_background(state, email);
        if !background.background.spawn(work) {
            tracing::warn!("password reset request dropped: every background slot is busy");
        }
    }

    fn start_jobs(state: SharedState<Self>) -> crate::jobs::Jobs {
        crate::jobs::start(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(n: u128) -> OrderId {
        OrderId::from_uuid(uuid::Uuid::from_u128(n))
    }

    #[test]
    fn an_order_token_opens_its_own_order_only() {
        let env = HostEnv::new([7; 32], None, AuthSettings::default());
        let token = env.order_token(order(1));
        assert!(token.starts_with("hypha_ord_") && token.len() == "hypha_ord_".len() + 64);
        assert_eq!(env.order_token(order(1)), token, "the same every time");
        assert!(env.is_order_token(order(1), &token));
        assert!(!env.is_order_token(order(2), &token), "another order's");
        let other_key = HostEnv::new([8; 32], None, AuthSettings::default());
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

    #[tokio::test]
    async fn background_work_beyond_the_slots_is_dropped_and_drain_waits() {
        let background = Background::default();
        let (release, wait) = tokio::sync::watch::channel(false);
        for _ in 0..Background::SLOTS {
            let mut wait = wait.clone();
            assert!(background.spawn(async move {
                let _ = wait.wait_for(|go| *go).await;
            }));
        }
        assert!(!background.spawn(async {}));
        assert!(!background.drain(std::time::Duration::from_millis(20)).await);
        release.send(true).unwrap();
        assert!(background.drain(std::time::Duration::from_secs(5)).await);
    }
}
