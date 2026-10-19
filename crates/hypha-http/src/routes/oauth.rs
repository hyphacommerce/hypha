//! OAuth 2.1 for apps (design note 0029, AP5, §4.1): Hypha's API is the authorization
//! server.
//!
//! - `GET /oauth/authorize` (public): checks the request against the app's manifest
//!   (its `client_id` is the manifest's URL) and sends the browser to the admin's
//!   consent page, `{server.admin_url}/apps/consent?request=<id>`; an app asking for no
//!   more than its grant goes straight back with a code. A bad `client_id` or
//!   `redirect_uri` is a 400 page (RFC 6749 §4.1.2.1: never redirect to an untrusted
//!   URI); every other problem goes back to the app as `error=…`.
//! - The consent page reads `GET /admin/oauth/requests/{id}` and answers with
//!   `POST …/approve` or `…/deny`, which return where to send the browser.
//! - `POST /oauth/token` and `POST /oauth/revoke` (public, form-encoded, RFC 6749 and
//!   7009 errors `{"error", "error_description"}`), and the RFC 8414 metadata at
//!   `/.well-known/oauth-authorization-server`.
//!
//! The public routes are rate limited per client address (`Rule::OAuthIp`). They are in
//! no OpenAPI document (their RFCs and the metadata document them); their annotations
//! keep the shapes typed.

use axum::{
    Json,
    body::Bytes,
    extract::{FromRequest, Request, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, HOST, LOCATION, PRAGMA},
    },
    response::{IntoResponse, Response},
};
use hypha_core::{OAuthRequestId, Scope};
use hypha_oauth::{
    AuthorizationRequest, AuthorizeError, AuthorizeInput, Authorized, CodeExchange, IssuedTokens,
    Refresh, TokenError, approve, authorize, deny, exchange_code, get_request, group_scopes,
    reasons, refresh, request_private_app, revoke, scope_diff,
};
use serde::{Deserialize, Serialize};

use super::rfc3339;
use crate::{
    client_ip::ClientIp,
    error::ApiError,
    extract::{Caller, JsonBody, PathId, Query, StaffCaller},
    rate_limit::{Rule, enforce},
    state::{SharedState, Storage},
};

/// The most a token or revocation request may weigh: a few parameters.
const MAX_FORM_BYTES: usize = 16 * 1024;

// ---------- authorize ----------

/// `/oauth/authorize`'s query (RFC 6749 §4.1.1 with PKCE). Unknown parameters are
/// ignored, as the RFC asks.
#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuthorizeQuery {
    /// `code`.
    response_type: Option<String>,
    /// The URL of the app's manifest (`hypha-app.json`).
    client_id: Option<String>,
    /// One of the manifest's `redirect_uris`, exactly.
    redirect_uri: Option<String>,
    /// Space-separated scopes, among the manifest's; all of them when absent.
    scope: Option<String>,
    /// Sent back unchanged; at most 512 characters.
    state: Option<String>,
    /// `BASE64URL(SHA256(code_verifier))`.
    code_challenge: Option<String>,
    /// `S256`.
    code_challenge_method: Option<String>,
}

