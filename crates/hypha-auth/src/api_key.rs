//! API keys (design note 0008, A8): how scripts, integrations and storefronts call.

use hypha_core::{Actor, ApiKeyId, ChannelId, Scope, Timestamp, Version, Versioned, required_text};

use crate::{error::ApiKeyError, error::RestoreError, token::TokenHash};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiKeyKind {
    /// For servers: carries the scopes it was given. Never put one in a browser.
    Secret,
    /// For storefronts, safe to ship to browsers: always `Scope::STOREFRONT`.
    Publishable,
}

impl ApiKeyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ApiKeyKind::Secret => "secret",
            ApiKeyKind::Publishable => "publishable",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "secret" => Some(ApiKeyKind::Secret),
            "publishable" => Some(ApiKeyKind::Publishable),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKey {
    id: ApiKeyId,
    version: Version,
    kind: ApiKeyKind,
    name: String,
    token_hash: TokenHash,
    last_four: String,
    scopes: Vec<Scope>,
    /// The sales channel a publishable key sells through (design note 0020, CH4); a
    /// secret key has none.
    channel: Option<ChannelId>,
    created_by: Actor,
    created_at: Timestamp,
    revoked_at: Option<Timestamp>,
}

impl Versioned for ApiKey {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

/// What a new key is made from, besides its token.
pub struct ApiKeyParts {
    pub id: ApiKeyId,
    pub kind: ApiKeyKind,
    pub name: String,
    /// Required for a secret key; must be `None` for a publishable one.
    pub scopes: Option<Vec<Scope>>,
    /// Required for a publishable key; must be `None` for a secret one.
    pub channel: Option<ChannelId>,
    pub created_by: Actor,
    pub created_at: Timestamp,
}

impl ApiKey {
    /// The name is trimmed and required. Scopes are kept in `Scope::ALL` order without
    /// repeats, so two keys with the same grants look the same.
    pub fn new(
        parts: ApiKeyParts,
        token_hash: TokenHash,
        last_four: &str,
    ) -> Result<Self, ApiKeyError> {
        let scopes = match (parts.kind, parts.scopes) {
            (ApiKeyKind::Publishable, None) => normalize(&Scope::STOREFRONT),
            (ApiKeyKind::Publishable, Some(_)) => return Err(ApiKeyError::PublishableScopesFixed),
            (ApiKeyKind::Secret, None) => return Err(ApiKeyError::NoScopes),
            (ApiKeyKind::Secret, Some(scopes)) => normalize(&scopes),
        };
        if scopes.is_empty() {
            return Err(ApiKeyError::NoScopes);
        }
        match (parts.kind, parts.channel) {
            (ApiKeyKind::Publishable, None) => return Err(ApiKeyError::ChannelRequired),
            (ApiKeyKind::Secret, Some(_)) => return Err(ApiKeyError::ChannelNotAllowed),
            _ => {}
        }
        Ok(Self {
            id: parts.id,
            version: Version::NEW,
            kind: parts.kind,
            name: required_text("name", &parts.name)?,
            token_hash,
            last_four: last_four.to_string(),
            scopes,
            channel: parts.channel,
            created_by: parts.created_by,
            created_at: parts.created_at,
            revoked_at: None,
        })
    }

    pub fn id(&self) -> ApiKeyId {
        self.id
    }

    pub fn kind(&self) -> ApiKeyKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn token_hash(&self) -> TokenHash {
        self.token_hash
    }

    /// The token as it may be shown again: prefix, an ellipsis, the last four.
    pub fn redacted(&self) -> String {
        let prefix = match self.kind {
            ApiKeyKind::Secret => crate::token::TokenKind::SecretKey.prefix(),
            ApiKeyKind::Publishable => crate::token::TokenKind::PublishableKey.prefix(),
        };
        format!("{prefix}…{}", self.last_four)
    }

    pub fn last_four(&self) -> &str {
        &self.last_four
    }

    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    /// The channel a publishable key sells through; `None` for a secret key.
    pub fn channel(&self) -> Option<ChannelId> {
        self.channel
    }

