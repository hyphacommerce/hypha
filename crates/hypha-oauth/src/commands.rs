//! The OAuth 2.1 flow that installs apps: authorization requests, staff's consent, codes,
//! tokens with rotation, revocation, grants and uninstalling.

use core::time::Duration;

use hypha_apps::{
    App, AppManifest, AppRepo, Grant, InstalledManifest, ManifestFetcher, ManifestSource,
    StageApps, check_retypes, fetch_manifest, is_private_client, new_secret, normalize,
    open_secret, private_client_id,
};
use hypha_auth::{NewToken, TokenHash, TokenKind};
use hypha_core::{
    Actor, AppId, AppKey, Clock, Ctx, IdGenerator, OAuthRequestId, Random, RepoError, Scope,
    SecretBox, Timestamp,
};
use uuid::Uuid;

use crate::{
    AuthorizationCode, AuthorizationRequest, CommandError, NewCode, NewRequest, NewTokenPair,
    OAuthToken, RequestStatus,
    error::{AuthorizeError, TokenError, UnredirectableError},
    pkce,
    repo::{
        HasOAuthRequests, HasOAuthTokens, OAuthCodeRepo, OAuthPorts, OAuthRequestRepo,
        OAuthTokenRepo, StageOAuth,
    },
};

/// How long a request waits for staff.
pub const REQUEST_LIFETIME: Duration = Duration::from_secs(10 * 60);
/// How long a code works.
pub const CODE_LIFETIME: Duration = Duration::from_secs(60);
/// How long an access token works.
pub const ACCESS_TOKEN_LIFETIME: Duration = Duration::from_secs(60 * 60);
/// How long a refresh token works, unless it's used (and replaced) before.
pub const REFRESH_TOKEN_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// The longest `state` kept and sent back.
pub const MAX_STATE_LEN: usize = 512;

/// The ports for minting tokens and sealing new apps' secrets.
pub trait OAuthEnv: IdGenerator + Clock + Random + SecretBox {}

impl<T: IdGenerator + Clock + Random + SecretBox> OAuthEnv for T {}

// ---------- authorize ----------

/// `/oauth/authorize`'s query, as sent.
#[derive(Debug, Clone, Default)]
pub struct AuthorizeInput {
    pub response_type: Option<String>,
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    /// Space-separated; the manifest's scopes when absent.
    pub scope: Option<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
}

/// What `/oauth/authorize` does next.
#[derive(Debug, Clone, PartialEq)]
pub enum Authorized {
    /// Staff must answer: the browser goes to the admin's consent page for it.
    Consent(Box<AuthorizationRequest>),
    /// Nothing new to approve (the app asks for no more than it was granted): the
    /// browser goes straight back to the app, with a code.
    Redirect(String),
}

