//! In-memory repos for `hypha-auth`.

use hypha_auth::{
    ApiKey, ApiKeyRepo, CredentialKind, Identity, IdentityRepo, PasswordReset, PasswordResetRepo,
    Principal, PrincipalKind, Session, SessionRepo, TokenHash, keys,
};
use hypha_core::{ApiKeyId, IdentityId, PasswordResetId, RepoError, SessionId};

use crate::table::Table;

#[derive(Default)]
pub struct InMemoryIdentityRepo {
    pub(crate) identities: Table<IdentityId, Identity>,
}

/// One identity per principal, and a login (principal kind, credential kind, lookup)
/// on one identity only.
pub(crate) fn unique_identity(new: &Identity, other: &Identity) -> Option<&'static str> {
    if new.principal() == other.principal() {
        return Some(keys::IDENTITY_PRINCIPAL);
    }
    let same_login = new.principal().kind() == other.principal().kind()
        && new.credentials().iter().any(|c| {
            c.lookup().is_some()
                && other
                    .credentials()
                    .iter()
                    .any(|o| o.kind() == c.kind() && o.lookup() == c.lookup())
        });
    same_login.then_some(keys::IDENTITY_LOGIN)
}

impl IdentityRepo for InMemoryIdentityRepo {
    async fn get(&self, id: IdentityId) -> Result<Option<Identity>, RepoError> {
        Ok(self.identities.get(&id))
    }

    async fn get_by_principal(&self, principal: Principal) -> Result<Option<Identity>, RepoError> {
        Ok(self.identities.find(|i| i.principal() == principal))
    }

    async fn find_login(
        &self,
        principal: PrincipalKind,
        credential: CredentialKind,
        lookup: &str,
    ) -> Result<Option<Identity>, RepoError> {
        Ok(self.identities.find(|i| {
            i.principal().kind() == principal
                && i.credentials()
                    .iter()
                    .any(|c| c.kind() == credential && c.lookup() == Some(lookup))
        }))
    }

    async fn insert(&self, identity: &mut Identity) -> Result<(), RepoError> {
        self.identities
            .insert(identity.id(), identity, keys::IDENTITY_ID, &unique_identity)
    }

    async fn update(&self, identity: &mut Identity) -> Result<(), RepoError> {
        self.identities
            .update(identity.id(), identity, &unique_identity)
    }
}

#[derive(Default)]
pub struct InMemorySessionRepo {
    pub(crate) sessions: Table<SessionId, Session>,
}

pub(crate) fn unique_session(new: &Session, other: &Session) -> Option<&'static str> {
    (new.token_hash() == other.token_hash()).then_some(keys::SESSION_TOKEN)
}

impl SessionRepo for InMemorySessionRepo {
    async fn get(&self, id: SessionId) -> Result<Option<Session>, RepoError> {
        Ok(self.sessions.get(&id))
    }

    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<Session>, RepoError> {
        Ok(self.sessions.find(|s| s.token_hash() == hash))
    }

    async fn insert(&self, session: &mut Session) -> Result<(), RepoError> {
        self.sessions
            .insert(session.id(), session, keys::SESSION_ID, &unique_session)
    }

    async fn update(&self, session: &mut Session) -> Result<(), RepoError> {
        self.sessions.update(session.id(), session, &unique_session)
    }
}

#[derive(Default)]
pub struct InMemoryApiKeyRepo {
    keys: Table<ApiKeyId, ApiKey>,
}

fn unique_key(new: &ApiKey, other: &ApiKey) -> Option<&'static str> {
    (new.token_hash() == other.token_hash()).then_some(keys::API_KEY_TOKEN)
}

impl ApiKeyRepo for InMemoryApiKeyRepo {
    async fn get(&self, id: ApiKeyId) -> Result<Option<ApiKey>, RepoError> {
        Ok(self.keys.get(&id))
    }

    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<ApiKey>, RepoError> {
        Ok(self.keys.find(|k| k.token_hash() == hash))
    }

    async fn insert(&self, key: &mut ApiKey) -> Result<(), RepoError> {
        self.keys
            .insert(key.id(), key, keys::API_KEY_ID, &unique_key)
    }

    async fn update(&self, key: &mut ApiKey) -> Result<(), RepoError> {
        self.keys.update(key.id(), key, &unique_key)
    }
}

#[derive(Default)]
pub struct InMemoryPasswordResetRepo {
    pub(crate) resets: Table<PasswordResetId, PasswordReset>,
}

pub(crate) fn unique_reset(new: &PasswordReset, other: &PasswordReset) -> Option<&'static str> {
    (new.token_hash() == other.token_hash()).then_some(keys::PASSWORD_RESET_TOKEN)
}

impl PasswordResetRepo for InMemoryPasswordResetRepo {
    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<PasswordReset>, RepoError> {
        Ok(self.resets.find(|r| r.token_hash() == hash))
    }

    async fn latest_for_identity(
        &self,
        identity: IdentityId,
    ) -> Result<Option<PasswordReset>, RepoError> {
        Ok(self
            .resets
            .all()
            .into_iter()
            .filter(|r| r.identity() == identity)
            .max_by_key(|r| (r.created_at(), r.id().as_uuid())))
    }

    async fn insert(&self, reset: &mut PasswordReset) -> Result<(), RepoError> {
        self.resets
            .insert(reset.id(), reset, keys::PASSWORD_RESET_ID, &unique_reset)
    }
}
