mod error;
mod extract;
mod patch;
mod routes;
mod state;

use std::sync::Arc;

use axum::Router;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use hypha_core::Ctx;
use hypha_stores::ensure_single_store;

use crate::state::{
    AppState, MemoryStorage, PgStorage, SharedState, Storage, SystemClock, UuidV7Ids,
};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    // DATABASE_URL set: Postgres (migrated on startup). Otherwise in memory.
    match std::env::var("DATABASE_URL") {
        Ok(url) => {
            let storage = match PgStorage::connect(&url).await {
                Ok(storage) => storage,
                Err(e) => {
                    tracing::error!("{e}");
                    std::process::exit(1);
                }
            };
            tracing::info!("storage: Postgres (migrations applied)");
            serve(routes::postgres_router(), storage).await;
        }
        Err(_) => {
            tracing::info!("storage: in memory (set DATABASE_URL for Postgres)");
            serve(routes::memory_router(), MemoryStorage::default()).await;
        }
    }
}

async fn serve<S: Storage>(router: Router<SharedState<S>>, storage: S) {
    let ids = UuidV7Ids;
    // One store per deployment (design note 0006, model A): created on first start.
    let name = std::env::var("HYPHA_STORE_NAME").unwrap_or_else(|_| "My store".to_string());
    let store = match storage.directory().await {
        Ok(directory) => ensure_single_store(&Ctx::system(), &directory, &ids, &name)
            .await
            .map_err(|e| e.to_string()),
        Err(_) => Err("cannot reach the store directory".to_string()),
    };
    let store = match store {
        Ok(store) => store,
        Err(e) => {
            tracing::error!("store setup failed: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!(store = %store.id(), name = store.name(), "serving store");

    let state = Arc::new(AppState {
        storage,
        ids,
        clock: SystemClock,
        store: store.id(),
    });
    let app = router.layer(TraceLayer::new_for_http()).with_state(state);

    let addr = bind_address();
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!("cannot listen on {addr}: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!("Hypha listening on http://{addr}");
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        tracing::error!("server error: {e}");
        std::process::exit(1);
    }
    tracing::info!("stopped");
}

/// `HYPHA_BIND` (`host:port`) if set. Otherwise `PORT` on every interface, as platforms
/// like Fly.io and Render expect. Otherwise `127.0.0.1:3000` for local development.
fn bind_address() -> String {
    bind_address_from(std::env::var("HYPHA_BIND").ok(), std::env::var("PORT").ok())
}

fn bind_address_from(bind: Option<String>, port: Option<String>) -> String {
    match (bind, port) {
        (Some(bind), _) => bind,
        (None, Some(port)) => format!("0.0.0.0:{port}"),
        (None, None) => "127.0.0.1:3000".to_string(),
    }
}

/// Resolves on Ctrl+C or (on Unix) SIGTERM, which is how orchestrators stop a
/// container. The server then stops accepting connections and lets in-flight requests
/// finish before `main` returns.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("cannot listen for Ctrl+C: {e}");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(e) => {
                tracing::error!("cannot listen for SIGTERM: {e}");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down: finishing in-flight requests");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_address_prefers_hypha_bind_then_port_then_localhost() {
        let s = |v: &str| Some(v.to_string());
        assert_eq!(bind_address_from(s("[::]:8080"), s("9000")), "[::]:8080");
        assert_eq!(bind_address_from(None, s("9000")), "0.0.0.0:9000");
        assert_eq!(bind_address_from(None, None), "127.0.0.1:3000");
    }
}