/// Checks an authorization request against the app's manifest (fetched from
/// `client_id`) and stores it for staff's consent; an app asking for no scope beyond
/// its grant gets a code at once for the scopes it asked for, its grant untouched.
pub async fn authorize<S: OAuthPorts>(
    ports: &S,
    fetcher: &impl ManifestFetcher,
    env: &impl OAuthEnv,
    input: AuthorizeInput,
) -> Result<Authorized, AuthorizeError> {
    let client_id = input
        .client_id
        .filter(|c| !c.trim().is_empty())
        .ok_or(UnredirectableError::MissingClientId)?;
    let manifest = fetch_manifest(fetcher, &client_id)
        .await
        .map_err(UnredirectableError::Manifest)?;
    let redirect_uri = input
        .redirect_uri
        .filter(|r| !r.is_empty())
        .ok_or(UnredirectableError::MissingRedirectUri)?;
    if !manifest.allows_redirect(&redirect_uri) {
        return Err(UnredirectableError::RedirectUriNotListed(redirect_uri).into());
    }
    let app = ports.apps().get(manifest.key()).await?;
    if let Some(installed) = app.as_ref().and_then(|a| a.manifest())
        && installed.url != client_id
    {
        return Err(UnredirectableError::OtherManifest(manifest.key().clone()).into());
    }
    // A field's type is part of its identity: a manifest that changes one can never be
    // approved, so it doesn't reach the consent page.
    if let Some(installed) = app.as_ref().and_then(|a| a.manifest()) {
        check_retypes(&installed.manifest, &manifest)
            .map_err(|e| UnredirectableError::Manifest(e.into()))?;
    }

    // From here on, errors go back to the app.
    let state = input.state.clone();
    let refuse = |error: &'static str, description: String| {
        let mut params = vec![
            ("error", error.to_string()),
            ("error_description", description.clone()),
        ];
        if let Some(state) = &state {
            params.push(("state", state.clone()));
        }
        AuthorizeError::Redirect {
            location: with_params(&redirect_uri, &params),
            error,
            description,
        }
    };
    if input.response_type.as_deref() != Some("code") {
        return Err(refuse(
            "unsupported_response_type",
            "response_type must be code".to_string(),
        ));
    }
    if state.as_ref().is_some_and(|s| s.len() > MAX_STATE_LEN) {
        return Err(refuse(
            "invalid_request",
            format!("state is at most {MAX_STATE_LEN} characters"),
        ));
    }
    let Some(challenge) = input.code_challenge.filter(|c| !c.is_empty()) else {
        return Err(refuse(
            "invalid_request",
            "code_challenge is required (PKCE)".to_string(),
        ));
    };
    if input.code_challenge_method.as_deref() != Some("S256") {
        return Err(refuse(
            "invalid_request",
            "code_challenge_method must be S256".to_string(),
        ));
    }
    if !pkce::is_challenge(&challenge) {
        return Err(refuse(
            "invalid_request",
            "code_challenge is a base64url SHA-256 (43 characters)".to_string(),
        ));
    }
    let scopes = match &input.scope {
        None => manifest.scopes().to_vec(),
        Some(scope) => {
            let mut scopes = Vec::new();
            for name in scope.split_whitespace() {
                match Scope::parse(name) {
                    Some(s) if manifest.scopes().contains(&s) => scopes.push(s),
                    Some(_) => {
                        return Err(refuse(
                            "invalid_scope",
                            format!("{name} is not among the manifest's scopes"),
                        ));
                    }
                    None => {
                        return Err(refuse("invalid_scope", format!("{name:?} is not a scope")));
                    }
                }
            }
            normalize(&scopes)
        }
    };

    let now = env.now();
    let previous = app
        .as_ref()
        .filter(|a| a.is_installed())
        .and_then(|a| a.grant())
        .map(|g| g.scopes().to_vec());
    let mut request = AuthorizationRequest::new(NewRequest {
        id: OAuthRequestId::from_uuid(env.new_uuid()),
        client_id: client_id.clone(),
        manifest: manifest.clone(),
        redirect_uri: redirect_uri.clone(),
        state: state.clone(),
        code_challenge: Some(challenge.clone()),
        scopes: scopes.clone(),
        previous_scopes: previous.clone(),
        created_at: now,
        expires_at: now.plus(REQUEST_LIFETIME),
    });

    // Already granted at least this much: no consent needed. The code (and so the
    // tokens) carries the scopes asked for, and the app is left exactly as it is: this
    // route takes no credentials, so it must never change a grant or a manifest (a
    // stranger could otherwise shrink an app's grant, or apply its manifest, by sending
    // a browser here). Narrowing a grant is staff's `PATCH /admin/apps/{key}/grant`;
    // applying a manifest is `refresh_manifest`.
    if let (Some(app), Some(previous)) = (app, previous)
        && scopes.iter().all(|s| previous.contains(s))
    {
        // Answered by the grant staff gave before: the app is its own approver here.
        request
            .decide(true, Actor::App(app.id()), now)
            .map_err(|e| refuse("server_error", e.to_string()))?;
        let (location, mut code) = new_code(&request, &challenge, false, env);
        let mut batch = ports.batch();
        batch.insert_oauth_request(&mut request);
        batch.insert_oauth_code(&mut code);
        ports.commit(batch).await?;
        return Ok(Authorized::Redirect(location));
    }

    ports.oauth_requests().insert(&mut request).await?;
    Ok(Authorized::Consent(Box::new(request)))
}

/// `uri` with `params` added to its query.
fn with_params(uri: &str, params: &[(&str, String)]) -> String {
    match url::Url::parse(uri) {
        Ok(mut url) => {
            url.query_pairs_mut()
                .extend_pairs(params.iter().map(|(k, v)| (*k, v.as_str())));
            url.to_string()
        }
        // Manifests' redirect URIs are checked URLs: this can't happen.
        Err(_) => uri.to_string(),
    }
}

/// A code for `request` (bound to its PKCE `challenge`), and where to send the browser
/// with it.
fn new_code(
    request: &AuthorizationRequest,
    challenge: &str,
    first_install: bool,
    env: &(impl Random + IdGenerator + Clock),
) -> (String, AuthorizationCode) {
    let token = NewToken::generate(TokenKind::AuthorizationCode, env);
    let code = AuthorizationCode::new(NewCode {
        hash: token.hash(),
        request: request.id(),
        app: request.app().clone(),
        client_id: request.client_id().to_string(),
        redirect_uri: request.redirect_uri().to_string(),
        code_challenge: challenge.to_string(),
        scopes: request.scopes().to_vec(),
        first_install,
        family: env.new_uuid(),
        expires_at: env.now().plus(CODE_LIFETIME),
    });
    let mut params = vec![("code", token.expose().to_string())];
    if let Some(state) = request.state() {
        params.push(("state", state.to_string()));
    }
    (with_params(request.redirect_uri(), &params), code)
}

// ---------- consent ----------

/// A request for the consent page, with the app as it is now (`None`: a first
/// install), to show what changes.
pub async fn get_request<S: HasOAuthRequests + hypha_apps::HasApps>(
    ctx: &Ctx,
    ports: &S,
    id: OAuthRequestId,
) -> Result<(AuthorizationRequest, Option<App>), CommandError> {
    ctx.require(Scope::AppsRead)?;
    let request = ports
        .oauth_requests()
        .get(id)
        .await?
        .ok_or(CommandError::RequestNotFound(id))?;
    let app = ports.apps().get(request.app()).await?;
    Ok((request, app))
}

