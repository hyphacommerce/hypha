use hypha_core::{ApiKeyId, IdentityId, Page, PageRequest, RepoError, SessionId, Timestamp};

use crate::{
    ApiKey, Identity, PasswordReset, Session,
    identity::{CredentialKind, Principal, PrincipalKind},
    token::TokenHash,
};

/// Uniqueness rules the repos enforce, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const IDENTITY_ID: &str = "identity_id";
    /// One identity per principal.
    pub const IDENTITY_PRINCIPAL: &str = "identity_principal";
    /// A credential's lookup (a password's email) is unique per principal kind and
    /// credential kind.
    pub const IDENTITY_LOGIN: &str = "identity_login";
    pub const SESSION_ID: &str = "session_id";
    pub const SESSION_TOKEN: &str = "session_token";
    pub const API_KEY_ID: &str = "api_key_id";
    pub const API_KEY_TOKEN: &str = "api_key_token";
    pub const PASSWORD_RESET_ID: &str = "password_reset_id";
    pub const PASSWORD_RESET_TOKEN: &str = "password_reset_token";
}

#[allow(async_fn_in_trait)]
pub trait IdentityRepo {
    async fn get(&self, id: IdentityId) -> Result<Option<Identity>, RepoError>;
    async fn get_by_principal(&self, principal: Principal) -> Result<Option<Identity>, RepoError>;
    /// Whether `principal` has an identity: its ID alone, one lookup without the
    /// credentials (an account's existence).
    async fn find_principal(&self, principal: Principal) -> Result<Option<IdentityId>, RepoError>;
    /// The identity of `principal` kind with a `credential` whose lookup is `lookup`
    /// (for a password, the lowercased email).
    async fn find_login(
        &self,
        principal: PrincipalKind,
        credential: CredentialKind,
        lookup: &str,
    ) -> Result<Option<Identity>, RepoError>;
    /// Enforces `IDENTITY_ID`, `IDENTITY_PRINCIPAL` and `IDENTITY_LOGIN`.
    async fn insert(&self, identity: &mut Identity) -> Result<(), RepoError>;
    /// Replaces the credentials. Enforces `IDENTITY_LOGIN`.
    async fn update(&self, identity: &mut Identity) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait SessionRepo {
    async fn get(&self, id: SessionId) -> Result<Option<Session>, RepoError>;
    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<Session>, RepoError>;
    /// Enforces `SESSION_ID` and `SESSION_TOKEN`.
    async fn insert(&self, session: &mut Session) -> Result<(), RepoError>;
    /// Enforces `SESSION_TOKEN` (activation swaps the token). The client stays as
    /// inserted, and `last_used_at` never moves back: a copy loaded before a recorded
    /// use keeps the later time.
    async fn update(&self, session: &mut Session) -> Result<(), RepoError>;
    /// Every session of `identity` neither revoked nor expired at `now`, pending ones
    /// too, newest first: what its owner lists (a handful).
    async fn live_for_identity(
        &self,
        identity: IdentityId,
        now: Timestamp,
    ) -> Result<Vec<Session>, RepoError>;
    /// Sets the session's `last_used_at` to `at` if that is later, without moving its
    /// version: a use isn't a change, so it never makes a concurrent update stale. A
    /// missing session is no error (it may have just been pruned).
    async fn record_use(&self, id: SessionId, at: Timestamp) -> Result<(), RepoError>;
    /// Deletes up to `limit` sessions that ended (expired or revoked) before `before`;
    /// returns how many.
    async fn prune(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait ApiKeyRepo {
    async fn get(&self, id: ApiKeyId) -> Result<Option<ApiKey>, RepoError>;
    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<ApiKey>, RepoError>;
    /// A page of keys, revoked ones too, newest first.
    async fn list(&self, page: PageRequest<ApiKeyId>) -> Result<Page<ApiKey>, RepoError>;
    /// Enforces `API_KEY_ID` and `API_KEY_TOKEN`.
    async fn insert(&self, key: &mut ApiKey) -> Result<(), RepoError>;
    async fn update(&self, key: &mut ApiKey) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait PasswordResetRepo {
    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<PasswordReset>, RepoError>;
    /// The identity's most recently created reset, used or not.
    async fn latest_for_identity(
        &self,
        identity: IdentityId,
    ) -> Result<Option<PasswordReset>, RepoError>;
    /// Enforces `PASSWORD_RESET_ID` and `PASSWORD_RESET_TOKEN`.
    async fn insert(&self, reset: &mut PasswordReset) -> Result<(), RepoError>;
}

/// Stages identity writes in a `hypha_core::HasBatch` batch.
pub trait StageIdentities {
    /// What `IdentityRepo::insert` would write; same keys on commit.
    fn insert_identity(&mut self, identity: &mut Identity);
    /// What `IdentityRepo::update` would write; `StaleVersion` on commit if it moved.
    fn update_identity(&mut self, identity: &mut Identity);
    /// Removes the identity with its credentials at the version it was loaded at (a
    /// customer erased or deleted); its sessions are revoked apart
    /// (`StageSessions::revoke_sessions`). `StaleVersion` on commit if it moved or is gone.
    fn delete_identity(&mut self, identity: &Identity);
}

/// Stages session writes in a `hypha_core::HasBatch` batch.
pub trait StageSessions {
    fn insert_session(&mut self, session: &mut Session);
    fn update_session(&mut self, session: &mut Session);
    /// Revokes every live session of `identity` except `keep` at `at`, bumping their
    /// versions (a password change or reset). Sessions already revoked keep their time.
    fn revoke_sessions(&mut self, identity: IdentityId, keep: Option<SessionId>, at: Timestamp);
}

/// Stages password reset writes in a `hypha_core::HasBatch` batch.
pub trait StagePasswordResets {
    /// What `PasswordResetRepo::insert` would write; same keys on commit.
    fn insert_password_reset(&mut self, reset: &mut PasswordReset);
    fn update_password_reset(&mut self, reset: &mut PasswordReset);
    /// Marks every unused reset of `identity` as used at `at`, bumping their versions:
    /// once the password changed, no older reset link may change it again. Runs after
    /// the batch's single updates, so a reset staged with `update_password_reset` keeps
    /// its own time.
    fn use_up_password_resets(&mut self, identity: IdentityId, at: Timestamp);
}

/// Accessors, like `hypha_customers::HasCustomers`: commands bind the ones they use.
pub trait HasIdentities {
    type Identities: IdentityRepo;
    fn identities(&self) -> &Self::Identities;
}

pub trait HasSessions {
    type Sessions: SessionRepo;
    fn sessions(&self) -> &Self::Sessions;
}

pub trait HasApiKeys {
    type ApiKeys: ApiKeyRepo;
    fn api_keys(&self) -> &Self::ApiKeys;
}

pub trait HasPasswordResets {
    type PasswordResets: PasswordResetRepo;
    fn password_resets(&self) -> &Self::PasswordResets;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasIdentities + ?Sized> HasIdentities for &T {
    type Identities = T::Identities;
    fn identities(&self) -> &Self::Identities {
        (**self).identities()
    }
}

impl<T: HasSessions + ?Sized> HasSessions for &T {
    type Sessions = T::Sessions;
    fn sessions(&self) -> &Self::Sessions {
        (**self).sessions()
    }
}

impl<T: HasApiKeys + ?Sized> HasApiKeys for &T {
    type ApiKeys = T::ApiKeys;
    fn api_keys(&self) -> &Self::ApiKeys {
        (**self).api_keys()
    }
}

impl<T: HasPasswordResets + ?Sized> HasPasswordResets for &T {
    type PasswordResets = T::PasswordResets;
    fn password_resets(&self) -> &Self::PasswordResets {
        (**self).password_resets()
    }
}