/// Starts an app's authorization: 302 to the admin's consent page (or straight back to
/// the app with a `code` when nothing new needs approving, or with an `error`).
#[utoipa::path(
    get,
    path = "/oauth/authorize",
    tag = "oauth",
    params(AuthorizeQuery),
    responses(
        (status = 302, description = "To the consent page, or back to the app"),
        (status = 400, description = "The client or redirect URI can't be trusted", body = crate::error::ApiErrorBody),
    ),
    security(()),
)]
pub async fn authorize_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    query: Query<AuthorizeQuery>,
) -> Result<Response, ApiError> {
    enforce(&state.limiter, Rule::OAuthIp, &ip).await?;
    let q = query.parse()?;
    let consent_base = state
        .config
        .server
        .admin_url
        .clone()
        .ok_or_else(ApiError::admin_url_missing)?;
    let ports = state.public_ports().await?;
    let input = AuthorizeInput {
        response_type: q.response_type,
        client_id: q.client_id,
        redirect_uri: q.redirect_uri,
        scope: q.scope,
        state: q.state,
        code_challenge: q.code_challenge,
        code_challenge_method: q.code_challenge_method,
    };
    match authorize(&ports, &state.manifests(), &state.env, input).await {
        Ok(Authorized::Consent(request)) => Ok(redirect(&format!(
            "{consent_base}/apps/consent?request={}",
            request.id()
        ))),
        Ok(Authorized::Redirect(location)) => Ok(redirect(&location)),
        Err(AuthorizeError::Redirect { location, .. }) => Ok(redirect(&location)),
        Err(AuthorizeError::Unredirectable(e)) => Err(ApiError::unredirectable(e)),
        Err(AuthorizeError::Repo(e)) => Err(e.into()),
    }
}

fn redirect(location: &str) -> Response {
    let mut response = StatusCode::FOUND.into_response();
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert(LOCATION, value);
    }
    response
}

// ---------- token and revoke ----------

/// A form-encoded body (`application/x-www-form-urlencoded`), read but not parsed:
/// `parse` after the rate limit, like `JsonBody`.
pub struct FormBody<T> {
    form: bool,
    bytes: Bytes,
    _body: std::marker::PhantomData<fn() -> T>,
}

impl<S: Storage, T> FromRequest<SharedState<S>> for FormBody<T> {
    type Rejection = Response;

    async fn from_request(req: Request, _state: &SharedState<S>) -> Result<Self, Response> {
        let form = req
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .is_some_and(|m| {
                m.trim()
                    .eq_ignore_ascii_case("application/x-www-form-urlencoded")
            });
        let bytes = axum::body::to_bytes(req.into_body(), MAX_FORM_BYTES)
            .await
            .map_err(|_| {
                oauth_error(TokenError::InvalidRequest(format!(
                    "the body is larger than {MAX_FORM_BYTES} bytes"
                )))
            })?;
        Ok(Self {
            form,
            bytes,
            _body: std::marker::PhantomData,
        })
    }
}

impl<T: serde::de::DeserializeOwned> FormBody<T> {
    fn parse(self) -> Result<T, TokenError> {
        if !self.form {
            return Err(TokenError::InvalidRequest(
                "send the parameters as application/x-www-form-urlencoded".to_string(),
            ));
        }
        serde_html_form::from_bytes(&self.bytes)
            .map_err(|e| TokenError::InvalidRequest(format!("malformed parameters: {e}")))
    }
}

/// `POST /oauth/token`'s parameters (RFC 6749 §4.1.3 and §6).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TokenRequest {
    /// `authorization_code` or `refresh_token`.
    grant_type: String,
    /// `authorization_code`: the code from the redirect.
    code: Option<String>,
    /// `authorization_code`: the PKCE verifier.
    code_verifier: Option<String>,
    /// `authorization_code`: the redirect URI the code was sent to.
    redirect_uri: Option<String>,
    /// The manifest's URL. Required with `authorization_code`.
    client_id: Option<String>,
    /// `refresh_token`: the refresh token, which this replaces.
    refresh_token: Option<String>,
    /// `refresh_token`: space-separated scopes within the token's.
    scope: Option<String>,
}

/// RFC 6749 §5.1's answer. `app_secret` comes with the first installation's code
/// exchange only: it verifies Hypha's webhook and extension-point signatures. `store`
/// and `issuer` come with every answer, so an app keys the installation by them.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TokenResponse {
    /// `hypha_at_…`: send as `Authorization: Bearer`. An hour.
    access_token: String,
    /// `Bearer`.
    token_type: &'static str,
    /// Seconds the access token works.
    expires_in: u64,
    /// `hypha_rt_…`: 30 days, replaced on every use; a used one used again revokes every
    /// token of the app.
    refresh_token: String,
    /// The granted scopes, space-separated.
    scope: String,
    /// The store the tokens are for (`store_…`), as webhooks and extension calls name it
    /// in `Hypha-Store`.
    store: hypha_core::StoreId,
    /// The API's base URL (as `/.well-known/oauth-authorization-server` names it): where
    /// to call with the tokens, and where to refresh them.
    issuer: String,
    /// `hypha_as_…`: the app's signing secret, shown this once.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    app_secret: Option<String>,
}

