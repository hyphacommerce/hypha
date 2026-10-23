//! The MCP endpoint (`POST /mcp`, Streamable HTTP, stateless JSON) and its OAuth Protected
//! Resource Metadata (RFC 9728). In no OpenAPI document: the MCP specification and the
//! RFC document them.
//!
//! The protocol is `hypha_mcp`'s; this module is the transport: the kill switch, the
//! `Origin` check, the credential (an autonomous agent's key `hypha_ak_…`, or a connected
//! agent's OAuth access token, issued for `/mcp` only), the content type and the body's
//! size. Without credentials the answer is `401` with
//! `WWW-Authenticate: Bearer resource_metadata="…"`, which is how an MCP client learns to
//! start OAuth. GET and DELETE are `405` (the router's): there is no stream to open and no
//! session to end.

use axum::{
    Json,
    body::Body,
    extract::{Request, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE, ORIGIN, WWW_AUTHENTICATE},
    },
    response::{IntoResponse, Response},
};
use hypha_auth::TokenKind;
use hypha_core::AgentId;
use hypha_mcp::{Headers, METHOD_HEADER, McpRequest, NAME_HEADER, PROTOCOL_VERSION_HEADER};
use serde::Serialize;

use super::oauth::Issuer;
use crate::{
    agents::{NoteUse, acting},
    error::ApiError,
    mcp::{AgentOperations, challenge, server},
    state::{SharedState, Storage},
};

/// Where the MCP server is, below the API's base URL: the resource agents' tokens are
/// for.
pub const MCP_PATH: &str = "/mcp";
/// The Protected Resource Metadata for `/mcp`, at the path RFC 9728 §3.1 builds from the
/// resource (`/.well-known/oauth-protected-resource` + `/mcp`).
pub const RESOURCE_METADATA_PATH: &str = "/.well-known/oauth-protected-resource/mcp";

/// The resource identifier of the MCP server for an API at `issuer`.
pub fn resource(issuer: &str) -> String {
    format!("{}{MCP_PATH}", issuer.trim_end_matches('/'))
}

/// Whether a client's `resource` parameter names this API's MCP server: the same URL,
/// the scheme and host in any case, a trailing slash or not (RFC 8707; the MCP
/// specification asks servers to be lenient there).
pub fn names_mcp(asked: &str, issuer: &str) -> bool {
    let canonical = |raw: &str| {
        let url = url::Url::parse(raw).ok()?;
        if url.fragment().is_some() {
            return None;
        }
        let path = url.path().trim_end_matches('/').to_string();
        Some((
            url.origin().ascii_serialization(),
            path,
            url.query().map(str::to_string),
        ))
    };
    match (canonical(asked), canonical(&resource(issuer))) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// RFC 9728's metadata for the MCP server.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ProtectedResourceMetadata {
    /// `<api>/mcp`: what clients send as `resource`, and all an agent's tokens are good
    /// for.
    resource: String,
    /// This API: it is its own authorization server.
    authorization_servers: Vec<String>,
    bearer_methods_supported: Vec<&'static str>,
    resource_name: &'static str,
    resource_documentation: &'static str,
}

fn metadata(issuer: String) -> Json<ProtectedResourceMetadata> {
    Json(ProtectedResourceMetadata {
        resource: resource(&issuer),
        authorization_servers: vec![issuer],
        bearer_methods_supported: vec!["header"],
        resource_name: "Hypha store admin (MCP)",
        resource_documentation: "https://github.com/hypha-commerce/hypha/tree/main/docs/agents",
    })
}

/// The MCP server's Protected Resource Metadata (RFC 9728), at the path built from its
/// resource.
#[utoipa::path(
    get,
    path = "/.well-known/oauth-protected-resource/mcp",
    tag = "mcp",
    responses((status = 200, description = "OK", body = ProtectedResourceMetadata)),
    security(()),
)]
pub async fn resource_metadata<S: Storage>(
    Issuer(issuer): Issuer,
) -> Json<ProtectedResourceMetadata> {
    metadata(issuer)
}

/// The same metadata at the root, where clients look when the 401 named none.
#[utoipa::path(
    get,
    path = "/.well-known/oauth-protected-resource",
    tag = "mcp",
    responses((status = 200, description = "OK", body = ProtectedResourceMetadata)),
    security(()),
)]
pub async fn root_resource_metadata<S: Storage>(
    Issuer(issuer): Issuer,
) -> Json<ProtectedResourceMetadata> {
    metadata(issuer)
}

/// `url`'s origin (`scheme://host[:port]`), as browsers send it in `Origin`.
fn origin_of(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .map(|u| u.origin().ascii_serialization())
}