/// What changes between the app's current grant and a request: for the consent page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeDiff {
    pub added: Vec<Scope>,
    pub removed: Vec<Scope>,
    pub kept: Vec<Scope>,
}

pub fn scope_diff(current: &[Scope], requested: &[Scope]) -> ScopeDiff {
    ScopeDiff {
        added: requested
            .iter()
            .filter(|s| !current.contains(s))
            .copied()
            .collect(),
        removed: current
            .iter()
            .filter(|s| !requested.contains(s))
            .copied()
            .collect(),
        kept: requested
            .iter()
            .filter(|s| current.contains(s))
            .copied()
            .collect(),
    }
}

/// What approving a request gives the consent page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approval {
    /// Send the browser here: the app's redirect URI with a code (for a request a
    /// manifest refresh started, the app's `app_url`, without one).
    Redirect(String),
    /// A private app (its manifest uploaded) has no redirect to take a code: its tokens
    /// come back here, to show staff once, with its signing secret when the app is new.
    Credentials {
        app: AppKey,
        tokens: Box<IssuedTokens>,
    },
}

impl Approval {
    /// Where to send the browser, unless it's a private app's credentials.
    pub fn redirect_to(&self) -> Option<&str> {
        match self {
            Approval::Redirect(location) => Some(location),
            Approval::Credentials { .. } => None,
        }
    }
}

/// A write a command prepared without committing it: what it decided (`answer`), its writes
/// staged in `batch`, and the manifest the write applies to the app, if it applies one. The
/// caller stages what this crate can't see in the same batch (the HTTP layer syncs the
/// app's custom fields from `applies`), then commits: the app and what hangs off its
/// manifest change together or not at all.
#[derive(Debug)]
pub struct Prepared<B, T> {
    pub batch: B,
    pub answer: T,
    pub applies: Option<AppManifest>,
}

/// Approves a request and commits, with nothing else in the batch: `prepare_approval`
/// alone, for callers with nothing to add (this crate's tests). Hosts sync the app's
/// custom fields with it, so they use `prepare_approval`.
pub async fn approve<S: OAuthPorts>(
    ctx: &Ctx,
    ports: &S,
    env: &impl OAuthEnv,
    id: OAuthRequestId,
) -> Result<Approval, CommandError> {
    let prepared = prepare_approval(ctx, ports, env, id).await?;
    ports.commit(prepared.batch).await?;
    Ok(prepared.answer)
}

/// Approves a request: installs the app (a new one gets its sealed secret, handed to
/// it once with its first tokens) or replaces its grant. A public app's answer is where
/// to send the browser; a private app's is its tokens (`Approval`). The staff member
/// needs `apps:write` and every scope requested. Staged, not committed (`Prepared`):
/// every approval applies the request's manifest (`applies`).
pub async fn prepare_approval<S: OAuthPorts>(
    ctx: &Ctx,
    ports: &S,
    env: &impl OAuthEnv,
    id: OAuthRequestId,
) -> Result<Prepared<S::Batch, Approval>, CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let mut request = ports
        .oauth_requests()
        .get(id)
        .await?
        .ok_or(CommandError::RequestNotFound(id))?;
    if let Some(missing) = request.scopes().iter().find(|s| !ctx.has_scope(**s)) {
        return Err(CommandError::CannotGrant(*missing));
    }
    let now = env.now();
    request.decide(true, ctx.actor(), now)?;
    let installed = InstalledManifest {
        url: request.client_id().to_string(),
        manifest: request.manifest().clone(),
        fetched_at: request.created_at(),
    };
    let applies = Some(request.manifest().clone());
    let grant = Grant::new(request.scopes(), ctx.actor(), now);
    let app = ports.apps().get(request.app()).await?;
    // The key is an app installed (or uninstalled, keeping its manifest) from another
    // client ID since this was asked: two pending requests for one key from different
    // manifests must not swap the installed one (`authorize` refuses such a request
    // when the app is installed first; this is the other order).
    if let Some(existing) = app.as_ref().and_then(|a| a.manifest())
        && existing.url != request.client_id()
    {
        let key = request.app().clone();
        return Err(if request.is_private() {
            CommandError::InstalledFromUrl(key)
        } else {
            CommandError::InstalledFromOtherUrl(key)
        });
    }
    let mut batch = ports.batch();
    if request.is_private() {
        let key = request.app().clone();
        let tokens = approve_private(&mut batch, env, request, app, installed, grant)?;
        return Ok(Prepared {
            batch,
            answer: Approval::Credentials {
                app: key,
                tokens: Box::new(tokens),
            },
            applies,
        });
    }
    // Started by a manifest refresh (`refresh_manifest`): the app has no PKCE verifier
    // waiting, so it gets no code. The grant changes now; the app learns it from
    // `app.grant_changed` and asks `/oauth/authorize` for tokens with the new scopes,
    // which it then gets at once (nothing new to approve).
    let Some(challenge) = request.code_challenge().map(str::to_string) else {
        let Some(mut app) = app.filter(|a| a.is_installed()) else {
            return Err(hypha_apps::AppError::NotInstalled.into());
        };
        app.apply_manifest(installed)?;
        app.set_grant(grant);
        batch.update_app(&mut app);
        batch.update_oauth_request(&mut request);
        return Ok(Prepared {
            batch,
            answer: Approval::Redirect(request.manifest().app_url().as_str().to_string()),
            applies,
        });
    };
    let first_install = match app {
        None => {
            let (_, sealed) = new_secret(request.app(), env, env);
            let mut app = App::install(AppId::from_uuid(env.new_uuid()), sealed, installed, grant)?;
            batch.insert_app(&mut app);
            true
        }
        Some(mut app) if app.uninstalled_at().is_some() => {
            let (_, sealed) = new_secret(request.app(), env, env);
            app.reinstall(sealed, installed, grant)?;
            batch.update_app(&mut app);
            true
        }
        Some(mut app) => {
            app.apply_manifest(installed)?;
            app.set_grant(grant);
            batch.update_app(&mut app);
            false
        }
    };
    let (location, mut code) = new_code(&request, &challenge, first_install, env);
    batch.update_oauth_request(&mut request);
    batch.insert_oauth_code(&mut code);
    Ok(Prepared {
        batch,
        answer: Approval::Redirect(location),
        applies,
    })
}