impl TokenResponse {
    fn new(t: IssuedTokens, store: hypha_core::StoreId, issuer: String) -> Self {
        Self {
            access_token: t.access_token.expose().to_string(),
            token_type: "Bearer",
            expires_in: t.expires_in,
            refresh_token: t.refresh_token.expose().to_string(),
            scope: scope_string(&t.scopes),
            store,
            issuer,
            app_secret: t.app_secret,
        }
    }
}

fn scope_string(scopes: &[Scope]) -> String {
    scopes
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// RFC 6749 §5.2's error body.
#[derive(Serialize, utoipa::ToSchema)]
pub struct OAuthErrorResponse {
    /// `invalid_request`, `invalid_client`, `invalid_grant`, `unsupported_grant_type`,
    /// `invalid_scope` or `server_error`.
    error: &'static str,
    error_description: String,
}

fn oauth_error(e: TokenError) -> Response {
    use hypha_core::ErrorCode;
    let status = match &e {
        TokenError::InvalidClient(_) => StatusCode::UNAUTHORIZED,
        TokenError::Repo(_) | TokenError::Seal(_) => {
            tracing::error!(error = %e, "oauth: internal error");
            StatusCode::INTERNAL_SERVER_ERROR
        }
        _ => StatusCode::BAD_REQUEST,
    };
    let description = match &e {
        TokenError::Repo(_) | TokenError::Seal(_) => "internal error".to_string(),
        other => other.to_string(),
    };
    no_store(
        status,
        Json(OAuthErrorResponse {
            error: e.code(),
            error_description: description,
        }),
    )
}

/// Token answers aren't cached (RFC 6749 §5.1).
fn no_store(status: StatusCode, body: impl IntoResponse) -> Response {
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

fn required(value: Option<String>, name: &str) -> Result<String, TokenError> {
    value
        .filter(|v| !v.is_empty())
        .ok_or_else(|| TokenError::InvalidRequest(format!("{name} is required")))
}

/// Exchanges an authorization code (with its PKCE verifier) or a refresh token for an
/// access token and a new refresh token.
#[utoipa::path(
    post,
    path = "/oauth/token",
    tag = "oauth",
    request_body(content = TokenRequest, content_type = "application/x-www-form-urlencoded"),
    responses(
        (status = 200, description = "OK", body = TokenResponse),
        (status = 400, description = "RFC 6749 §5.2", body = OAuthErrorResponse),
    ),
    security(()),
)]
pub async fn token_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    body: FormBody<TokenRequest>,
) -> Result<Response, ApiError> {
    enforce(&state.limiter, Rule::OAuthIp, &ip).await?;
    let req = match body.parse() {
        Ok(req) => req,
        Err(e) => return Ok(oauth_error(e)),
    };
    let ports = state.public_ports().await?;
    let issued = async {
        match req.grant_type.as_str() {
            "authorization_code" => {
                let input = CodeExchange {
                    code: required(req.code, "code")?,
                    code_verifier: required(req.code_verifier, "code_verifier")?,
                    client_id: required(req.client_id, "client_id")?,
                    redirect_uri: required(req.redirect_uri, "redirect_uri")?,
                };
                exchange_code(&ports, &state.env, input).await
            }
            "refresh_token" => {
                let input = Refresh {
                    refresh_token: required(req.refresh_token, "refresh_token")?,
                    client_id: req.client_id,
                    scope: req.scope,
                };
                refresh(&ports, &state.env, input).await
            }
            other => Err(TokenError::UnsupportedGrantType(other.to_string())),
        }
    }
    .await;
    Ok(match issued {
        Ok(tokens) => no_store(
            StatusCode::OK,
            Json(TokenResponse::new(tokens, state.store, issuer(&headers))),
        ),
        Err(e) => oauth_error(e),
    })
}

