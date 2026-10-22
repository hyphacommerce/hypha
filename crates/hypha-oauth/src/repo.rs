use hypha_apps::{HasApps, StageApps};
use hypha_auth::TokenHash;
use hypha_core::{AgentId, AppKey, HasBatch, OAuthRequestId, RepoError, Timestamp};
use uuid::Uuid;

use crate::{AuthorizationCode, AuthorizationRequest, OAuthToken};

/// Uniqueness rules the repos enforce, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const OAUTH_REQUEST_ID: &str = "oauth_request_id";
    pub const OAUTH_CODE: &str = "oauth_code";
    pub const OAUTH_TOKEN_ID: &str = "oauth_token_id";
    pub const OAUTH_ACCESS_TOKEN: &str = "oauth_access_token";
    pub const OAUTH_REFRESH_TOKEN: &str = "oauth_refresh_token";
}

#[allow(async_fn_in_trait)]
pub trait OAuthRequestRepo {
    async fn get(&self, id: OAuthRequestId) -> Result<Option<AuthorizationRequest>, RepoError>;
    /// An app's requests, newest first, at most `limit`: its history. MCP clients'
    /// requests belong to no app.
    async fn list_for_app(
        &self,
        app: &AppKey,
        limit: u32,
    ) -> Result<Vec<AuthorizationRequest>, RepoError>;
    /// Enforces `OAUTH_REQUEST_ID`.
    async fn insert(&self, request: &mut AuthorizationRequest) -> Result<(), RepoError>;
    /// `StaleVersion` if it changed since it was loaded.
    async fn update(&self, request: &mut AuthorizationRequest) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait OAuthCodeRepo {
    async fn get(&self, hash: TokenHash) -> Result<Option<AuthorizationCode>, RepoError>;
    /// Enforces `OAUTH_CODE`. The code's request must exist (a `Reference` otherwise).
    async fn insert(&self, code: &mut AuthorizationCode) -> Result<(), RepoError>;
    /// `StaleVersion` if it changed since it was loaded: one of two racing exchanges
    /// loses.
    async fn update(&self, code: &mut AuthorizationCode) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait OAuthTokenRepo {
    async fn get_by_access(&self, hash: TokenHash) -> Result<Option<OAuthToken>, RepoError>;
    async fn get_by_refresh(&self, hash: TokenHash) -> Result<Option<OAuthToken>, RepoError>;
    /// Enforces `OAUTH_TOKEN_ID`, `OAUTH_ACCESS_TOKEN` and `OAUTH_REFRESH_TOKEN`. The
    /// token's app or agent must exist (a `Reference` otherwise); deleting the app deletes
    /// them.
    async fn insert(&self, token: &mut OAuthToken) -> Result<(), RepoError>;
    async fn update(&self, token: &mut OAuthToken) -> Result<(), RepoError>;
    /// Revokes every token of `family` not revoked yet at `at`, bumping their versions;
    /// returns how many.
    async fn revoke_family(&self, family: Uuid, at: Timestamp) -> Result<u32, RepoError>;
    /// Revokes every token of `app` not revoked yet at `at`; returns how many.
    async fn revoke_app(&self, app: &AppKey, at: Timestamp) -> Result<u32, RepoError>;
    /// Revokes every token of `agent` not revoked yet at `at`; returns how many.
    async fn revoke_agent(&self, agent: AgentId, at: Timestamp) -> Result<u32, RepoError>;
}

/// Stages OAuth writes in a `hypha_core::HasBatch` batch, beside the app's
/// (`hypha_apps::StageApps`).
pub trait StageOAuth {
    fn insert_oauth_request(&mut self, request: &mut AuthorizationRequest);
    fn update_oauth_request(&mut self, request: &mut AuthorizationRequest);
    /// The code's request is staged or stored already.
    fn insert_oauth_code(&mut self, code: &mut AuthorizationCode);
    fn update_oauth_code(&mut self, code: &mut AuthorizationCode);
    /// The token's app is staged or stored already.
    fn insert_oauth_token(&mut self, token: &mut OAuthToken);
    fn update_oauth_token(&mut self, token: &mut OAuthToken);
    /// `OAuthTokenRepo::revoke_app` in the batch, after its single writes.
    fn revoke_app_tokens(&mut self, app: &AppKey, at: Timestamp);
}

pub trait HasOAuthRequests {
    type OAuthRequests: OAuthRequestRepo;
    fn oauth_requests(&self) -> &Self::OAuthRequests;
}

pub trait HasOAuthCodes {
    type OAuthCodes: OAuthCodeRepo;
    fn oauth_codes(&self) -> &Self::OAuthCodes;
}

pub trait HasOAuthTokens {
    type OAuthTokens: OAuthTokenRepo;
    fn oauth_tokens(&self) -> &Self::OAuthTokens;
}

impl<T: HasOAuthRequests + ?Sized> HasOAuthRequests for &T {
    type OAuthRequests = T::OAuthRequests;
    fn oauth_requests(&self) -> &Self::OAuthRequests {
        (**self).oauth_requests()
    }
}

impl<T: HasOAuthCodes + ?Sized> HasOAuthCodes for &T {
    type OAuthCodes = T::OAuthCodes;
    fn oauth_codes(&self) -> &Self::OAuthCodes {
        (**self).oauth_codes()
    }
}

impl<T: HasOAuthTokens + ?Sized> HasOAuthTokens for &T {
    type OAuthTokens = T::OAuthTokens;
    fn oauth_tokens(&self) -> &Self::OAuthTokens {
        (**self).oauth_tokens()
    }
}

/// Everything the OAuth commands use, as one bound (like `hypha_cart::CartPorts`).
pub trait OAuthPorts:
    HasApps
    + HasOAuthRequests
    + HasOAuthCodes
    + HasOAuthTokens
    + HasBatch<Batch: StageApps + StageOAuth>
{
}

impl<T> OAuthPorts for T where
    T: HasApps
        + HasOAuthRequests
        + HasOAuthCodes
        + HasOAuthTokens
        + HasBatch<Batch: StageApps + StageOAuth>
        + ?Sized
{
}
