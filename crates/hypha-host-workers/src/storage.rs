//! Postgres through Hyperdrive (design note 0026, W4): a connection per `ports()` call,
//! which Hyperdrive's pool next to the database makes cheap; no statement cache (each
//! connection is used once). Migrations aren't run here (W9: `setup` from the CLI).

use hypha_core::StoreId;
use hypha_http::error::ApiError;
use hypha_http::host::HostClock;
use hypha_http::state::{SharedState, Storage};
use hypha_store_postgres::PgStore;
use tokio_postgres::Client;
use worker::Env;
use worker::send::SendFuture;

/// The Hyperdrive binding's name in `wrangler.toml`.
pub const HYPERDRIVE: &str = "HYPERDRIVE";

pub struct WorkersStorage {
    env: Env,
    /// What the stores stamp writes with (design note 0016, Q3).
    clock: HostClock,
}

impl WorkersStorage {
    pub fn new(env: Env, clock: HostClock) -> Self {
        Self { env, clock }
    }

    async fn store(&self) -> Result<PgStore<Client>, ApiError> {
        let client = SendFuture::new(connect(&self.env))
            .await
            .map_err(ApiError::internal)?;
        Ok(PgStore::new(client, self.clock.shared()))
    }
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

impl Storage for WorkersStorage {
    type Ports<'a> = PgStore<Client>;
    type Directory<'a> = PgStore<Client>;
    type Search<'p> = &'p PgStore<Client>;
    type Handlers = crate::handlers::Handlers;

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

    fn handlers(&self) -> &crate::handlers::Handlers {
        &crate::handlers::HANDLERS
    }

    fn spawn_password_reset(state: SharedState<Self>, email: String) {
        let background = std::sync::Arc::clone(&state);
        let work = hypha_http::routes::password_reset_in_background(state, email);
        if !background.background.spawn(work) {
            tracing::warn!("password reset request dropped");
        }
    }
}
