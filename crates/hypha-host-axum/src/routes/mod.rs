//! HTTP routes. Each module owns its request/response shapes (the API's own types,
//! separate from domain types) and thin handlers that call one domain command.

mod carts;
mod markets;
mod products;

use axum::Router;

use crate::{error::ApiError, state::SharedState};

pub fn router() -> Router<SharedState> {
    Router::new()
        .merge(products::router())
        .merge(markets::router())
        .merge(carts::router())
        .fallback(|| async { ApiError::not_found("no such route") })
        .method_not_allowed_fallback(|| async { ApiError::method_not_allowed() })
}
