//! OAuth's requests, codes and tokens in memory, with the database's rules: unique token
//! hashes, a code's request and a token's app must exist, an app's tokens go with it.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

use hypha_auth::TokenHash;
use hypha_core::{AgentId, AppKey, OAuthRequestId, RepoError, Timestamp, Versioned};
use hypha_oauth::{
    AuthorizationCode, AuthorizationRequest, OAuthCodeRepo, OAuthRequestRepo, OAuthToken,
    OAuthTokenRepo, keys,
};
use uuid::Uuid;

use crate::{
    apps::Apps,
    table::{insert_row, missing, no_unique_keys, update_row},
};

#[derive(Default, Clone)]
pub(crate) struct OAuthState {
    pub(crate) requests: HashMap<OAuthRequestId, AuthorizationRequest>,
    pub(crate) codes: HashMap<TokenHash, AuthorizationCode>,
    pub(crate) tokens: HashMap<Uuid, OAuthToken>,
}

/// One store's OAuth records: the request, code and token repos at once.
#[derive(Default, Clone)]
pub struct InMemoryOAuth {
    inner: Arc<Mutex<OAuthState>>,
    /// The apps tokens must belong to, when linked (`crate::apps::linked`).
    apps: Option<Apps>,
}

pub type InMemoryOAuthRequestRepo = InMemoryOAuth;
pub type InMemoryOAuthCodeRepo = InMemoryOAuth;
pub type InMemoryOAuthTokenRepo = InMemoryOAuth;

/// The two token hashes are unique among tokens.
pub(crate) fn unique_token(a: &OAuthToken, b: &OAuthToken) -> Option<&'static str> {
    let hashes = [b.access_hash(), b.refresh_hash()];
    if hashes.contains(&a.access_hash()) {
        Some(keys::OAUTH_ACCESS_TOKEN)
    } else if hashes.contains(&a.refresh_hash()) {
        Some(keys::OAUTH_REFRESH_TOKEN)
    } else {
        None
    }
}

impl InMemoryOAuth {
    pub(crate) fn linked(apps: &Apps) -> Self {
        Self {
            inner: Arc::default(),
            apps: Some(Arc::clone(apps)),
        }
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, OAuthState> {
        self.inner.lock().unwrap()
    }

    pub(crate) fn app_exists(&self, app: &AppKey) -> bool {
        self.apps
            .as_ref()
            .is_none_or(|apps| apps.lock().unwrap().contains_key(app.as_str()))
    }

    pub(crate) fn drop_app_tokens(&self, app: &AppKey) {
        self.lock()
            .tokens
            .retain(|_, t| t.client().app() != Some(app));
    }
}

/// Inserts a code on locked state, checking its request.
pub(crate) fn insert_code(
    state: &mut OAuthState,
    code: &mut AuthorizationCode,
) -> Result<(), RepoError> {
    if !state.requests.contains_key(&code.request()) {
        return Err(missing("authorization request", code.request()));
    }
    insert_row(
        &mut state.codes,
        code.hash(),
        code,
        keys::OAUTH_CODE,
        &no_unique_keys,
    )
}

/// Revokes `pick`'s tokens on locked state; how many.
pub(crate) fn revoke_where(
    state: &mut OAuthState,
    at: Timestamp,
    pick: impl Fn(&OAuthToken) -> bool,
) -> u32 {
    let mut n = 0;
    for token in state.tokens.values_mut() {
        if token.revoked_at().is_none() && pick(token) {
            token.revoke(at);
            token.set_version(token.version().next());
            n += 1;
        }
    }
    n
}

/// A valid manifest for tests: app `key` served from `app_url`, asking for
/// `orders:read`, `customers:read` and `customer_data:email`, with a webhook for
/// `order.*` and the callback `<app_url>/callback`. Tests use
/// `<app_url>/hypha-app.json` as its client ID (manifest URL).
pub fn app_manifest(key: &str, app_url: &str) -> hypha_apps::AppManifest {
    let document = serde_json::json!({
        "key": key,
        "name": "Test app",
        "version": "1.0.0",
        "publisher": "Tests Inc.",
        "app_url": app_url,
        "redirect_uris": ["/callback"],
        "scopes": ["orders:read", "customers:read", "customer_data:email"],
        "scope_reasons": { "customer_data:email": "to send receipts" },
        "privacy_url": "/privacy",
        "webhooks": { "url": "/hooks", "events": ["order.*"] }
    });
    hypha_apps::AppManifest::parse(document.to_string().as_bytes())
        .unwrap_or_else(|e| panic!("the test manifest is invalid: {e}"))
}

impl OAuthRequestRepo for InMemoryOAuth {
    async fn get(&self, id: OAuthRequestId) -> Result<Option<AuthorizationRequest>, RepoError> {
        Ok(self.lock().requests.get(&id).cloned())
    }