/// `POST /oauth/revoke`'s parameters (RFC 7009).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RevokeRequest {
    /// An access or refresh token: its whole family is revoked.
    token: String,
    /// `access_token` or `refresh_token`; the token's prefix says anyway.
    token_type_hint: Option<String>,
}

/// Revokes a token and every token issued with it. 200 whether or not the token was
/// known (RFC 7009 §2.2).
#[utoipa::path(
    post,
    path = "/oauth/revoke",
    tag = "oauth",
    request_body(content = RevokeRequest, content_type = "application/x-www-form-urlencoded"),
    responses(
        (status = 200, description = "Revoked, or never known"),
        (status = 400, description = "RFC 6749 §5.2", body = OAuthErrorResponse),
    ),
    security(()),
)]
pub async fn revoke_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    body: FormBody<RevokeRequest>,
) -> Result<Response, ApiError> {
    enforce(&state.limiter, Rule::OAuthIp, &ip).await?;
    let req = match body.parse() {
        Ok(req) => req,
        Err(e) => return Ok(oauth_error(e)),
    };
    let _ = req.token_type_hint;
    let ports = state.public_ports().await?;
    Ok(match revoke(&ports, &state.clock, &req.token).await {
        Ok(()) => no_store(StatusCode::OK, ()),
        Err(e) => oauth_error(e),
    })
}

/// RFC 8414's metadata.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ServerMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    revocation_endpoint: String,
    response_types_supported: Vec<&'static str>,
    grant_types_supported: Vec<&'static str>,
    code_challenge_methods_supported: Vec<&'static str>,
    /// Apps are public clients: no client secret at the token endpoint (PKCE instead).
    token_endpoint_auth_methods_supported: Vec<&'static str>,
    revocation_endpoint_auth_methods_supported: Vec<&'static str>,
    scopes_supported: Vec<Scope>,
    /// Client IDs are URLs of client metadata documents (the app's manifest).
    client_id_metadata_document_supported: bool,
}

/// The API's own base URL, as the request reached it: `X-Forwarded-Proto` (from a proxy
/// that ends TLS) or `https`, except `http` for a local host.
pub(crate) fn issuer(headers: &HeaderMap) -> String {
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let local = host.starts_with("localhost")
        || host.starts_with("127.0.0.1")
        || host.starts_with("[::1]")
        || host
            .split(':')
            .next()
            .is_some_and(|h| h.ends_with(".localhost"));
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(if local { "http" } else { "https" });
    format!("{scheme}://{host}")
}

/// The authorization server's metadata (RFC 8414).
#[utoipa::path(
    get,
    path = "/.well-known/oauth-authorization-server",
    tag = "oauth",
    responses((status = 200, description = "OK", body = ServerMetadata)),
    security(()),
)]
pub async fn metadata<S: Storage>(headers: HeaderMap) -> Json<ServerMetadata> {
    let base = issuer(&headers);
    Json(ServerMetadata {
        authorization_endpoint: format!("{base}/oauth/authorize"),
        token_endpoint: format!("{base}/oauth/token"),
        revocation_endpoint: format!("{base}/oauth/revoke"),
        issuer: base,
        response_types_supported: vec!["code"],
        grant_types_supported: vec!["authorization_code", "refresh_token"],
        code_challenge_methods_supported: vec!["S256"],
        token_endpoint_auth_methods_supported: vec!["none"],
        revocation_endpoint_auth_methods_supported: vec!["none"],
        scopes_supported: Scope::ALL.to_vec(),
        client_id_metadata_document_supported: true,
    })
}

// ---------- consent (admin) ----------

