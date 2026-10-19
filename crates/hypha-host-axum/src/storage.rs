//! Postgres for the native host (design note 0001): a deadpool pool of `tokio-postgres`
//! connections, each keeping its prepared statements (design note 0025, §6), migrated
//! on startup. One `PgStore` per request.

use std::collections::BTreeMap;
use std::sync::Arc;

use hypha_core::{StoreId, Union};
use hypha_http::error::ApiError;
use hypha_http::host::HostClock;
use hypha_http::mail::MailHost;
use hypha_http::state::{SharedState, Storage};
use hypha_spore_core::{SporeHost, SporeSet, SporeStartError};
use hypha_store_postgres::{
    MigrationSet, PgConnection, PgStore, StatementCache, database_config, migrate,
};

use crate::Deployment;
use crate::handlers::CoreHandlers;
use crate::payments::CoreProviders;

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

    /// Straight to Postgres: a read's ROLLBACK can be queued (no Hyperdrive here).
    fn ends_reads_unawaited(&self) -> bool {
        true
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

/// The connection pool, migrated: what `PgStorage` serves from, and what startup reads
/// the store from before the handlers (which need it) exist.
pub struct PgDatabase {
    pool: deadpool::managed::Pool<PgManager>,
    /// What the stores stamp writes with (design note 0016, Q3).
    clock: HostClock,
}

impl PgDatabase {
    /// Opens a connection pool (`[database]`) and applies pending `migrations` (core's,
    /// then each spore's: `crate::migrations`). `clock` stamps what the stores write
    /// (design note 0016, Q3).
    pub async fn connect(
        url: &str,
        settings: &hypha_config::DatabaseConfig,
        clock: HostClock,
        migrations: &[MigrationSet],
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
        migrate(&mut conn.client, migrations)
            .await
            .map_err(|e| format!("migrations failed: {e}"))?;
        Ok(Self { pool, clock })
    }
}

impl PgDatabase {
    /// A store on a pooled connection.
    pub async fn store(&self) -> Result<PgStore<PooledClient>, ApiError> {
        let conn = self.pool.get().await.map_err(ApiError::internal)?;
        Ok(PgStore::new(PooledClient(conn), self.clock.shared()))
    }
}

/// A request's ports: a store on a pooled connection.
pub type PgPorts = PgStore<PooledClient>;

/// The handlers of a host built with `D`'s spores: its own, then theirs.
pub type HandlersOf<D> = Union<(
    CoreHandlers,
    <<D as Deployment>::Spores as SporeSet>::Handlers,
)>;

/// The payment providers of a host built with `D`'s spores: its own, then theirs.
pub type ProvidersOf<D> = Union<(
    CoreProviders,
    <<D as Deployment>::Spores as SporeSet>::PaymentProviders,
)>;

/// The storage of a host built with `D`'s spores (`host!`): the pool, and the handlers
/// and payment providers its spores join.
pub struct PgStorage<D: Deployment> {
    db: PgDatabase,
    /// Built at startup: the `Mailer` holds the host's mail environment.
    handlers: HandlersOf<D>,
    payments: ProvidersOf<D>,
    /// The spores themselves, built at startup.
    spores: hypha_spore_core::BuiltSpores<D::Spores>,
}

impl<D: Deployment> PgStorage<D> {
    /// Builds the spores (`SporeSet::build`, each with its environment from `spores` and
    /// its section of `config`, `[spores.<name>]`) and joins what they contribute to the
    /// host's own handlers and providers. A spore that fails to build is the error.
    pub fn new(
        db: PgDatabase,
        mail: Arc<MailHost>,
        spores: &SporeHost,
        config: &BTreeMap<String, toml::Table>,
    ) -> Result<Self, SporeStartError> {
        let built = <D::Spores as SporeSet>::build(spores, config)?;
        Ok(Self {
            db,
            handlers: Union((crate::handlers::core_handlers(mail), built.handlers)),
            payments: Union((crate::payments::CORE_PROVIDERS, built.payment_providers)),
            spores: built.spores,
        })
    }

    async fn store(&self) -> Result<PgPorts, ApiError> {
        self.db.store().await
    }
}

impl<D: Deployment> Storage for PgStorage<D> {
    type Ports<'a> = PgPorts;
    type Directory<'a> = PgPorts;
    type Search<'p> = &'p PgPorts;
    type Handlers = HandlersOf<D>;
    type Payments = ProvidersOf<D>;
    type Spores = hypha_spore_core::BuiltSpores<D::Spores>;

    async fn ports(&self, _store: StoreId) -> Result<PgPorts, ApiError> {
        self.store().await
    }

    async fn directory(&self) -> Result<PgPorts, ApiError> {
        self.store().await
    }

    /// The stores table is in the store's database (model A).
    fn directory_in<'p>(&'p self, ports: &'p PgPorts) -> Option<&'p PgPorts> {
        Some(ports)
    }

    fn search<'p>(&'p self, ports: &'p PgPorts) -> &'p PgPorts {
        ports
    }

    fn handlers(&self) -> &HandlersOf<D> {
        &self.handlers
    }

    fn payment_providers(&self) -> &ProvidersOf<D> {
        &self.payments
    }

    fn spores(&self) -> &hypha_spore_core::BuiltSpores<D::Spores> {
        &self.spores
    }

    fn spawn_password_reset(state: SharedState<Self>, email: String) {
        let background = Arc::clone(&state);
        let work = hypha_http::routes::password_reset_in_background(state, email);
        if !background.background.spawn(work) {
            tracing::warn!("password reset request dropped: every background slot is busy");
        }
    }
}
