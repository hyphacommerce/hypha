//! How apps are installed and act (design note 0029, AP5, §4.1): Hypha's API is an
//! OAuth 2.1 authorization server. An app's client ID is its manifest's URL; a staff
//! member approves the scopes it asks for on the consent page of whichever admin the
//! store uses; the app exchanges the code (with PKCE) for an hour-long access token
//! (`hypha_at_…`) and a rotating refresh token (`hypha_rt_…`), bound to the grant.
//!
//! A composing domain: `hypha-apps` (the app, its manifest and grant) and `hypha-auth`
//! (tokens stored as SHA-256), over core.

pub mod code;
pub mod commands;
pub mod error;
pub mod pkce;
pub mod repo;
pub mod request;
pub mod token;

pub use code::{AuthorizationCode, CodeSnapshot, NewCode};
pub use commands::{
    ACCESS_TOKEN_LIFETIME, Approval, AuthorizeInput, Authorized, CODE_LIFETIME, CodeExchange,
    IssuedTokens, OAuthEnv, Prepared, REFRESH_TOKEN_LIFETIME, REQUEST_LIFETIME, Refresh, Refreshed,
    ScopeDiff, Uninstalled, approve, authenticate_access_token, authorize, deny, exchange_code,
    get_request, group_scopes, is_pending, narrow_grant, prepare_approval, prepare_refresh,
    reasons, refresh, refresh_manifest, request_history, request_private_app, revoke, scope_diff,
    uninstall,
};
pub use error::{AuthorizeError, CommandError, TokenError, UnredirectableError};
pub use repo::{
    HasOAuthCodes, HasOAuthRequests, HasOAuthTokens, OAuthCodeRepo, OAuthPorts, OAuthRequestRepo,
    OAuthTokenRepo, StageOAuth, keys,
};
pub use request::{AuthorizationRequest, NewRequest, RequestSnapshot, RequestStatus};
pub use token::{NewTokenPair, OAuthToken, TokenSnapshot};
