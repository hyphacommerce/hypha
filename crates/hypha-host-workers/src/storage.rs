//! Postgres through Hyperdrive: a connection per `ports()` call, which Hyperdrive's pool
//! next to the database makes cheap; no statement cache (each connection is used once).
//! Migrations aren't run here (`setup` from the CLI).

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use hypha_core::{StoreId, Union};
use hypha_http::error::ApiError;
use hypha_http::host::HostClock;
use hypha_http::mail::MailHost;
use hypha_http::preview::OpenPreview;
use hypha_http::state::{SharedState, Storage};
use hypha_spore_core::{SporeHost, SporeSet, SporeStartError};
use hypha_store_postgres::PgStore;
use tokio_postgres::Client;
use worker::Env;
use worker::send::SendFuture;

use crate::Deployment;
use hypha_http::handlers::CoreHandlers;
use hypha_http::payments::CoreProviders;

/// The Hyperdrive binding's name in `wrangler.toml`.
pub const HYPERDRIVE: &str = "HYPERDRIVE";

/// A request's ports: a store on its own connection.
pub type WorkersPorts = PgStore<Client>;

/// The handlers of a Worker built with `D`'s spores: its own, then theirs.
pub type HandlersOf<D> = Union<(
    CoreHandlers,
    <<D as Deployment>::Spores as SporeSet>::Handlers,
)>;

/// The payment providers of a Worker built with `D`'s spores: its own, then theirs.
pub type ProvidersOf<D> = Union<(
    CoreProviders,
    <<D as Deployment>::Spores as SporeSet>::PaymentProviders,
)>;

/// The storage of a Worker built with `D`'s spores (`worker!`): the bindings, and the
/// handlers and payment providers its spores join.
pub struct WorkersStorage<D: Deployment> {
    env: Env,
    /// What the stores stamp writes with.
    clock: HostClock,
    /// Built at setup: the `Mailer` holds the host's mail environment.
    handlers: HandlersOf<D>,
    payments: ProvidersOf<D>,
    /// The spores themselves, built at setup.
    spores: hypha_spore_core::BuiltSpores<D::Spores>,
}

impl<D: Deployment> WorkersStorage<D> {
    /// Builds the spores (`SporeSet::build`, each with its environment from `spores` and
    /// its section of `config`) and joins what they contribute to the host's own
    /// handlers and providers. A spore that fails to build is the error.
    pub fn new(
        env: Env,
        clock: HostClock,
        mail: Arc<MailHost>,
        spores: &SporeHost,
        config: &BTreeMap<String, toml::Table>,
    ) -> Result<Self, SporeStartError> {
        let built = <D::Spores as SporeSet>::build(spores, config)?;
        Ok(Self {
            env,
            clock,
            handlers: Union((hypha_http::handlers::core_handlers(mail), built.handlers)),
            payments: Union((
                hypha_http::payments::CORE_PROVIDERS,
                built.payment_providers,
            )),
            spores: built.spores,
        })
    }

    async fn store(&self) -> Result<PgStore<Client>, ApiError> {
        store(&self.env, &self.clock).await
    }
}

/// A store on a new connection: what `WorkersStorage` serves from, and what setup reads
/// the store from before the handlers (which need it) exist.
pub async fn store(env: &Env, clock: &HostClock) -> Result<PgStore<Client>, ApiError> {
    let client = SendFuture::new(connect(env))
        .await
        .map_err(ApiError::internal)?;
    Ok(PgStore::new(client, clock.shared()))
}

/// A connection over the binding's socket, in plain text: Hyperdrive ends TLS to the
/// database itself, and the socket to it stays inside Cloudflare. The connection's task
/// runs on the isolate's event loop until the client is dropped.
async fn connect(env: &Env) -> Result<Client, String> {
    let hyperdrive = env
        .hyperdrive(HYPERDRIVE)
        .map_err(|e| format!("the {HYPERDRIVE} binding: {e}"))?;
    let mut config: tokio_postgres::Config = hyperdrive
        .connection_string()
        .parse()
        .map_err(|e| format!("Hyperdrive's connection string: {e}"))?;
    config.ssl_mode(tokio_postgres::config::SslMode::Disable);
    let socket = hyperdrive
        .connect()
        .map_err(|e| format!("cannot open Hyperdrive's socket: {e}"))?;
    let (client, connection) = config
        .connect_raw(socket, tokio_postgres::NoTls)
        .await
        .map_err(|e| format!("cannot connect to Postgres: {e}"))?;
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = connection.await {
            tracing::error!(error = %e, "postgres connection");
        }
    });
    Ok(client)
}

impl<D: Deployment> Storage for WorkersStorage<D> {
    type Ports<'a> = PgStore<Client>;
    type Directory<'a> = PgStore<Client>;
    type Search<'p> = &'p PgStore<Client>;
    type Handlers = HandlersOf<D>;
    type Payments = ProvidersOf<D>;
    type Spores = hypha_spore_core::BuiltSpores<D::Spores>;

    /// Each `ports()` is a new connection: a dispatch step opens one, not one per
    /// delivery.
    const KEEPS_PORTS_ACROSS_DELIVERIES: bool = true;

    async fn ports(&self, _store: StoreId) -> Result<PgStore<Client>, ApiError> {
        self.store().await
    }

    async fn directory(&self) -> Result<PgStore<Client>, ApiError> {
        self.store().await
    }

    /// The stores table is in the store's database (model A).
    fn directory_in<'p>(&'p self, ports: &'p PgStore<Client>) -> Option<&'p PgStore<Client>> {
        Some(ports)
    }

    fn search<'p>(&'p self, ports: &'p PgStore<Client>) -> &'p PgStore<Client> {
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

    /// Each request's connection is its own and closes when dropped, so a preview whose
    /// request was dropped half-way ends with it.
    fn begin_preview<'a>(
        &'a self,
        ports: &'a Self::Ports<'_>,
        timeout: std::time::Duration,
    ) -> Pin<Box<dyn Future<Output = Result<OpenPreview, ApiError>> + Send + 'a>> {
        Box::pin(async move { Ok(open_preview(ports.begin_preview(timeout).await?)) })
    }

    fn begin_apply<'a>(
        &'a self,
        ports: &'a Self::Ports<'_>,
    ) -> Pin<Box<dyn Future<Output = Result<OpenPreview, ApiError>> + Send + 'a>> {
        Box::pin(async move { Ok(open_preview(ports.begin_apply().await?)) })
    }

    fn spawn_password_reset(state: SharedState<Self>, email: String) {
        let background = Arc::clone(&state);
        let work = hypha_http::routes::password_reset_in_background(state, email);
        if !background.background.spawn(work) {
            tracing::warn!("password reset request dropped");
        }
    }
}

/// A store's open preview or apply, as `hypha-http` ends it.
fn open_preview<C>(open: hypha_store_postgres::PgPreview<C>) -> OpenPreview
where
    C: hypha_store_postgres::PgConnection + Send + 'static,
{
    let reader = open.clone();
    OpenPreview::new(
        move || {
            let reader = reader.clone();
            Box::pin(async move { reader.events().await })
        },
        move |commit, token| Box::pin(open.finish(commit, token)),
    )
}