/// Approving a private app's request: installs it (new, again after an uninstall, or a
/// hand-made app taking its manifest) or replaces its grant, and issues a token pair at
/// once, since there is no redirect to send a code to. A newly sealed secret comes
/// back with the tokens, as a public app's first code exchange gives it; an app that
/// keeps its secret (a hand-made one, or an update) gets none. Staged in `batch`.
fn approve_private<B: StageApps + StageOAuth>(
    batch: &mut B,
    env: &impl OAuthEnv,
    mut request: AuthorizationRequest,
    app: Option<App>,
    installed: InstalledManifest,
    grant: Grant,
) -> Result<IssuedTokens, CommandError> {
    let (app, app_secret) = match app {
        None => {
            let (secret, sealed) = new_secret(request.app(), env, env);
            let mut app = App::install(AppId::from_uuid(env.new_uuid()), sealed, installed, grant)?;
            batch.insert_app(&mut app);
            (app, Some(secret))
        }
        Some(mut app) if app.uninstalled_at().is_some() => {
            let (secret, sealed) = new_secret(request.app(), env, env);
            app.reinstall(sealed, installed, grant)?;
            batch.update_app(&mut app);
            (app, Some(secret))
        }
        Some(mut app) => {
            app.apply_manifest(installed)?;
            app.set_grant(grant);
            batch.update_app(&mut app);
            (app, None)
        }
    };
    let (access, refresh, mut token) = new_pair(env.new_uuid(), &app, request.scopes(), env);
    batch.update_oauth_request(&mut request);
    batch.insert_oauth_token(&mut token);
    Ok(IssuedTokens {
        access_token: access,
        refresh_token: refresh,
        expires_in: ACCESS_TOKEN_LIFETIME.as_secs(),
        scopes: request.scopes().to_vec(),
        app_secret,
    })
}

/// A private app's manifest, uploaded by staff: checked like a served one except that it
/// may list no `redirect_uris`, and stored as a pending authorization request under the
/// client ID `urn:hypha:private:<key>` for the consent page. Approving it installs the app
/// and answers with its tokens (`Approval::Credentials`). A hand-made app with the same key
/// becomes this private app on approval; one installed from a served manifest can't be
/// replaced by an upload (`app_installed_from_url`).
pub async fn request_private_app<S: OAuthPorts>(
    ctx: &Ctx,
    ports: &S,
    env: &impl OAuthEnv,
    manifest: &[u8],
) -> Result<AuthorizationRequest, CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let manifest = AppManifest::parse_from(manifest, ManifestSource::Uploaded)
        .map_err(hypha_apps::AppError::Manifest)?;
    let client_id = private_client_id(manifest.key());
    let app = ports.apps().get(manifest.key()).await?;
    if let Some(installed) = app.as_ref().and_then(|a| a.manifest())
        && installed.url != client_id
    {
        return Err(CommandError::InstalledFromUrl(manifest.key().clone()));
    }
    if let Some(installed) = app.as_ref().and_then(|a| a.manifest()) {
        check_retypes(&installed.manifest, &manifest).map_err(hypha_apps::AppError::Manifest)?;
    }
    let previous = app
        .as_ref()
        .filter(|a| a.is_installed())
        .and_then(|a| a.grant())
        .map(|g| g.scopes().to_vec());
    let now = env.now();
    let mut request = AuthorizationRequest::new(NewRequest {
        id: OAuthRequestId::from_uuid(env.new_uuid()),
        client_id,
        scopes: manifest.scopes().to_vec(),
        manifest,
        redirect_uri: String::new(),
        state: None,
        code_challenge: None,
        previous_scopes: previous,
        created_at: now,
        expires_at: now.plus(REQUEST_LIFETIME),
    });
    ports.oauth_requests().insert(&mut request).await?;
    Ok(request)
}

