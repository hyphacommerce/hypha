use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use hypha_auth::{
    Argon2id, HasApiKeys, HashError, Password, PasswordHash, PasswordHasher, SealError, Sealed,
    SecretBox, XChaChaBox,
};
use hypha_checkout::CheckoutPorts;
use hypha_core::{Clock, IdGenerator, Notifier, NotifyError, Random, StoreId, Timestamp};
use hypha_store_postgres::{CORE, PgConnection, PgStore, database_config, migrate};
use hypha_stores::StoreRepo;
use hypha_testkit::{InMemoryPorts, InMemoryStoreRepo};
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

/// The non-storage ports auth commands use (`hypha_auth::AuthEnv`) and the staff
/// emails' `Notifier`, for real.
pub struct HostEnv {
    secret_box: XChaChaBox,
    hasher: Argon2id,
    /// Where the admin panel lives, for links in emails. Unset: the email shows the
    /// bare token.
    admin_url: Option<String>,
}

impl HostEnv {
    /// `secret_key` seals TOTP secrets (`HYPHA_SECRET_KEY`); losing it makes every
    /// authenticator unreadable, so it must stay the same across restarts.
    pub fn new(secret_key: [u8; 32], admin_url: Option<String>) -> Self {
        Self {
            secret_box: XChaChaBox::new(secret_key),
            hasher: Argon2id::OWASP,
            admin_url: admin_url.map(|url| url.trim_end_matches('/').to_string()),
        }
    }

    /// Other Argon2 parameters (tests use cheap ones).
    #[cfg(test)]
    pub fn with_hasher(mut self, hasher: Argon2id) -> Self {
        self.hasher = hasher;
        self
    }

    fn link(&self, path: &str, token: &str) -> String {
        match &self.admin_url {
            Some(base) => format!("{base}{path}?token={token}"),
            None => format!("token {token} (set HYPHA_ADMIN_URL for a link)"),
        }
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

pub struct AppState<S> {
    pub storage: S,
    pub ids: UuidV7Ids,
    pub clock: SystemClock,
    pub env: HostEnv,
    /// The one store this deployment serves (design note 0006, model A), set at startup.
    pub store: StoreId,
}

impl<S: Storage> AppState<S> {
    /// Ports scoped to the store this request is for. With one store per deployment
    /// that's always `self.store`; a multi-store host would resolve it from the request
    /// (API key, hostname) here, and nothing else would change.
    pub async fn ports(&self) -> Result<S::Ports<'_>, ApiError> {
        self.storage.ports(self.store).await
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
    type Ports<'a>: CheckoutPorts + StaffPorts + HasApiKeys
    where
        Self: 'a;
    type Directory<'a>: StoreRepo
    where
        Self: 'a;

    /// Repositories that only see `store`'s data (design note 0006). With one store per
    /// deployment (model A) the database *is* the store, so implementations ignore it;
    /// shared tables (B) would filter by it, a database per store (C) would pick by it.
    async fn ports(&self, store: StoreId) -> Result<Self::Ports<'_>, ApiError>;

    /// The stores this deployment knows (in model C, a control plane).
    async fn directory(&self) -> Result<Self::Directory<'_>, ApiError>;

    /// Runs a password reset request on a task of its own, so the response doesn't wait
    /// for it (design note 0008, section 5). Here rather than in the handler because
    /// `tokio::spawn` needs a `Send` future, which the compiler can only prove for a
    /// concrete storage (the same reason as `routes::router!`).
    fn spawn_password_reset(state: SharedState<Self>, email: String)
    where
        Self: Sized;
}

// ---------- in memory (default; data is lost on restart) ----------

/// The store directory and one store's repositories (`InMemoryPorts`, shared with the
/// tests).
#[derive(Default)]
pub struct MemoryStorage {
    stores: InMemoryStoreRepo,
    ports: InMemoryPorts,
}

impl Storage for MemoryStorage {
    type Ports<'a> = &'a InMemoryPorts;
    type Directory<'a> = &'a InMemoryStoreRepo;

    async fn ports(&self, _store: StoreId) -> Result<&InMemoryPorts, ApiError> {
        Ok(&self.ports)
    }

    async fn directory(&self) -> Result<&InMemoryStoreRepo, ApiError> {
        Ok(&self.stores)
    }

    fn spawn_password_reset(state: SharedState<Self>, email: String) {
        tokio::spawn(crate::routes::password_reset_in_background(state, email));
    }
}

// ---------- Postgres (when DATABASE_URL is set) ----------

/// A connection checked out of the pool; it goes back when the request's store is dropped.
pub struct PooledClient(deadpool_postgres::Object);

impl PgConnection for PooledClient {
    fn client(&mut self) -> &mut tokio_postgres::Client {
        // Object -> deadpool's ClientWrapper -> tokio_postgres::Client
        &mut self.0
    }
}

/// How long a request waits for a pooled connection (or for a new one to open).
const POOL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct PgStorage {
    pool: deadpool_postgres::Pool,
}

impl PgStorage {
    /// Opens a connection pool and applies pending migrations.
    pub async fn connect(url: &str) -> Result<Self, String> {
        use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime};

        let config = database_config(url).map_err(|e| format!("invalid DATABASE_URL: {e}"))?;
        let manager = Manager::from_config(
            config,
            tokio_postgres::NoTls,
            ManagerConfig {
                recycling_method: RecyclingMethod::Fast,
            },
        );
        // A request waits at most this long for a free connection or a new one, then
        // fails with a 500 instead of hanging while the pool is exhausted or Postgres
        // is down.
        let pool = Pool::builder(manager)
            .runtime(Runtime::Tokio1)
            .wait_timeout(Some(POOL_TIMEOUT))
            .create_timeout(Some(POOL_TIMEOUT))
            .recycle_timeout(Some(POOL_TIMEOUT))
            .build()
            .map_err(|e| format!("cannot build the connection pool: {e}"))?;
        let mut conn = pool
            .get()
            .await
            .map_err(|e| format!("cannot connect to Postgres: {e}"))?;
        migrate(&mut conn, &[CORE])
            .await
            .map_err(|e| format!("migrations failed: {e}"))?;
        Ok(Self { pool })
    }
}

impl PgStorage {
    async fn store(&self) -> Result<PgStore<PooledClient>, ApiError> {
        let conn = self.pool.get().await.map_err(ApiError::internal)?;
        Ok(PgStore::new(PooledClient(conn)))
    }
}

impl Storage for PgStorage {
    type Ports<'a> = PgStore<PooledClient>;
    type Directory<'a> = PgStore<PooledClient>;

    async fn ports(&self, _store: StoreId) -> Result<PgStore<PooledClient>, ApiError> {
        self.store().await
    }

    async fn directory(&self) -> Result<PgStore<PooledClient>, ApiError> {
        self.store().await
    }

    fn spawn_password_reset(state: SharedState<Self>, email: String) {
        tokio::spawn(crate::routes::password_reset_in_background(state, email));
    }
}
