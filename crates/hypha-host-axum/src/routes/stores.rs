use axum::{Json, extract::State};
use hypha_core::StoreId;
use hypha_stores::{Store, UpdateStore, get_store, update_store};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, Storefront},
    patch,
    state::{SharedState, Storage},
};

#[derive(Serialize, utoipa::ToSchema)]
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
#[utoipa::path(
    get,
    path = "/admin/store",
    responses((status = 200, description = "OK", body = StoreResponse)),
    extensions(("x-hypha-scopes" = json!(["stores:read"]))),
)]
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

/// A JSON Merge Patch of the store: absent fields stay as they are.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateStoreRequest {
    /// 1 to 100 characters, trimmed.
    #[serde(default, deserialize_with = "patch::non_null")]
    // Optional, but `null` is refused (`patch::non_null`): not nullable in the spec.
    #[schema(value_type = String, required = false)]
    name: Option<String>,
}

/// Changes the store's details.
#[utoipa::path(
    patch,
    path = "/admin/store",
    request_body = UpdateStoreRequest,
    responses((status = 200, description = "OK", body = StoreResponse)),
    extensions(("x-hypha-scopes" = json!(["stores:write"]))),
)]
pub(super) async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<UpdateStoreRequest>,
) -> Result<Json<StoreResponse>, ApiError> {
    // As in `fetch`: the ports go before the directory takes a connection.
    let (_, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let directory = state.storage.directory().await?;
    let input = UpdateStore { name: req.name };
    let store = update_store(&ctx, &directory, state.store, input).await?;
    Ok(Json(StoreResponse::from(&store)))
}

/// What a storefront sees of the store: its name.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct StorefrontStoreResponse {
    name: String,
}

/// The store's name.
#[utoipa::path(
    get,
    path = "/storefront/store",
    responses((status = 200, description = "OK", body = StorefrontStoreResponse)),
    extensions(("x-hypha-scopes" = json!(["stores:read"]))),
)]
pub(super) async fn fetch_storefront<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
) -> Result<Json<StorefrontStoreResponse>, ApiError> {
    // The ports are only needed to authenticate; `_` drops them before the directory
    // takes a connection.
    let (_, ctx) = caller.authenticate(&state).await?;
    let directory = state.storage.directory().await?;
    let store = get_store(&ctx, &directory, state.store).await?;
    Ok(Json(StorefrontStoreResponse {
        name: store.name().to_string(),
    }))
}
