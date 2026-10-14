use axum::{Json, extract::State};
use hypha_core::StoreId;
use hypha_stores::{Store, get_store};
use serde::Serialize;

use crate::{
    error::ApiError,
    extract::Caller,
    state::{SharedState, Storage},
};

#[derive(Serialize)]
pub(super) struct StoreResponse {
    id: StoreId,
    name: String,
}

impl From<&Store> for StoreResponse {
    fn from(s: &Store) -> Self {
        Self {
            id: s.id(),
            name: s.name().to_string(),
        }
    }
}

/// The store this request is served by.
pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<StoreResponse>, ApiError> {
    // The ports are only needed to authenticate; `_` drops them (and their pooled
    // connection) at the end of this statement, before the directory takes one.
    let (_, ctx) = caller.authenticate(&state).await?;
    let directory = state.storage.directory().await?;
    let store = get_store(&ctx, &directory, state.store).await?;
    Ok(Json(StoreResponse::from(&store)))
}
