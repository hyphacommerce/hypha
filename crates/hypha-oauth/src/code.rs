//! Authorization codes (`hypha_ac_…`): what staff's approval sends back to the app's
//! redirect URI. Valid 60 seconds, once, and only with the client ID, redirect URI and PKCE
//! verifier it was issued for. Stored as SHA-256 only.

use hypha_apps::normalize;
use hypha_auth::TokenHash;
use hypha_core::{AppKey, OAuthRequestId, Scope, Timestamp, Version, Versioned};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationCode {
    hash: TokenHash,
    version: Version,
    request: OAuthRequestId,
    app: AppKey,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    scopes: Vec<Scope>,
    /// Whether exchanging it answers with the app's secret too: the app's first
    /// installation (or one after an uninstall), when it can't have the secret yet.
    first_install: bool,
    /// The token family its exchange starts: a second exchange revokes it.
    family: Uuid,
    expires_at: Timestamp,
    used_at: Option<Timestamp>,
}

impl Versioned for AuthorizationCode {
    fn version(&self) -> Version {
        self.version
    }
    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

/// A new code's parts.
pub struct NewCode {
    pub hash: TokenHash,
    pub request: OAuthRequestId,
    pub app: AppKey,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub scopes: Vec<Scope>,
    pub first_install: bool,
    pub family: Uuid,
    pub expires_at: Timestamp,
}

impl AuthorizationCode {
    pub fn new(new: NewCode) -> Self {
        Self {
            hash: new.hash,
            version: Version::NEW,
            request: new.request,
            app: new.app,
            client_id: new.client_id,
            redirect_uri: new.redirect_uri,
            code_challenge: new.code_challenge,
            scopes: normalize(&new.scopes),
            first_install: new.first_install,
            family: new.family,
            expires_at: new.expires_at,
            used_at: None,
        }
    }

    /// Spends it; `false` if it was spent already.
    pub fn use_up(&mut self, at: Timestamp) -> bool {
        if self.used_at.is_some() {
            return false;
        }
        self.used_at = Some(at);
        true
    }

    pub fn is_expired(&self, now: Timestamp) -> bool {
        now >= self.expires_at
    }

    pub fn hash(&self) -> TokenHash {
        self.hash
    }
    pub fn request(&self) -> OAuthRequestId {
        self.request
    }
    pub fn app(&self) -> &AppKey {
        &self.app
    }
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }
    pub fn code_challenge(&self) -> &str {
        &self.code_challenge
    }
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }
    pub fn first_install(&self) -> bool {
        self.first_install
    }
    pub fn family(&self) -> Uuid {
        self.family
    }
    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }
    pub fn used_at(&self) -> Option<Timestamp> {
        self.used_at
    }
}

/// Plain data for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSnapshot {
    pub hash: TokenHash,
    pub version: Version,
    pub request: OAuthRequestId,
    pub app: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub scopes: Vec<Scope>,
    pub first_install: bool,
    pub family: Uuid,
    pub expires_at: Timestamp,
    pub used_at: Option<Timestamp>,
}

impl AuthorizationCode {
    pub fn snapshot(&self) -> CodeSnapshot {
        CodeSnapshot {
            hash: self.hash,
            version: self.version,
            request: self.request,
            app: self.app.as_str().to_string(),
            client_id: self.client_id.clone(),
            redirect_uri: self.redirect_uri.clone(),
            code_challenge: self.code_challenge.clone(),
            scopes: self.scopes.clone(),
            first_install: self.first_install,
            family: self.family,
            expires_at: self.expires_at,
            used_at: self.used_at,
        }
    }

    pub fn restore(s: CodeSnapshot) -> Result<Self, String> {
        let app = AppKey::new(&s.app).map_err(|e| e.to_string())?;
        let mut code = Self::new(NewCode {
            hash: s.hash,
            request: s.request,
            app,
            client_id: s.client_id,
            redirect_uri: s.redirect_uri,
            code_challenge: s.code_challenge,
            scopes: s.scopes,
            first_install: s.first_install,
            family: s.family,
            expires_at: s.expires_at,
        });
        code.version = s.version;
        code.used_at = s.used_at;
        Ok(code)
    }
}