/// Denies a request; returns where to send the browser: the app's redirect URI with
/// `error=access_denied` (a refresh's request: the app's `app_url`; a private app's:
/// nowhere, `None`). The grant stays.
pub async fn deny<S: HasOAuthRequests>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: OAuthRequestId,
) -> Result<Option<String>, CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let mut request = ports
        .oauth_requests()
        .get(id)
        .await?
        .ok_or(CommandError::RequestNotFound(id))?;
    request.decide(false, ctx.actor(), clock.now())?;
    ports.oauth_requests().update(&mut request).await?;
    // A private app's: nobody waits anywhere; the admin stays where it is.
    if request.is_private() {
        return Ok(None);
    }
    // A refresh's request: no app is waiting at its redirect URI.
    if request.code_challenge().is_none() {
        return Ok(Some(request.manifest().app_url().as_str().to_string()));
    }
    let mut params = vec![
        ("error", "access_denied".to_string()),
        (
            "error_description",
            "the store declined the authorization".to_string(),
        ),
    ];
    if let Some(state) = request.state() {
        params.push(("state", state.to_string()));
    }
    Ok(Some(with_params(request.redirect_uri(), &params)))
}

/// An app's requests, newest first: its grant's history.
pub async fn request_history<S: HasOAuthRequests>(
    ctx: &Ctx,
    ports: &S,
    app: &AppKey,
) -> Result<Vec<AuthorizationRequest>, CommandError> {
    ctx.require(Scope::AppsRead)?;
    Ok(ports.oauth_requests().list_for_app(app, 50).await?)
}

// ---------- tokens ----------

/// What the token endpoint answers (RFC 6749 §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedTokens {
    pub access_token: NewToken,
    pub refresh_token: NewToken,
    pub expires_in: u64,
    pub scopes: Vec<Scope>,
    /// The app's signing secret, on its first installation's code exchange only.
    pub app_secret: Option<String>,
}

/// `grant_type=authorization_code`'s parameters.
pub struct CodeExchange {
    pub code: String,
    pub code_verifier: String,
    pub client_id: String,
    pub redirect_uri: String,
}

/// Exchanges a code (once, within a minute, with its PKCE verifier) for tokens. A
/// code used twice revokes the tokens its first exchange gave (RFC 6749 §4.1.2).
pub async fn exchange_code<S: OAuthPorts>(
    ports: &S,
    env: &impl OAuthEnv,
    input: CodeExchange,
) -> Result<IssuedTokens, TokenError> {
    let invalid = |why: &str| TokenError::InvalidGrant(why.to_string());
    if TokenKind::of(&input.code) != Some(TokenKind::AuthorizationCode) {
        return Err(invalid("the code is invalid"));
    }
    let mut code = ports
        .oauth_codes()
        .get(TokenHash::of(&input.code))
        .await?
        .ok_or_else(|| invalid("the code is invalid"))?;
    let now = env.now();
    if code.used_at().is_some() {
        ports
            .oauth_tokens()
            .revoke_family(code.family(), now)
            .await?;
        return Err(invalid("the code was used already"));
    }
    if code.is_expired(now) {
        return Err(invalid("the code has expired"));
    }
    if code.client_id() != input.client_id {
        return Err(invalid("the code was issued to another client"));
    }
    if code.redirect_uri() != input.redirect_uri {
        return Err(invalid("redirect_uri differs from the authorization's"));
    }
    if !pkce::verify(&input.code_verifier, code.code_challenge()) {
        return Err(invalid("code_verifier doesn't match the code_challenge"));
    }
    let mut app = installed_app(ports, code.app())
        .await?
        .ok_or_else(|| invalid("the app is not installed"))?;
    let scopes = within_grant(code.scopes(), &app);
    code.use_up(now);
    let (access, refresh, mut token) = new_pair(code.family(), &app, &scopes, env);
    let mut batch = ports.batch();
    batch.update_oauth_code(&mut code);
    batch.insert_oauth_token(&mut token);
    // The app gets its secret with these tokens: only now can it verify deliveries, so
    // `app.installed` is recorded here, not at approval.
    if code.first_install() {
        app.installed()
            .map_err(|_| invalid("the app is not installed"))?;
        batch.update_app(&mut app);
    }
    match ports.commit(batch).await {
        // Another exchange of the same code won.
        Err(RepoError::StaleVersion) => return Err(invalid("the code was used already")),
        written => written?,
    }
    let app_secret = if code.first_install() {
        Some(open_secret(env, &app)?)
    } else {
        None
    };
    Ok(IssuedTokens {
        access_token: access,
        refresh_token: refresh,
        expires_in: ACCESS_TOKEN_LIFETIME.as_secs(),
        scopes,
        app_secret,
    })
}

/// `grant_type=refresh_token`'s parameters.
pub struct Refresh {
    pub refresh_token: String,
    pub client_id: Option<String>,
    /// Space-separated, within the token's; all of them when absent.
    pub scope: Option<String>,
}