/// Browsers send `Origin`; other clients don't. A present one must be the API's own or
/// the admin's (whose assistant may talk to `/mcp` from the page one day); anything else
/// is a page on another site trying its luck.
fn origin_allowed(headers: &HeaderMap, issuer: &str, admin_url: Option<&str>) -> bool {
    let Some(origin) = headers.get(ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let ours = [Some(issuer), admin_url]
        .into_iter()
        .flatten()
        .filter_map(origin_of);
    ours.into_iter().any(|o| o.eq_ignore_ascii_case(origin))
}

/// A `401` telling an MCP client where to start OAuth.
fn unauthorized(e: ApiError, issuer: &str, error: Option<&str>) -> Response {
    let mut response = e.into_response();
    let metadata = format!("{}{RESOURCE_METADATA_PATH}", issuer.trim_end_matches('/'));
    if let Ok(value) = HeaderValue::from_str(&challenge(&metadata, error)) {
        response.headers_mut().insert(WWW_AUTHENTICATE, value);
    }
    response
}

/// The agent the request's bearer token names: an autonomous agent's key, or a connected
/// agent's access token. `None` without a token; an `Err` for any other.
async fn agent_of<S: Storage>(
    state: &SharedState<S>,
    headers: &HeaderMap,
) -> Result<Option<AgentId>, ApiError> {
    let Some(value) = headers.get(AUTHORIZATION) else {
        return Ok(None);
    };
    let invalid = || ApiError::from(hypha_auth::CommandError::InvalidToken);
    let value = value.to_str().map_err(|_| invalid())?;
    let (scheme, token) = value.split_once(' ').ok_or_else(invalid)?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(invalid());
    }
    let token = token.trim();
    let ports = state.public_ports().await?;
    let agent = match TokenKind::of(token) {
        Some(TokenKind::AgentKey) => {
            use hypha_auth::HasIdentities;
            hypha_auth::authenticate_agent_key(ports.identities(), token).await?
        }
        // An app's access token wasn't issued for `/mcp`: it's refused like any stranger.
        Some(TokenKind::AccessToken) => {
            hypha_oauth::authenticate_agent_token(&ports, &state.clock, token).await?
        }
        _ => return Err(invalid()),
    };
    Ok(Some(agent))
}

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json"))
}

/// One MCP message: a JSON-RPC request answered with its JSON response, a notification
/// with `202`. Stateless: every request carries its credential and protocol version.
#[utoipa::path(
    post,
    path = "/mcp",
    tag = "mcp",
    request_body(content = serde_json::Value, content_type = "application/json"),
    responses(
        (status = 200, description = "A JSON-RPC response"),
        (status = 202, description = "A notification, taken"),
        (status = 401, description = "No agent credential: see WWW-Authenticate"),
    ),
    security(()),
)]
pub async fn handle<S: Storage>(
    State(state): State<SharedState<S>>,
    Issuer(issuer): Issuer,
    request: Request,
) -> Response {
    match serve(&state, &issuer, request).await {
        Ok(response) => response,
        Err(e) => e.into_response(),
    }
}

async fn serve<S: Storage>(
    state: &SharedState<S>,
    issuer: &str,
    request: Request,
) -> Result<Response, ApiError> {
    if !state.config.agents.enabled {
        return Err(ApiError::agents_disabled());
    }
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    if !origin_allowed(&headers, issuer, state.config.server.admin_url.as_deref()) {
        return Err(ApiError::origin_not_allowed());
    }
    let agent = match agent_of(state, &headers).await {
        Ok(Some(agent)) => agent,
        Ok(None) => return Ok(unauthorized(ApiError::unauthenticated(), issuer, None)),
        Err(e) if e.status() == StatusCode::UNAUTHORIZED => {
            return Ok(unauthorized(e, issuer, Some("invalid_token")));
        }
        Err(e) => return Err(e),
    };
    // Found once per request: every tool call in it runs as this.
    let acting = acting(state, agent, NoteUse::Yes).await?;
    if !is_json(&headers) {
        return Err(ApiError::missing_content_type());
    }
    let limit = state.config.server.max_body;
    let bytes = axum::body::to_bytes(Body::new(body), limit)
        .await
        .map_err(|_| ApiError::body_too_large(limit))?;
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let mcp_request = McpRequest {
        body: &bytes,
        headers: Headers {
            protocol_version: header(PROTOCOL_VERSION_HEADER),
            method: header(METHOD_HEADER),
            name: header(NAME_HEADER),
        },
    };
    let ops = AgentOperations::new(state, acting);
    let answer = server().handle(&ops, mcp_request).await;
    let status = StatusCode::from_u16(answer.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    Ok(match answer.body {
        Some(body) => (status, Json(body)).into_response(),
        None => status.into_response(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resource_names_mcp_leniently() {
        let issuer = "https://api.shop.example";
        assert!(names_mcp("https://api.shop.example/mcp", issuer));
        assert!(names_mcp("HTTPS://API.shop.example/mcp/", issuer));
        assert!(!names_mcp("https://api.shop.example", issuer));
        assert!(!names_mcp("https://evil.example/mcp", issuer));
        assert!(!names_mcp("https://api.shop.example/mcp#x", issuer));
        assert!(!names_mcp("api.shop.example/mcp", issuer));
    }

    #[test]
    fn only_our_origins_may_post() {
        let mut headers = HeaderMap::new();
        let issuer = "https://api.shop.example";
        let admin = Some("https://admin.shop.example/");
        assert!(
            origin_allowed(&headers, issuer, admin),
            "no Origin: a client"
        );
        headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://admin.shop.example"),
        );
        assert!(origin_allowed(&headers, issuer, admin));
        headers.insert(ORIGIN, HeaderValue::from_static("https://api.shop.example"));
        assert!(origin_allowed(&headers, issuer, admin));
        headers.insert(ORIGIN, HeaderValue::from_static("https://evil.example"));
        assert!(!origin_allowed(&headers, issuer, admin));
        headers.insert(ORIGIN, HeaderValue::from_static("null"));
        assert!(!origin_allowed(&headers, issuer, None));
    }
}
