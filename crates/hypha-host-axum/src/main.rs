mod error;
mod extract;
mod routes;
mod state;

use std::sync::Arc;

use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::state::{AppState, SharedState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    let state: SharedState = Arc::new(AppState::default());

    let app = routes::router()
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("port 3000 should be free");
    tracing::info!("Hypha listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await.expect("server error");
}