/// Replaces a refresh token and its access token with a new pair (rotation). One used
/// again after it was replaced was copied: every token of the app is revoked.
pub async fn refresh<S: OAuthPorts>(
    ports: &S,
    env: &impl OAuthEnv,
    input: Refresh,
) -> Result<IssuedTokens, TokenError> {
    let invalid = |why: &str| TokenError::InvalidGrant(why.to_string());
    if TokenKind::of(&input.refresh_token) != Some(TokenKind::RefreshToken) {
        return Err(invalid("the refresh token is invalid"));
    }
    let mut old = ports
        .oauth_tokens()
        .get_by_refresh(TokenHash::of(&input.refresh_token))
        .await?
        .ok_or_else(|| invalid("the refresh token is invalid"))?;
    let now = env.now();
    if old.revoked_at().is_some() {
        return Err(invalid("the refresh token was revoked"));
    }
    if old.rotated_at().is_some() {
        // Theft detection: the app's every token goes; it must be authorized again.
        ports.oauth_tokens().revoke_app(old.app(), now).await?;
        return Err(invalid(
            "the refresh token was used already: every token of the app is revoked",
        ));
    }
    if now >= old.refresh_expires_at() {
        return Err(invalid("the refresh token has expired"));
    }
    let app = installed_app(ports, old.app())
        .await?
        .ok_or_else(|| invalid("the app is not installed"))?;
    if let Some(client) = &input.client_id
        && app.manifest().is_some_and(|m| &m.url != client)
    {
        return Err(invalid("the refresh token was issued to another client"));
    }
    let mut scopes = within_grant(old.scopes(), &app);
    if let Some(scope) = &input.scope {
        let mut asked = Vec::new();
        for name in scope.split_whitespace() {
            match Scope::parse(name) {
                Some(s) if scopes.contains(&s) => asked.push(s),
                _ => {
                    return Err(TokenError::InvalidScope(format!(
                        "{name:?} is not among the token's scopes"
                    )));
                }
            }
        }
        scopes = normalize(&asked);
    }
    old.rotate(now);
    let (access, refresh, mut token) = new_pair(old.family(), &app, &scopes, env);
    let mut batch = ports.batch();
    batch.update_oauth_token(&mut old);
    batch.insert_oauth_token(&mut token);
    match ports.commit(batch).await {
        Err(RepoError::StaleVersion) => return Err(invalid("the refresh token was used already")),
        written => written?,
    }
    Ok(IssuedTokens {
        access_token: access,
        refresh_token: refresh,
        expires_in: ACCESS_TOKEN_LIFETIME.as_secs(),
        scopes,
        app_secret: None,
    })
}

/// Revokes a token (RFC 7009): an access or refresh token revokes its whole family.
/// Unknown tokens are fine (the answer is the same, so it tells nothing).
pub async fn revoke<S: HasOAuthTokens>(
    ports: &S,
    clock: &impl Clock,
    token: &str,
) -> Result<(), TokenError> {
    let hash = TokenHash::of(token);
    let found = match TokenKind::of(token) {
        Some(TokenKind::AccessToken) => ports.oauth_tokens().get_by_access(hash).await?,
        Some(TokenKind::RefreshToken) => ports.oauth_tokens().get_by_refresh(hash).await?,
        _ => None,
    };
    if let Some(token) = found {
        ports
            .oauth_tokens()
            .revoke_family(token.family(), clock.now())
            .await?;
    }
    Ok(())
}

/// The `Ctx` for a request carrying an app's access token: `Actor::App` with the
/// token's scopes as far as the app's grant still has them, read fresh on each request.
/// A disabled or uninstalled app's tokens are invalid.
pub async fn authenticate_access_token<S: HasOAuthTokens + hypha_apps::HasApps>(
    ports: &S,
    clock: &impl Clock,
    token: &str,
) -> Result<Ctx, CommandError> {
    if TokenKind::of(token) != Some(TokenKind::AccessToken) {
        return Err(CommandError::InvalidToken);
    }
    let found = ports
        .oauth_tokens()
        .get_by_access(TokenHash::of(token))
        .await?
        .filter(|t| t.access_valid(clock.now()))
        .ok_or(CommandError::InvalidToken)?;
    let app = installed_app(ports, found.app())
        .await?
        .ok_or(CommandError::InvalidToken)?;
    Ok(Ctx::new(
        Actor::App(app.id()),
        within_grant(found.scopes(), &app),
    ))
}

/// The app under `key` if its tokens may work: installed and enabled.
async fn installed_app<S: hypha_apps::HasApps>(
    ports: &S,
    key: &AppKey,
) -> Result<Option<App>, RepoError> {
    Ok(ports
        .apps()
        .get(key)
        .await?
        .filter(|a| a.is_installed() && a.enabled()))
}

/// `scopes` as far as `app`'s grant still has them.
fn within_grant(scopes: &[Scope], app: &App) -> Vec<Scope> {
    match app.grant() {
        Some(grant) => scopes.iter().filter(|s| grant.has(**s)).copied().collect(),
        None => Vec::new(),
    }
}

