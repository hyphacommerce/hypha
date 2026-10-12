mod error;
mod extract;
mod routes;
mod state;

use std::sync::Arc;

use axum::Router;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::state::{AppState, MemoryStorage, PgStorage, SharedState, UuidV7Ids};

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

async fn serve<S: Send + Sync + 'static>(router: Router<SharedState<S>>, storage: S) {
    let state = Arc::new(AppState {
        storage,
        ids: UuidV7Ids,
    });
    let app = router.layer(TraceLayer::new_for_http()).with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("port 3000 should be free");
    tracing::info!("Hypha listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await.expect("server error");
}