/// The app as its manifest describes it, for the consent page.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ConsentApp {
    /// `app_<name>`.
    key: String,
    name: String,
    /// The app's own version.
    version: String,
    publisher: Option<String>,
    /// A square image's URL.
    icon: Option<String>,
    /// The publisher's site (the manifest's `app_url`).
    app_url: String,
    /// The app's privacy policy: link it.
    privacy_url: String,
    /// The manifest's URL: the app's client ID.
    client_id: String,
    /// Whether the app is installed (a grant in force): then this is a change of scopes.
    installed: bool,
    /// A private app (its manifest uploaded to this store): approving answers with its
    /// credentials instead of a redirect.
    private: bool,
}

/// Scopes of one area (`orders`), for the consent page.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ScopeGroup {
    /// The scopes' prefix: `orders`, `products`, `checkout`.
    area: &'static str,
    scopes: Vec<Scope>,
}

/// A personal-data scope and why the app says it needs it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PersonalDataScope {
    scope: Scope,
    /// From the manifest's `scope_reasons` (required for these).
    reason: String,
}

/// What approving changes compared with the app's current grant (everything is `added`
/// on a first install).
#[derive(Serialize, utoipa::ToSchema)]
pub struct ScopeChanges {
    added: Vec<Scope>,
    removed: Vec<Scope>,
    kept: Vec<Scope>,
}

/// Where a request stands.
#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = OAuthRequestStatus)]
pub enum RequestStatusDto {
    Pending,
    Approved,
    Denied,
    /// Pending, but its ten minutes passed: the app must start again.
    Expired,
}

/// An authorization request, as the consent page shows it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ConsentRequestResponse {
    id: OAuthRequestId,
    status: RequestStatusDto,
    app: ConsentApp,
    /// Every scope asked for, in `Scope` order (personal-data ones included).
    scopes: Vec<Scope>,
    /// The scopes that aren't personal data, by area.
    groups: Vec<ScopeGroup>,
    /// The `customer_data:*` scopes asked for, with the manifest's reasons: list these
    /// apart, so staff see exactly which personal data the app will see.
    personal_data: Vec<PersonalDataScope>,
    changes: ScopeChanges,
    /// Where the browser goes after the answer; empty for a private app.
    redirect_uri: String,
    created_at: String,
    /// The request can be answered until then (RFC 3339).
    expires_at: String,
}

impl ConsentRequestResponse {
    fn new(
        request: &AuthorizationRequest,
        app: Option<&hypha_apps::App>,
        now: hypha_core::Timestamp,
    ) -> Result<Self, ApiError> {
        let manifest = request.manifest();
        let installed = app.is_some_and(|a| a.is_installed());
        let current: Vec<Scope> = app
            .filter(|a| a.is_installed())
            .and_then(|a| a.grant())
            .map(|g| g.scopes().to_vec())
            .unwrap_or_default();
        let diff = scope_diff(&current, request.scopes());
        let (groups, personal) = group_scopes(request.scopes());
        let status = match request.status() {
            hypha_oauth::RequestStatus::Pending if request.is_expired(now) => {
                RequestStatusDto::Expired
            }
            hypha_oauth::RequestStatus::Pending => RequestStatusDto::Pending,
            hypha_oauth::RequestStatus::Approved => RequestStatusDto::Approved,
            hypha_oauth::RequestStatus::Denied => RequestStatusDto::Denied,
        };
        Ok(Self {
            id: request.id(),
            status,
            app: ConsentApp {
                key: manifest.key().to_string(),
                name: manifest.name().to_string(),
                version: manifest.version().to_string(),
                publisher: manifest.publisher().map(str::to_string),
                icon: manifest.icon().map(str::to_string),
                app_url: manifest.app_url().as_str().to_string(),
                privacy_url: manifest.privacy_url().to_string(),
                client_id: request.client_id().to_string(),
                installed,
                private: request.is_private(),
            },
            scopes: request.scopes().to_vec(),
            groups: groups
                .into_iter()
                .map(|(area, scopes)| ScopeGroup { area, scopes })
                .collect(),
            personal_data: reasons(manifest, &personal)
                .into_iter()
                .map(|(scope, reason)| PersonalDataScope { scope, reason })
                .collect(),
            changes: ScopeChanges {
                added: diff.added,
                removed: diff.removed,
                kept: diff.kept,
            },
            redirect_uri: request.redirect_uri().to_string(),
            created_at: rfc3339(request.created_at())?,
            expires_at: rfc3339(request.expires_at())?,
        })
    }
}