fn new_pair(
    family: Uuid,
    app: &App,
    scopes: &[Scope],
    env: &(impl Random + IdGenerator + Clock),
) -> (NewToken, NewToken, OAuthToken) {
    let access = NewToken::generate(TokenKind::AccessToken, env);
    let refresh = NewToken::generate(TokenKind::RefreshToken, env);
    let now = env.now();
    let token = OAuthToken::new(NewTokenPair {
        id: env.new_uuid(),
        family,
        app: app.key().clone(),
        scopes: scopes.to_vec(),
        access_hash: access.hash(),
        access_expires_at: now.plus(ACCESS_TOKEN_LIFETIME),
        refresh_hash: refresh.hash(),
        refresh_expires_at: now.plus(REFRESH_TOKEN_LIFETIME),
        created_at: now,
    });
    (access, refresh, token)
}

// ---------- grants and uninstalling ----------

async fn load_app<S: hypha_apps::HasApps>(ports: &S, key: &str) -> Result<App, CommandError> {
    let not_found = || CommandError::AppNotFound(key.to_string());
    let parsed = AppKey::new(key).map_err(|_| not_found())?;
    ports.apps().get(&parsed).await?.ok_or_else(not_found)
}

/// Narrows an app's grant to `scopes` (`PATCH /admin/apps/{key}/grant`), at once; the
/// app learns it from `app.grant_changed`. Adding scopes takes an authorization.
pub async fn narrow_grant<S: hypha_apps::HasApps>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    key: &str,
    scopes: &[Scope],
) -> Result<App, CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let mut app = load_app(ports, key).await?;
    if !app.is_installed() {
        return Err(hypha_apps::AppError::NotInstalled.into());
    }
    app.narrow_grant(scopes, ctx.actor(), clock.now())?;
    ports.apps().update(&mut app).await?;
    Ok(app)
}

/// What `uninstall` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uninstalled {
    /// Its tokens revoked and its grant gone; it's sent `app.uninstalled`, then waits
    /// to be deleted (a second `DELETE`) or installed again.
    Uninstalled,
    /// It had no grant (made by hand, or uninstalled already): removed for good, with
    /// its deliveries.
    Deleted,
}

/// `DELETE /admin/apps/{key}`: uninstalls an app installed through OAuth (every token
/// revoked, registrations removed, `app.uninstalled` sent as its last webhook);
/// deletes any other.
pub async fn uninstall<S: OAuthPorts>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    key: &str,
) -> Result<Uninstalled, CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let mut app = load_app(ports, key).await?;
    if !app.is_installed() {
        ports.apps().delete(app.key()).await?;
        return Ok(Uninstalled::Deleted);
    }
    let now = clock.now();
    app.uninstall(now)?;
    let mut batch = ports.batch();
    batch.update_app(&mut app);
    batch.revoke_app_tokens(app.key(), now);
    ports.commit(batch).await?;
    Ok(Uninstalled::Uninstalled)
}

/// What refreshing an app's manifest did.
#[derive(Debug, Clone, PartialEq)]
pub enum Refreshed {
    /// Its registrations (webhooks, extension points, UI) are the manifest's now; scopes
    /// it no longer asks for left the grant at once.
    Updated(Box<App>),
    /// As `Updated`, and the manifest asks for scopes the grant lacks, or drops custom
    /// fields (whose values would go with them): staff approve on the consent page for
    /// this request (the grant, and the app's fields, stay until then).
    ConsentRequired(Box<App>, Box<AuthorizationRequest>),
}

/// `prepare_refresh`, committed with nothing else in the batch and no custom fields
/// considered: for callers with nothing to add (this crate's tests). Hosts use
/// `prepare_refresh`.
pub async fn refresh_manifest<S: OAuthPorts>(
    ctx: &Ctx,
    ports: &S,
    fetcher: &impl ManifestFetcher,
    env: &impl OAuthEnv,
    key: &str,
) -> Result<Refreshed, CommandError> {
    let prepared = prepare_refresh(ctx, ports, fetcher, env, key, |_| false).await?;
    ports.commit(prepared.batch).await?;
    Ok(prepared.answer)
}

