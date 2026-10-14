use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use hypha_checkout::CheckoutPorts;
use hypha_core::{Clock, IdGenerator, StoreId, Timestamp};
use hypha_store_postgres::{CORE, PgConnection, PgStore, database_config, migrate};
use hypha_stores::StoreRepo;
use hypha_testkit::{InMemoryPorts, InMemoryStoreRepo};
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

pub struct AppState<S> {
    pub storage: S,
    pub ids: UuidV7Ids,
    pub clock: SystemClock,
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
/// pass them to commands; `CheckoutPorts` (which includes `CartPorts`) gives access to
/// every repository and the batch.
///
/// Handlers are generic over this, but axum can only prove their futures are `Send` for
/// a concrete storage type (the repo traits deliberately have no `Send` bound), so the
/// router is built per concrete type (see `routes::router!`).
#[allow(async_fn_in_trait)]
pub trait Storage: Send + Sync + 'static {
    type Ports<'a>: CheckoutPorts
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
}
