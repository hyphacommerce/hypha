use axum::{Json, extract::State};
use hypha_core::{Email, Language, StoreId, Timezone};
use hypha_stores::{Store, UpdateStore, get_store, update_store};
use serde::{Deserialize, Serialize};

use super::address::{PostalAddressRequest, PostalAddressResponse};
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
    /// Where shoppers and agents can write to the store.
    contact_email: Option<Email>,
    /// Plain text, at most 500 characters.
    description: Option<String>,
    /// The registered business name, for invoices and legal pages.
    legal_name: Option<String>,
    legal_address: Option<PostalAddressResponse>,
    /// Clients convert times with it; the API keeps every time in UTC.
    timezone: Option<Timezone>,
    /// The language the store's content (titles, descriptions, option labels) is written
    /// in (design note 0021, SU5).
    default_language: Language,
}

impl From<&Store> for StoreResponse {
    fn from(s: &Store) -> Self {
        Self {
            id: s.id(),
            name: s.name().to_string(),
            contact_email: s.contact_email().cloned(),
            description: s.description().map(str::to_string),
            legal_name: s.legal_name().map(str::to_string),
            legal_address: s.legal_address().map(PostalAddressResponse::from),
            timezone: s.timezone(),
            default_language: s.default_language().clone(),
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
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    contact_email: Option<Option<String>>,
    /// Plain text, at most 500 characters, trimmed; `null` or blank clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    description: Option<Option<String>>,
    /// At most 255 characters, trimmed; `null` or blank clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    legal_name: Option<Option<String>>,
    /// Replaced whole; `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<PostalAddressRequest>, required = false)]
    legal_address: Option<Option<PostalAddressRequest>>,
    /// An IANA name such as `Europe/Belgrade`; `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    timezone: Option<Option<String>>,
    /// A BCP 47 tag such as `en` or `sr-Latn`.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    default_language: Option<String>,
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
    let input = UpdateStore {
        name: req.name,
        contact_email: req.contact_email,
        description: req.description,
        legal_name: req.legal_name,
        legal_address: req.legal_address.map(|a| a.map(Into::into)),
        timezone: req.timezone,
        default_language: req.default_language,
    };
    let store = update_store(&ctx, &directory, state.store, input).await?;
    Ok(Json(StoreResponse::from(&store)))
}

/// What a storefront sees of the store (design note 0021, SU4): what a header, footer
/// or contact page shows. Legal details stay in the admin until a storefront needs them.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct StorefrontStoreResponse {
    name: String,
    /// Plain text, at most 500 characters.
    description: Option<String>,
    contact_email: Option<Email>,
    /// The language the store's content is written in.
    default_language: Language,
}

/// The store's public details.
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
        description: store.description().map(str::to_string),
        contact_email: store.contact_email().cloned(),
        default_language: store.default_language().clone(),
    }))
}
