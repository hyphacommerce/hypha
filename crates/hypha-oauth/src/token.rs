//! An app's tokens: an access token (`hypha_at_…`, an hour) and the refresh token issued
//! with it (`hypha_rt_…`, 30 days), stored together as SHA-256 hashes. Refreshing replaces
//! the pair: the old refresh token is marked rotated, and the new pair joins its *family*
//! (every pair since one code exchange). A rotated refresh token used again means it was
//! copied: the app's tokens are all revoked (theft detection, OAuth 2.1 §4.3.1).

use hypha_apps::normalize;
use hypha_auth::TokenHash;
use hypha_core::{AppKey, Scope, Timestamp, Version, Versioned};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthToken {
    id: Uuid,
    version: Version,
    family: Uuid,
    app: AppKey,
    scopes: Vec<Scope>,
    access_hash: TokenHash,
    access_expires_at: Timestamp,
    refresh_hash: TokenHash,
    refresh_expires_at: Timestamp,
    created_at: Timestamp,
    /// When its refresh token was spent on the next pair.
    rotated_at: Option<Timestamp>,
    revoked_at: Option<Timestamp>,
}

impl Versioned for OAuthToken {
    fn version(&self) -> Version {
        self.version
    }
    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

/// A new pair's parts.
pub struct NewTokenPair {
    pub id: Uuid,
    pub family: Uuid,
    pub app: AppKey,
    pub scopes: Vec<Scope>,
    pub access_hash: TokenHash,
    pub access_expires_at: Timestamp,
    pub refresh_hash: TokenHash,
    pub refresh_expires_at: Timestamp,
    pub created_at: Timestamp,
}

impl OAuthToken {
    pub fn new(new: NewTokenPair) -> Self {
        Self {
            id: new.id,
            version: Version::NEW,
            family: new.family,
            app: new.app,
            scopes: normalize(&new.scopes),
            access_hash: new.access_hash,
            access_expires_at: new.access_expires_at,
            refresh_hash: new.refresh_hash,
            refresh_expires_at: new.refresh_expires_at,
            created_at: new.created_at,
            rotated_at: None,
            revoked_at: None,
        }
    }

    /// Whether the access token works at `now`.
    pub fn access_valid(&self, now: Timestamp) -> bool {
        self.revoked_at.is_none() && now < self.access_expires_at
    }

    /// Spends the refresh token on the next pair; the access token stays until it
    /// expires.
    pub fn rotate(&mut self, at: Timestamp) {
        self.rotated_at = Some(at);
    }

    pub fn revoke(&mut self, at: Timestamp) {
        self.revoked_at.get_or_insert(at);
    }

    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn family(&self) -> Uuid {
        self.family
    }
    pub fn app(&self) -> &AppKey {
        &self.app
    }
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }
    pub fn access_hash(&self) -> TokenHash {
        self.access_hash
    }
    pub fn access_expires_at(&self) -> Timestamp {
        self.access_expires_at
    }
    pub fn refresh_hash(&self) -> TokenHash {
        self.refresh_hash
    }
    pub fn refresh_expires_at(&self) -> Timestamp {
        self.refresh_expires_at
    }
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
    pub fn rotated_at(&self) -> Option<Timestamp> {
        self.rotated_at
    }
    pub fn revoked_at(&self) -> Option<Timestamp> {
        self.revoked_at
    }
}

/// Plain data for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSnapshot {
    pub id: Uuid,
    pub version: Version,
    pub family: Uuid,
    pub app: String,
    pub scopes: Vec<Scope>,
    pub access_hash: TokenHash,
    pub access_expires_at: Timestamp,
    pub refresh_hash: TokenHash,
    pub refresh_expires_at: Timestamp,
    pub created_at: Timestamp,
    pub rotated_at: Option<Timestamp>,
    pub revoked_at: Option<Timestamp>,
}

impl OAuthToken {
    pub fn snapshot(&self) -> TokenSnapshot {
        TokenSnapshot {
            id: self.id,
            version: self.version,
            family: self.family,
            app: self.app.as_str().to_string(),
            scopes: self.scopes.clone(),
            access_hash: self.access_hash,
            access_expires_at: self.access_expires_at,
            refresh_hash: self.refresh_hash,
            refresh_expires_at: self.refresh_expires_at,
            created_at: self.created_at,
            rotated_at: self.rotated_at,
            revoked_at: self.revoked_at,
        }
    }

    pub fn restore(s: TokenSnapshot) -> Result<Self, String> {
        let app = AppKey::new(&s.app).map_err(|e| e.to_string())?;
        let mut token = Self::new(NewTokenPair {
            id: s.id,
            family: s.family,
            app,
            scopes: s.scopes,
            access_hash: s.access_hash,
            access_expires_at: s.access_expires_at,
            refresh_hash: s.refresh_hash,
            refresh_expires_at: s.refresh_expires_at,
            created_at: s.created_at,
        });
        token.version = s.version;
        token.rotated_at = s.rotated_at;
        token.revoked_at = s.revoked_at;
        Ok(token)
    }
}
