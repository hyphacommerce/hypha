//! A pending authorization (design note 0029, §4.1 step 2): what an app asked for at
//! `/oauth/authorize`, waiting ten minutes for a staff member's answer on the consent
//! page. Kept after the answer, as the app's request history.

use hypha_apps::{AppManifest, ManifestSource, is_private_client, normalize};
use hypha_core::{Actor, AppKey, OAuthRequestId, Scope, Timestamp, Version, Versioned};

use crate::error::CommandError;

/// Where a request stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    Pending,
    Approved,
    Denied,
}

impl RequestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RequestStatus::Pending => "pending",
            RequestStatus::Approved => "approved",
            RequestStatus::Denied => "denied",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(RequestStatus::Pending),
            "approved" => Some(RequestStatus::Approved),
            "denied" => Some(RequestStatus::Denied),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizationRequest {
    id: OAuthRequestId,
    version: Version,
    app: AppKey,
    /// The manifest's URL: the app's client ID.
    client_id: String,
    /// The manifest as fetched for this request: approving installs from it.
    manifest: AppManifest,
    redirect_uri: String,
    state: Option<String>,
    /// The app's PKCE challenge; `None` for a request Hypha started itself (a manifest
    /// refresh asking for new scopes), which approving answers with no code.
    code_challenge: Option<String>,
    /// What the app asked for, in `Scope::ALL` order.
    scopes: Vec<Scope>,
    /// The app's grant when it asked (`None`: not installed), for the history.
    previous_scopes: Option<Vec<Scope>>,
    status: RequestStatus,
    decided_by: Option<Actor>,
    decided_at: Option<Timestamp>,
    created_at: Timestamp,
    expires_at: Timestamp,
}

impl Versioned for AuthorizationRequest {
    fn version(&self) -> Version {
        self.version
    }
    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

/// A new request's parts.
pub struct NewRequest {
    pub id: OAuthRequestId,
    pub client_id: String,
    pub manifest: AppManifest,
    pub redirect_uri: String,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub scopes: Vec<Scope>,
    pub previous_scopes: Option<Vec<Scope>>,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

impl AuthorizationRequest {
    pub fn new(new: NewRequest) -> Self {
        Self {
            id: new.id,
            version: Version::NEW,
            app: new.manifest.key().clone(),
            client_id: new.client_id,
            manifest: new.manifest,
            redirect_uri: new.redirect_uri,
            state: new.state,
            code_challenge: new.code_challenge,
            scopes: normalize(&new.scopes),
            previous_scopes: new.previous_scopes.map(|s| normalize(&s)),
            status: RequestStatus::Pending,
            decided_by: None,
            decided_at: None,
            created_at: new.created_at,
            expires_at: new.expires_at,
        }
    }

    /// Whether it can still be answered at `now`.
    pub fn is_expired(&self, now: Timestamp) -> bool {
        now >= self.expires_at
    }

    /// Records staff's answer; only a pending, unexpired request takes one.
    pub fn decide(&mut self, approved: bool, by: Actor, at: Timestamp) -> Result<(), CommandError> {
        if self.status != RequestStatus::Pending {
            return Err(CommandError::RequestDecided(self.id));
        }
        if self.is_expired(at) {
            return Err(CommandError::RequestExpired(self.id));
        }
        self.status = if approved {
            RequestStatus::Approved
        } else {
            RequestStatus::Denied
        };
        self.decided_by = Some(by);
        self.decided_at = Some(at);
        Ok(())
    }

    pub fn id(&self) -> OAuthRequestId {
        self.id
    }
    pub fn app(&self) -> &AppKey {
        &self.app
    }
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    /// Whether it's a private app's (its manifest uploaded): approving it answers with
    /// the app's tokens rather than a redirect.
    pub fn is_private(&self) -> bool {
        is_private_client(&self.client_id)
    }
    pub fn manifest(&self) -> &AppManifest {
        &self.manifest
    }
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }
    pub fn state(&self) -> Option<&str> {
        self.state.as_deref()
    }
    pub fn code_challenge(&self) -> Option<&str> {
        self.code_challenge.as_deref()
    }
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }
    pub fn previous_scopes(&self) -> Option<&[Scope]> {
        self.previous_scopes.as_deref()
    }
    pub fn status(&self) -> RequestStatus {
        self.status
    }
    pub fn decided_by(&self) -> Option<Actor> {
        self.decided_by
    }
    pub fn decided_at(&self) -> Option<Timestamp> {
        self.decided_at
    }
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }
}

/// Plain data for storage adapters.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestSnapshot {
    pub id: OAuthRequestId,
    pub version: Version,
    pub app: String,
    pub client_id: String,
    pub manifest: serde_json::Value,
    pub redirect_uri: String,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub scopes: Vec<Scope>,
    pub previous_scopes: Option<Vec<Scope>>,
    pub status: RequestStatus,
    pub decided_by: Option<Actor>,
    pub decided_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

impl AuthorizationRequest {
    pub fn snapshot(&self) -> RequestSnapshot {
        RequestSnapshot {
            id: self.id,
            version: self.version,
            app: self.app.as_str().to_string(),
            client_id: self.client_id.clone(),
            manifest: self.manifest.to_value(),
            redirect_uri: self.redirect_uri.clone(),
            state: self.state.clone(),
            code_challenge: self.code_challenge.clone(),
            scopes: self.scopes.clone(),
            previous_scopes: self.previous_scopes.clone(),
            status: self.status,
            decided_by: self.decided_by,
            decided_at: self.decided_at,
            created_at: self.created_at,
            expires_at: self.expires_at,
        }
    }

    /// From storage, with the manifest's rules checked again.
    pub fn restore(s: RequestSnapshot) -> Result<Self, String> {
        let manifest =
            AppManifest::from_value_of(s.manifest, ManifestSource::of_client(&s.client_id))
                .map_err(|e| e.to_string())?;
        if manifest.key().as_str() != s.app {
            return Err(format!(
                "request {} names another app than its manifest",
                s.id
            ));
        }
        let mut request = Self::new(NewRequest {
            id: s.id,
            client_id: s.client_id,
            manifest,
            redirect_uri: s.redirect_uri,
            state: s.state,
            code_challenge: s.code_challenge,
            scopes: s.scopes,
            previous_scopes: s.previous_scopes,
            created_at: s.created_at,
            expires_at: s.expires_at,
        });
        request.version = s.version;
        request.status = s.status;
        request.decided_by = s.decided_by;
        request.decided_at = s.decided_at;
        Ok(request)
    }
}