    pub fn created_by(&self) -> Actor {
        self.created_by
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn revoked_at(&self) -> Option<Timestamp> {
        self.revoked_at
    }

    /// Stops the key working. `false` if it already was revoked.
    pub fn revoke(&mut self, now: Timestamp) -> bool {
        if self.revoked_at.is_some() {
            return false;
        }
        self.revoked_at = Some(now);
        true
    }

    pub fn snapshot(&self) -> ApiKeySnapshot {
        ApiKeySnapshot {
            id: self.id,
            version: self.version,
            kind: self.kind,
            name: self.name.clone(),
            token_hash: self.token_hash,
            last_four: self.last_four.clone(),
            scopes: self.scopes.clone(),
            channel: self.channel,
            created_by: self.created_by,
            created_at: self.created_at,
            revoked_at: self.revoked_at,
        }
    }

    /// Rebuilds a key from storage, re-checking the name and the scope rules.
    ///
    /// A publishable key's scopes come from its kind, not from storage: they are
    /// whatever `Scope::STOREFRONT` is in this version, so changing that list changes
    /// every publishable key at once instead of making the stored ones unreadable.
    pub fn restore(s: ApiKeySnapshot) -> Result<ApiKey, RestoreError> {
        let scopes = match s.kind {
            ApiKeyKind::Secret => Some(s.scopes),
            ApiKeyKind::Publishable => None,
        };
        let parts = ApiKeyParts {
            id: s.id,
            kind: s.kind,
            name: s.name,
            scopes,
            channel: s.channel,
            created_by: s.created_by,
            created_at: s.created_at,
        };
        let mut key = ApiKey::new(parts, s.token_hash, &s.last_four)?;
        key.version = s.version;
        key.revoked_at = s.revoked_at;
        Ok(key)
    }
}

fn normalize(scopes: &[Scope]) -> Vec<Scope> {
    Scope::ALL
        .into_iter()
        .filter(|s| scopes.contains(s))
        .collect()
}

/// Plain-data form of an `ApiKey` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeySnapshot {
    pub id: ApiKeyId,
    pub version: Version,
    pub kind: ApiKeyKind,
    pub name: String,
    pub token_hash: TokenHash,
    pub last_four: String,
    pub scopes: Vec<Scope>,
    pub channel: Option<ChannelId>,
    pub created_by: Actor,
    pub created_at: Timestamp,
    pub revoked_at: Option<Timestamp>,
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    fn parts(kind: ApiKeyKind, scopes: Option<Vec<Scope>>) -> ApiKeyParts {
        ApiKeyParts {
            id: ApiKeyId::from_uuid(Uuid::from_u128(1)),
            kind,
            name: " Importer ".to_string(),
            scopes: scopes.clone(),
            channel: (kind == ApiKeyKind::Publishable)
                .then(|| hypha_core::ChannelId::from_uuid(Uuid::from_u128(9))),
            created_by: Actor::System,
            created_at: Timestamp::UNIX_EPOCH,
        }
    }

    fn key(kind: ApiKeyKind, scopes: Option<Vec<Scope>>) -> Result<ApiKey, ApiKeyError> {
        ApiKey::new(parts(kind, scopes), TokenHash::from_bytes([0; 32]), "a1b2")
    }

    #[test]
    fn a_secret_key_keeps_its_scopes_sorted_and_once() {
        let k = key(
            ApiKeyKind::Secret,
            Some(vec![
                Scope::OrdersRead,
                Scope::ProductsWrite,
                Scope::OrdersRead,
            ]),
        )
        .unwrap();
        assert_eq!(k.scopes(), [Scope::ProductsWrite, Scope::OrdersRead]);
        assert_eq!(k.name(), "Importer");
        assert_eq!(k.redacted(), "hypha_sk_…a1b2");
    }

    #[test]
    fn a_secret_key_needs_scopes() {
        assert_eq!(key(ApiKeyKind::Secret, None), Err(ApiKeyError::NoScopes));
        assert_eq!(
            key(ApiKeyKind::Secret, Some(vec![])),
            Err(ApiKeyError::NoScopes)
        );
    }

    #[test]
    fn a_publishable_key_has_the_storefront_scopes_only() {
        let k = key(ApiKeyKind::Publishable, None).unwrap();
        assert_eq!(k.scopes(), Scope::STOREFRONT);
        assert_eq!(k.redacted(), "hypha_pk_…a1b2");
        assert_eq!(
            key(ApiKeyKind::Publishable, Some(vec![Scope::ProductsRead])),
            Err(ApiKeyError::PublishableScopesFixed)
        );
    }

    #[test]
    fn a_key_needs_a_name() {
        let mut p = parts(ApiKeyKind::Publishable, None);
        p.name = "  ".to_string();
        assert_eq!(
            ApiKey::new(p, TokenHash::from_bytes([0; 32]), "a1b2"),
            Err(ApiKeyError::Text(TextError::Missing("name")))
        );
    }

    #[test]
    fn revoking_twice_reports_the_second() {
        let mut k = key(ApiKeyKind::Publishable, None).unwrap();
        assert!(k.revoke(Timestamp::UNIX_EPOCH));
        assert!(!k.revoke(Timestamp::UNIX_EPOCH.plus_secs(1)));
        assert_eq!(k.revoked_at(), Some(Timestamp::UNIX_EPOCH));
    }

    #[test]
    fn snapshot_round_trips() {
        for k in [
            key(ApiKeyKind::Publishable, None).unwrap(),
            key(ApiKeyKind::Secret, Some(vec![Scope::OrdersRead])).unwrap(),
        ] {
            assert_eq!(ApiKey::restore(k.snapshot()), Ok(k));
        }
    }

    #[test]
    fn a_stored_publishable_key_gets_todays_storefront_scopes() {
        let mut s = key(ApiKeyKind::Publishable, None).unwrap().snapshot();
        // As if stored by a version whose storefront scopes were different.
        s.scopes = vec![Scope::OrdersWrite];
        assert_eq!(
            ApiKey::restore(s).map(|k| k.scopes().to_vec()),
            Ok(normalize(&Scope::STOREFRONT))
        );
    }
}