/// Where to send the browser after an answer, or a private app's credentials.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ConsentAnswer {
    /// The app's redirect URI with `code` and `state` (approved), or `error=access_denied`
    /// and `state` (denied): navigate the browser there. `null` when a private app was
    /// approved: show `credentials` instead.
    redirect_to: Option<String>,
    /// A private app's tokens (it has no redirect to take a code), present only in the
    /// answer to its approval: show them once, like an API key.
    credentials: Option<PrivateAppCredentials>,
}

/// What a private app's integration needs to act, shown once (design note 0029, "As
/// built: the admin's app UI and private apps").
#[derive(Serialize, utoipa::ToSchema)]
pub struct PrivateAppCredentials {
    /// `app_<name>`.
    app: String,
    /// `hypha_at_…`: send as `Authorization: Bearer`. An hour.
    access_token: String,
    /// Seconds the access token works.
    expires_in: u64,
    /// `hypha_rt_…`: exchange at `token_endpoint` (`grant_type=refresh_token`) for a new
    /// pair before it lapses in 30 days; each is used once.
    refresh_token: String,
    /// Where to refresh: this API's `/oauth/token`.
    token_endpoint: String,
    /// The granted scopes.
    scopes: Vec<Scope>,
    /// `hypha_as_…`: verifies Hypha's webhook and extension-point signatures. Only when
    /// the app is new (or installed again): an update keeps the secret it has.
    app_secret: Option<String>,
}

impl ConsentAnswer {
    fn from_approval(approval: hypha_oauth::Approval, base: &str) -> Self {
        match approval {
            hypha_oauth::Approval::Redirect(location) => Self {
                redirect_to: Some(location),
                credentials: None,
            },
            hypha_oauth::Approval::Credentials { app, tokens } => Self {
                redirect_to: None,
                credentials: Some(PrivateAppCredentials {
                    app: app.to_string(),
                    access_token: tokens.access_token.expose().to_string(),
                    expires_in: tokens.expires_in,
                    refresh_token: tokens.refresh_token.expose().to_string(),
                    token_endpoint: format!("{base}/oauth/token"),
                    scopes: tokens.scopes,
                    app_secret: tokens.app_secret,
                }),
            },
        }
    }
}

/// An authorization request for the consent page: the app, the scopes grouped by area,
/// the personal-data scopes apart with the manifest's reasons, and what changes
/// compared with the app's grant.
#[utoipa::path(
    get,
    path = "/admin/oauth/requests/{id}",
    params(("id" = OAuthRequestId, Path)),
    responses((status = 200, description = "OK", body = ConsentRequestResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn fetch_request<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<OAuthRequestId>,
) -> Result<Json<ConsentRequestResponse>, ApiError> {
    use hypha_core::Clock;
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (request, app) = get_request(&ctx, &ports, id).await?;
    Ok(Json(ConsentRequestResponse::new(
        &request,
        app.as_ref(),
        state.clock.now(),
    )?))
}

/// Approves a request: installs the app (or replaces its grant) with the scopes asked
/// for. The staff member needs `apps:write` and every scope requested
/// (`not_allowed_to_grant`, 403, otherwise). 409 `oauth_request_expired` after ten
/// minutes, `oauth_request_decided` once answered. A private app's approval answers
/// with its `credentials` (and no `redirect_to`), shown this once.
#[utoipa::path(
    post,
    path = "/admin/oauth/requests/{id}/approve",
    params(("id" = OAuthRequestId, Path)),
    responses((status = 200, description = "OK", body = ConsentAnswer)),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn approve_request<S: Storage>(
    State(state): State<SharedState<S>>,
    headers: HeaderMap,
    caller: Caller,
    PathId(id): PathId<OAuthRequestId>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let approval = approve(&ctx, &ports, &state.env, id).await?;
    let answer = ConsentAnswer::from_approval(approval, &issuer(&headers));
    // Credentials are in it: never cached (RFC 6749 §5.1's rule for token answers).
    Ok(no_store(StatusCode::OK, Json(answer)))
}

/// `POST /admin/apps/private`'s body: a private app's manifest, as the JSON its
/// `hypha-app.json` would hold.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PrivateAppRequest {
    /// The manifest (design note 0029, §4). Checked like a served one, except that
    /// `redirect_uris` may be empty.
    #[schema(value_type = std::collections::HashMap<String, serde_json::Value>)]
    manifest: serde_json::Value,
}