    async fn list_for_app(
        &self,
        app: &AppKey,
        limit: u32,
    ) -> Result<Vec<AuthorizationRequest>, RepoError> {
        let mut found: Vec<AuthorizationRequest> = self
            .lock()
            .requests
            .values()
            .filter(|r| r.app() == Some(app))
            .cloned()
            .collect();
        found.sort_by(|a, b| {
            b.created_at()
                .cmp(&a.created_at())
                .then(b.id().as_uuid().cmp(&a.id().as_uuid()))
        });
        found.truncate(limit as usize);
        Ok(found)
    }

    async fn insert(&self, request: &mut AuthorizationRequest) -> Result<(), RepoError> {
        insert_row(
            &mut self.lock().requests,
            request.id(),
            request,
            keys::OAUTH_REQUEST_ID,
            &no_unique_keys,
        )
    }

    async fn update(&self, request: &mut AuthorizationRequest) -> Result<(), RepoError> {
        update_row(
            &mut self.lock().requests,
            request.id(),
            request,
            &no_unique_keys,
        )
    }
}

impl OAuthCodeRepo for InMemoryOAuth {
    async fn get(&self, hash: TokenHash) -> Result<Option<AuthorizationCode>, RepoError> {
        Ok(self.lock().codes.get(&hash).cloned())
    }

    async fn insert(&self, code: &mut AuthorizationCode) -> Result<(), RepoError> {
        insert_code(&mut self.lock(), code)
    }

    async fn update(&self, code: &mut AuthorizationCode) -> Result<(), RepoError> {
        update_row(&mut self.lock().codes, code.hash(), code, &no_unique_keys)
    }
}

impl OAuthTokenRepo for InMemoryOAuth {
    async fn get_by_access(&self, hash: TokenHash) -> Result<Option<OAuthToken>, RepoError> {
        Ok(self
            .lock()
            .tokens
            .values()
            .find(|t| t.access_hash() == hash)
            .cloned())
    }

    async fn get_by_refresh(&self, hash: TokenHash) -> Result<Option<OAuthToken>, RepoError> {
        Ok(self
            .lock()
            .tokens
            .values()
            .find(|t| t.refresh_hash() == hash)
            .cloned())
    }

    async fn insert(&self, token: &mut OAuthToken) -> Result<(), RepoError> {
        // An agent's must exist too; the repo alone can't see agents (a batch checks).
        if let Some(app) = token.client().app()
            && !self.app_exists(app)
        {
            return Err(missing("app", app));
        }
        insert_row(
            &mut self.lock().tokens,
            token.id(),
            token,
            keys::OAUTH_TOKEN_ID,
            &unique_token,
        )
    }

    async fn update(&self, token: &mut OAuthToken) -> Result<(), RepoError> {
        update_row(&mut self.lock().tokens, token.id(), token, &unique_token)
    }

    async fn revoke_family(&self, family: Uuid, at: Timestamp) -> Result<u32, RepoError> {
        Ok(revoke_where(&mut self.lock(), at, |t| t.family() == family))
    }

    async fn revoke_app(&self, app: &AppKey, at: Timestamp) -> Result<u32, RepoError> {
        Ok(revoke_where(&mut self.lock(), at, |t| {
            t.client().app() == Some(app)
        }))
    }

    async fn revoke_agent(&self, agent: AgentId, at: Timestamp) -> Result<u32, RepoError> {
        Ok(revoke_where(&mut self.lock(), at, |t| {
            t.client().agent() == Some(agent)
        }))
    }
}
