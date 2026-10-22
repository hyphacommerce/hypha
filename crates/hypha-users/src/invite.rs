//! Invites: how staff join. An admin invites an email with a role; the emailed token lets
//! the invitee set a name and password.

use core::time::Duration;

use hypha_auth::TokenHash;
use hypha_core::{Actor, Email, InviteId, RoleId, Timestamp, Version, Versioned};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    id: InviteId,
    version: Version,
    email: Email,
    role: RoleId,
    token_hash: TokenHash,
    invited_by: Actor,
    created_at: Timestamp,
    expires_at: Timestamp,
    accepted_at: Option<Timestamp>,
}

impl Versioned for Invite {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Invite {
    /// An invite that works for `lifetime` (`AuthSettings::invite_lifetime`).
    pub fn new(
        id: InviteId,
        email: Email,
        role: RoleId,
        token_hash: TokenHash,
        invited_by: Actor,
        now: Timestamp,
        lifetime: Duration,
    ) -> Self {
        Self {
            id,
            version: Version::NEW,
            email,
            role,
            token_hash,
            invited_by,
            created_at: now,
            expires_at: now.plus(lifetime),
            accepted_at: None,
        }
    }

    pub fn id(&self) -> InviteId {
        self.id
    }

    pub fn email(&self) -> &Email {
        &self.email
    }

    pub fn role(&self) -> RoleId {
        self.role
    }

    pub fn token_hash(&self) -> TokenHash {
        self.token_hash
    }

    pub fn invited_by(&self) -> Actor {
        self.invited_by
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }

    pub fn accepted_at(&self) -> Option<Timestamp> {
        self.accepted_at
    }

    /// Not accepted and not expired.
    pub fn is_usable(&self, now: Timestamp) -> bool {
        self.accepted_at.is_none() && now < self.expires_at
    }

    /// Sends the invite again: a new token (the old one stops working), `lifetime` from
    /// now, and maybe another role. Callers check it isn't accepted.
    pub fn renew(
        &mut self,
        token_hash: TokenHash,
        role: RoleId,
        by: Actor,
        now: Timestamp,
        lifetime: Duration,
    ) {
        self.token_hash = token_hash;
        self.role = role;
        self.invited_by = by;
        self.created_at = now;
        self.expires_at = now.plus(lifetime);
    }

    /// Callers check `is_usable` first.
    pub fn accept(&mut self, now: Timestamp) {
        self.accepted_at.get_or_insert(now);
    }

    pub fn snapshot(&self) -> InviteSnapshot {
        InviteSnapshot {
            id: self.id,
            version: self.version,
            email: self.email.clone(),
            role: self.role,
            token_hash: self.token_hash,
            invited_by: self.invited_by,
            created_at: self.created_at,
            expires_at: self.expires_at,
            accepted_at: self.accepted_at,
        }
    }

    pub fn restore(s: InviteSnapshot) -> Invite {
        Invite {
            id: s.id,
            version: s.version,
            email: s.email,
            role: s.role,
            token_hash: s.token_hash,
            invited_by: s.invited_by,
            created_at: s.created_at,
            expires_at: s.expires_at,
            accepted_at: s.accepted_at,
        }
    }
}

/// Plain-data form of an `Invite` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteSnapshot {
    pub id: InviteId,
    pub version: Version,
    pub email: Email,
    pub role: RoleId,
    pub token_hash: TokenHash,
    pub invited_by: Actor,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    pub accepted_at: Option<Timestamp>,
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    const NOW: Timestamp = Timestamp::UNIX_EPOCH;
    const WEEK: Duration = Duration::from_secs(7 * 24 * 3600);

    fn invite() -> Invite {
        Invite::new(
            InviteId::from_uuid(Uuid::from_u128(1)),
            Email::new("ana@example.com").unwrap(),
            RoleId::from_uuid(Uuid::from_u128(2)),
            TokenHash::from_bytes([1; 32]),
            Actor::System,
            NOW,
            WEEK,
        )
    }

    #[test]
    fn usable_for_a_week_until_accepted() {
        let mut i = invite();
        assert!(i.is_usable(NOW.plus(WEEK).plus_secs(-1)));
        assert!(!i.is_usable(NOW.plus(WEEK)));
        i.accept(NOW);
        assert!(!i.is_usable(NOW));
    }

    #[test]
    fn renewing_swaps_the_token_and_restarts_the_week() {
        let mut i = invite();
        let later = NOW.plus(WEEK).plus_secs(10);
        let role = RoleId::from_uuid(Uuid::from_u128(3));
        i.renew(
            TokenHash::from_bytes([2; 32]),
            role,
            Actor::System,
            later,
            WEEK,
        );
        assert!(i.is_usable(later));
        assert_eq!(i.token_hash(), TokenHash::from_bytes([2; 32]));
        assert_eq!(i.role(), role);
    }
}
