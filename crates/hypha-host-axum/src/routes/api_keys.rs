//! API keys (design note 0008, A8): secret keys for servers, publishable keys for
//! storefronts.

use axum::{Json, extract::State, http::StatusCode};
use hypha_auth::{
    ApiKey, ApiKeyKind, CreateApiKey, HasApiKeys, create_api_key, get_api_key, revoke_api_key,
};
use hypha_core::{ApiKeyId, Scope};
use serde::{Deserialize, Serialize};

use super::rfc3339;
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    state::{SharedState, Storage},
};

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub(super) enum KindRequest {
    Secret,
    Publishable,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateApiKeyRequest {
    kind: KindRequest,
    name: String,
    /// Required for a secret key; a publishable key always has the storefront scopes.
    scopes: Option<Vec<Scope>>,
}

#[derive(Serialize)]
pub(super) struct ApiKeyResponse {
    id: ApiKeyId,
    kind: &'static str,
    name: String,
    /// `hypha_sk_…a1b2`: enough to tell keys apart.
    redacted: String,
    scopes: Vec<Scope>,
    created_at: String,
    revoked_at: Option<String>,
}

impl ApiKeyResponse {
    fn new(k: &ApiKey) -> Result<Self, ApiError> {
        Ok(Self {
            id: k.id(),
            kind: k.kind().as_str(),
            name: k.name().to_string(),
            redacted: k.redacted(),
            scopes: k.scopes().to_vec(),
            created_at: rfc3339(k.created_at())?,
            revoked_at: k.revoked_at().map(rfc3339).transpose()?,
        })
    }
}

#[derive(Serialize)]
pub(super) struct CreatedApiKeyResponse {
    #[serde(flatten)]
    key: ApiKeyResponse,
    /// The key itself, shown only in this response.
    token: String,
}

pub(super) async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    JsonBody(req): JsonBody<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreatedApiKeyResponse>), ApiError> {
    let ports = state.ports().await?;
    let input = CreateApiKey {
        kind: match req.kind {
            KindRequest::Secret => ApiKeyKind::Secret,
            KindRequest::Publishable => ApiKeyKind::Publishable,
        },
        name: req.name,
        scopes: req.scopes,
    };
    let (key, token) = create_api_key(&ctx, ports.api_keys(), &state.env, input).await?;
    let response = CreatedApiKeyResponse {
        key: ApiKeyResponse::new(&key)?,
        token: token.expose().to_string(),
    };
    Ok((StatusCode::CREATED, Json(response)))
}

pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<ApiKeyId>,
) -> Result<Json<ApiKeyResponse>, ApiError> {
    let ports = state.ports().await?;
    let key = get_api_key(&ctx, ports.api_keys(), id).await?;
    Ok(Json(ApiKeyResponse::new(&key)?))
}

/// Stops the key working from the next request on. 409 if it already was revoked.
pub(super) async fn revoke<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<ApiKeyId>,
) -> Result<Json<ApiKeyResponse>, ApiError> {
    let ports = state.ports().await?;
    let key = revoke_api_key(&ctx, ports.api_keys(), &state.clock, id).await?;
    Ok(Json(ApiKeyResponse::new(&key)?))
}