/// Fetches an installed app's manifest again from its URL and applies it (`POST
/// /admin/apps/{key}/refresh`). Staff with `apps:write` do it, or the app itself with its
/// own token. New scopes aren't granted here: they make a pending authorization request (no
/// code: approving changes the grant, and the app then asks `/oauth/authorize` for tokens
/// with them, approved at once).
///
/// `drops_data` says whether applying a manifest deletes data staff should see go first:
/// the HTTP layer answers whether it removes custom fields. Such a refresh waits for
/// consent like new scopes do. Staged, not committed (`Prepared`): `applies` is the
/// manifest when the app's fields change now (an `Updated` refresh), `None` when they wait
/// for the approval.
pub async fn prepare_refresh<S: OAuthPorts>(
    ctx: &Ctx,
    ports: &S,
    fetcher: &impl ManifestFetcher,
    env: &impl OAuthEnv,
    key: &str,
    drops_data: impl Fn(&AppManifest) -> bool,
) -> Result<Prepared<S::Batch, Refreshed>, CommandError> {
    // An app may refresh itself without `apps:write`; anyone else needs it.
    let is_app = matches!(ctx.actor(), Actor::App(_));
    if !is_app {
        ctx.require(Scope::AppsWrite)?;
    }
    let mut app = load_app(ports, key).await?;
    if is_app && ctx.actor() != Actor::App(app.id()) {
        ctx.require(Scope::AppsWrite)?;
    }
    // A spore has no manifest: its registrations are the build's.
    app.require_external()?;
    let (Some(installed), true) = (app.manifest().cloned(), app.is_installed()) else {
        return Err(hypha_apps::AppError::NotInstalled.into());
    };
    // Nothing to fetch: a private app changes by uploading its manifest again.
    if is_private_client(&installed.url) {
        return Err(CommandError::PrivateApp(app.key().clone()));
    }
    let manifest = fetch_manifest(fetcher, &installed.url).await?;
    let grant = app
        .grant()
        .cloned()
        .ok_or(hypha_apps::AppError::NotInstalled)?;
    let now = env.now();
    let asked = manifest.scopes().to_vec();
    let added: Vec<Scope> = asked
        .iter()
        .filter(|s| !grant.scopes().contains(s))
        .copied()
        .collect();
    let kept: Vec<Scope> = grant
        .scopes()
        .iter()
        .filter(|s| asked.contains(s))
        .copied()
        .collect();
    // Refuses a manifest that changes a field's type.
    app.apply_manifest(InstalledManifest {
        url: installed.url.clone(),
        manifest: manifest.clone(),
        fetched_at: now,
    })?;
    // Fewer scopes take effect at once; the approver stays who approved them.
    if kept.len() != grant.scopes().len() {
        app.set_grant(Grant::new(&kept, grant.approved_by(), grant.approved_at()));
    }
    let mut batch = ports.batch();
    if added.is_empty() && !drops_data(&manifest) {
        batch.update_app(&mut app);
        return Ok(Prepared {
            batch,
            answer: Refreshed::Updated(Box::new(app)),
            applies: Some(manifest),
        });
    }
    let redirect_uri = manifest
        .redirect_uris()
        .first()
        .map(|u| u.as_str().to_string())
        .unwrap_or_default();
    let mut request = AuthorizationRequest::new(NewRequest {
        id: OAuthRequestId::from_uuid(env.new_uuid()),
        client_id: installed.url,
        manifest,
        redirect_uri,
        state: None,
        code_challenge: None,
        scopes: asked,
        previous_scopes: Some(kept),
        created_at: now,
        expires_at: now.plus(REQUEST_LIFETIME),
    });
    batch.update_app(&mut app);
    batch.insert_oauth_request(&mut request);
    Ok(Prepared {
        batch,
        answer: Refreshed::ConsentRequired(Box::new(app), Box::new(request)),
        applies: None,
    })
}

/// Whether `request` is still waiting for an answer at `now`.
pub fn is_pending(request: &AuthorizationRequest, now: Timestamp) -> bool {
    request.status() == RequestStatus::Pending && !request.is_expired(now)
}

/// The manifest scopes grouped by area (`orders`, `customers`, …) for consent pages,
/// personal-data scopes apart.
pub fn group_scopes(scopes: &[Scope]) -> (Vec<(&'static str, Vec<Scope>)>, Vec<Scope>) {
    let mut groups: Vec<(&'static str, Vec<Scope>)> = Vec::new();
    let mut personal = Vec::new();
    for scope in scopes {
        if scope.is_personal_data() {
            personal.push(*scope);
            continue;
        }
        let area = scope.as_str().split(':').next().unwrap_or("");
        match groups.iter_mut().find(|(a, _)| *a == area) {
            Some((_, list)) => list.push(*scope),
            None => groups.push((area, vec![*scope])),
        }
    }
    (groups, personal)
}

/// The manifest's reasons for `scopes` (only the ones it gives).
pub fn reasons(manifest: &AppManifest, scopes: &[Scope]) -> Vec<(Scope, String)> {
    scopes
        .iter()
        .filter_map(|s| manifest.reason(*s).map(|r| (*s, r.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diff_lists_what_is_added_removed_and_kept() {
        let diff = scope_diff(
            &[Scope::OrdersRead, Scope::ProductsRead],
            &[Scope::OrdersRead, Scope::CustomerDataEmail],
        );
        assert_eq!(diff.added, vec![Scope::CustomerDataEmail]);
        assert_eq!(diff.removed, vec![Scope::ProductsRead]);
        assert_eq!(diff.kept, vec![Scope::OrdersRead]);
    }

    #[test]
    fn personal_data_scopes_are_grouped_apart() {
        let (groups, personal) = group_scopes(&[
            Scope::OrdersRead,
            Scope::OrdersWrite,
            Scope::PaymentsWrite,
            Scope::CustomerDataEmail,
        ]);
        assert_eq!(
            groups,
            vec![
                ("orders", vec![Scope::OrdersRead, Scope::OrdersWrite]),
                ("payments", vec![Scope::PaymentsWrite]),
            ]
        );
        assert_eq!(personal, vec![Scope::CustomerDataEmail]);
    }
}
