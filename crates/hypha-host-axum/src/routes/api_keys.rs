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

#[derive(Deserialize, Clone, Copy, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum KindRequest {
    Secret,
    Publishable,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateApiKeyRequest {
    kind: KindRequest,
    name: String,
    /// Required for a secret key; a publishable key always has the storefront scopes.
    scopes: Option<Vec<Scope>>,
}

#[derive(Serialize, utoipa::ToSchema)]
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

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct CreatedApiKeyResponse {
    #[serde(flatten)]
    key: ApiKeyResponse,
    /// The key itself, shown only in this response.
    token: String,
}

/// Creates an API key; its token is shown only in this response.
#[utoipa::path(
    post,
    path = "/admin/api-keys",
    request_body = CreateApiKeyRequest,
    responses((status = 201, description = "Created", body = CreatedApiKeyResponse)),
    extensions(("x-hypha-scopes" = json!(["api_keys:write"]))),
)]
pub(super) async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreatedApiKeyResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
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

/// An API key, redacted.
#[utoipa::path(
    get,
    path = "/admin/api-keys/{id}",
    params(("id" = ApiKeyId, Path)),
    responses((status = 200, description = "OK", body = ApiKeyResponse)),
    extensions(("x-hypha-scopes" = json!(["api_keys:read"]))),
)]
pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ApiKeyId>,
) -> Result<Json<ApiKeyResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let key = get_api_key(&ctx, ports.api_keys(), id).await?;
    Ok(Json(ApiKeyResponse::new(&key)?))
}

/// Stops the key working from the next request on. 409 if it already was revoked.
#[utoipa::path(
    post,
    path = "/admin/api-keys/{id}/revoke",
    params(("id" = ApiKeyId, Path)),
    responses((status = 200, description = "OK", body = ApiKeyResponse)),
    extensions(("x-hypha-scopes" = json!(["api_keys:write"]))),
)]
pub(super) async fn revoke<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ApiKeyId>,
) -> Result<Json<ApiKeyResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let key = revoke_api_key(&ctx, ports.api_keys(), &state.clock, id).await?;
    Ok(Json(ApiKeyResponse::new(&key)?))
}