/// A private app waiting for approval.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PrivateAppResponse {
    /// The authorization request, as `GET /admin/oauth/requests/{id}` shows it.
    request: ConsentRequestResponse,
    /// The admin's consent page for it (`{server.admin_url}/apps/consent?request=<id>`),
    /// `null` without `server.admin_url`.
    consent_url: Option<String>,
}

/// Uploads a private app's manifest (a store's own integration with no public site,
/// design note 0029, §4.1): it becomes a pending authorization request for the consent
/// page, whose approval installs the app and answers with its tokens. Staff sessions
/// only (an API key can't install apps); `apps:write`. 422 `manifest_invalid` with
/// `details.errors`; 409 `app_installed_from_url` when the key is an app installed from
/// its manifest URL.
#[utoipa::path(
    post,
    path = "/admin/apps/private",
    request_body = PrivateAppRequest,
    responses((status = 201, description = "Created", body = PrivateAppResponse)),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn create_private<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: StaffCaller,
    body: JsonBody<PrivateAppRequest>,
) -> Result<(StatusCode, Json<PrivateAppResponse>), ApiError> {
    use hypha_core::Clock;
    let (ports, staff) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    // The manifest as uploaded, byte for byte the JSON it was (within the size limit).
    let bytes = serde_json::to_vec(&req.manifest).map_err(ApiError::internal)?;
    let request = request_private_app(&staff.ctx, &ports, &state.env, &bytes).await?;
    let app = {
        use hypha_apps::{AppRepo, HasApps};
        ports.apps().get(request.app()).await?
    };
    let consent_url = state
        .config
        .server
        .admin_url
        .as_ref()
        .map(|base| format!("{base}/apps/consent?request={}", request.id()));
    Ok((
        StatusCode::CREATED,
        Json(PrivateAppResponse {
            request: ConsentRequestResponse::new(&request, app.as_ref(), state.clock.now())?,
            consent_url,
        }),
    ))
}

/// Denies a request; the app is told `access_denied` (a private app's answer has no
/// `redirect_to`).
#[utoipa::path(
    post,
    path = "/admin/oauth/requests/{id}/deny",
    params(("id" = OAuthRequestId, Path)),
    responses((status = 200, description = "OK", body = ConsentAnswer)),
    extensions(("x-hypha-scopes" = json!(["apps:write"]))),
)]
pub async fn deny_request<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<OAuthRequestId>,
) -> Result<Json<ConsentAnswer>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let redirect_to = deny(&ctx, &ports, &state.clock, id).await?;
    Ok(Json(ConsentAnswer {
        redirect_to,
        credentials: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_issuer_is_the_host_the_request_reached() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, HeaderValue::from_static("api.shop.example"));
        assert_eq!(issuer(&headers), "https://api.shop.example");
        headers.insert(HOST, HeaderValue::from_static("localhost:3000"));
        assert_eq!(issuer(&headers), "http://localhost:3000");
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        assert_eq!(issuer(&headers), "https://localhost:3000");
    }
}
