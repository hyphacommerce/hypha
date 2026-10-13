mod error;
mod extract;
mod routes;
mod state;

use std::sync::Arc;

use axum::Router;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use hypha_core::Ctx;
use hypha_stores::ensure_single_store;

use crate::state::{AppState, MemoryStorage, PgStorage, SharedState, Storage, UuidV7Ids};

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
        store: store.id(),
    });
    let app = router.layer(TraceLayer::new_for_http()).with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("port 3000 should be free");
    tracing::info!("Hypha listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await.expect("server error");
}
