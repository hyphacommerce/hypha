use hypha_apps::{AppError, FetchError};
use hypha_core::{AppKey, AuthError, ErrorCode, OAuthRequestId, RepoError, Scope, SealError};
use thiserror::Error;

/// The admin commands' errors (the consent page, grants, uninstalling) and access
/// token authentication's.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    App(#[from] AppError),
    #[error(transparent)]
    Seal(#[from] SealError),
    /// Refreshing a manifest that can't be read or isn't valid.
    #[error(transparent)]
    Manifest(#[from] FetchError),
    /// The key as the caller sent it (keys only arrive in URLs here).
    #[error("app {0:?} not found")]
    AppNotFound(String),
    #[error("authorization request {0} not found")]
    RequestNotFound(OAuthRequestId),
    /// Ten minutes passed: the app starts again at `/oauth/authorize`.
    #[error("authorization request {0} has expired")]
    RequestExpired(OAuthRequestId),
    #[error("authorization request {0} was answered already")]
    RequestDecided(OAuthRequestId),
    /// Approving scopes the staff member doesn't hold (as role grants require, 0008).
    #[error("you can't grant {0}, which you don't have")]
    CannotGrant(Scope),
    /// A malformed, unknown, expired or revoked access token, or one of an app that is
    /// disabled or no longer installed: one answer for all.
    #[error("the token is invalid or expired")]
    InvalidToken,
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::App(e) => e.code(),
            CommandError::Seal(_) => "internal_error",
            CommandError::Manifest(e) => e.code(),
            CommandError::AppNotFound(_) => "app_not_found",
            CommandError::RequestNotFound(_) => "oauth_request_not_found",
            CommandError::RequestExpired(_) => "oauth_request_expired",
            CommandError::RequestDecided(_) => "oauth_request_decided",
            CommandError::CannotGrant(_) => "not_allowed_to_grant",
            CommandError::InvalidToken => "invalid_token",
        }
    }
}

/// Why `/oauth/authorize` can't send the browser back to the app: the client or the
/// redirect URI can't be trusted (RFC 6749 §4.1.2.1), so the answer is a page, never
/// a redirect.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum UnredirectableError {
    #[error("client_id is required: the URL of the app's manifest")]
    MissingClientId,
    #[error(transparent)]
    Manifest(#[from] FetchError),
    #[error("redirect_uri is required")]
    MissingRedirectUri,
    #[error("redirect_uri {0:?} is not one the app's manifest lists")]
    RedirectUriNotListed(String),
    /// The key is taken by an app installed from another manifest URL.
    #[error("app {0} is installed from another manifest URL")]
    OtherManifest(AppKey),
}

impl ErrorCode for UnredirectableError {
    fn code(&self) -> &'static str {
        match self {
            UnredirectableError::MissingClientId => "invalid_client",
            UnredirectableError::Manifest(e) => e.code(),
            UnredirectableError::MissingRedirectUri => "invalid_redirect_uri",
            UnredirectableError::RedirectUriNotListed(_) => "invalid_redirect_uri",
            UnredirectableError::OtherManifest(_) => "invalid_client",
        }
    }
}

/// `/oauth/authorize`'s failures.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum AuthorizeError {
    #[error(transparent)]
    Unredirectable(#[from] UnredirectableError),
    /// Sent back to the app (RFC 6749 §4.1.2.1): `location` carries `error`,
    /// `error_description` and `state`.
    #[error("{error}: {description}")]
    Redirect {
        location: String,
        error: &'static str,
        description: String,
    },
    #[error(transparent)]
    Repo(#[from] RepoError),
}

impl ErrorCode for AuthorizeError {
    fn code(&self) -> &'static str {
        match self {
            AuthorizeError::Unredirectable(e) => e.code(),
            AuthorizeError::Redirect { error, .. } => error,
            AuthorizeError::Repo(e) => e.code(),
        }
    }
}

/// The token and revocation endpoints' errors, as RFC 6749 §5.2 names them: the
/// endpoints answer `{"error", "error_description"}`.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TokenError {
    /// A parameter missing, repeated or malformed.
    #[error("{0}")]
    InvalidRequest(String),
    #[error("{0}")]
    InvalidClient(String),
    /// The code or refresh token is unknown, expired, spent, revoked, or for another
    /// client or redirect URI, or the verifier doesn't match.
    #[error("{0}")]
    InvalidGrant(String),
    #[error("grant_type {0:?} is not supported: use authorization_code or refresh_token")]
    UnsupportedGrantType(String),
    #[error("{0}")]
    InvalidScope(String),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Seal(#[from] SealError),
}

impl ErrorCode for TokenError {
    fn code(&self) -> &'static str {
        match self {
            TokenError::InvalidRequest(_) => "invalid_request",
            TokenError::InvalidClient(_) => "invalid_client",
            TokenError::InvalidGrant(_) => "invalid_grant",
            TokenError::UnsupportedGrantType(_) => "unsupported_grant_type",
            TokenError::InvalidScope(_) => "invalid_scope",
            TokenError::Repo(_) | TokenError::Seal(_) => "server_error",
        }
    }
}
