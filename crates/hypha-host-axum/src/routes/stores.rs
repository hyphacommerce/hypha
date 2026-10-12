use axum::{Json, extract::State};
use hypha_core::{Ctx, StoreId};
use hypha_stores::{Store, get_store};
use serde::Serialize;

use crate::{
    error::ApiError,
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
) -> Result<Json<StoreResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let directory = state.storage.directory().await?;
    let store = get_store(&ctx, &directory, state.store).await?;
    Ok(Json(StoreResponse::from(&store)))
}
