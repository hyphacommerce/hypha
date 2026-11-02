//! Password reset requests: a single-use emailed token.

use core::time::Duration;

use hypha_core::{IdentityId, PasswordResetId, Timestamp, Version, Versioned};

use crate::token::TokenHash;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordReset {
    id: PasswordResetId,
    version: Version,
    identity: IdentityId,
    token_hash: TokenHash,
    created_at: Timestamp,
    expires_at: Timestamp,
    used_at: Option<Timestamp>,
}

impl Versioned for PasswordReset {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl PasswordReset {
    /// A reset whose emailed link works for `lifetime`
    /// (`AuthSettings::password_reset_lifetime`).
    pub fn new(
        id: PasswordResetId,
        identity: IdentityId,
        token_hash: TokenHash,
        now: Timestamp,
        lifetime: Duration,
    ) -> Self {
        Self {
            id,
            version: Version::NEW,
            identity,
            token_hash,
            created_at: now,
            expires_at: now.plus(lifetime),
            used_at: None,
        }
    }

    pub fn id(&self) -> PasswordResetId {
        self.id
    }

    pub fn identity(&self) -> IdentityId {
        self.identity
    }

    pub fn token_hash(&self) -> TokenHash {
        self.token_hash
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }

    pub fn used_at(&self) -> Option<Timestamp> {
        self.used_at
    }

    /// Unused and not expired.
    pub fn is_usable(&self, now: Timestamp) -> bool {
        self.used_at.is_none() && now < self.expires_at
    }

    /// Whether a new reset for the same identity should wait: this one still works
    /// (unused, not expired) and is younger than `resend_after`
    /// (`AuthSettings::password_reset_resend_after`). An expired link never holds one
    /// back, whichever of the two settings is longer.
    pub fn blocks_another(&self, now: Timestamp, resend_after: Duration) -> bool {
        self.is_usable(now) && now < self.created_at.plus(resend_after)
    }

    /// Uses the token up. Callers check `is_usable` first.
    pub fn mark_used(&mut self, now: Timestamp) {
        self.used_at.get_or_insert(now);
    }

    pub fn snapshot(&self) -> PasswordResetSnapshot {
        PasswordResetSnapshot {
            id: self.id,
            version: self.version,
            identity: self.identity,
            token_hash: self.token_hash,
            created_at: self.created_at,
            expires_at: self.expires_at,
            used_at: self.used_at,
        }
    }

    pub fn restore(s: PasswordResetSnapshot) -> PasswordReset {
        PasswordReset {
            id: s.id,
            version: s.version,
            identity: s.identity,
            token_hash: s.token_hash,
            created_at: s.created_at,
            expires_at: s.expires_at,
            used_at: s.used_at,
        }
    }
}

/// Plain-data form of a `PasswordReset` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordResetSnapshot {
    pub id: PasswordResetId,
    pub version: Version,
    pub identity: IdentityId,
    pub token_hash: TokenHash,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    pub used_at: Option<Timestamp>,
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn usable_once_within_its_lifetime() {
        let now = Timestamp::UNIX_EPOCH;
        let mut r = PasswordReset::new(
            PasswordResetId::from_uuid(Uuid::from_u128(1)),
            IdentityId::from_uuid(Uuid::from_u128(2)),
            TokenHash::from_bytes([0; 32]),
            now,
            Duration::from_secs(3600),
        );
        assert!(r.is_usable(now.plus_secs(3599)));
        assert!(!r.is_usable(now.plus_secs(3600)));
        r.mark_used(now);
        assert!(!r.is_usable(now));
        assert_eq!(r.used_at(), Some(now));
    }

    fn reset(now: Timestamp, lifetime_secs: u64) -> PasswordReset {
        PasswordReset::new(
            PasswordResetId::from_uuid(Uuid::from_u128(1)),
            IdentityId::from_uuid(Uuid::from_u128(2)),
            TokenHash::from_bytes([0; 32]),
            now,
            Duration::from_secs(lifetime_secs),
        )
    }

    #[test]
    fn an_expired_link_never_holds_another_back() {
        // Resending waits a day, but the link only worked for an hour.
        let now = Timestamp::UNIX_EPOCH;
        let r = reset(now, 3600);
        let resend_after = Duration::from_secs(86_400);
        assert!(r.blocks_another(now.plus_secs(3599), resend_after));
        assert!(!r.blocks_another(now.plus_secs(3600), resend_after));
    }
}
