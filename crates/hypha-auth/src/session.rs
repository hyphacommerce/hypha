//! Sessions (design note 0008, A3 and A4): what a staff member's requests carry after
//! logging in. Server-side, so revoking one works on the next request.

use core::time::Duration;

use hypha_core::{IdentityId, SessionId, Timestamp, Version, Versioned};

use crate::{identity::Principal, token::TokenHash};

/// Where a session is in the login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// The password was right and a second factor is still owed. The token only works
    /// for finishing the login.
    Pending { failed_attempts: u32 },
    /// Logged in. `mfa_at` is when a second factor was last proven (`None` without
    /// one), for step-up checks later.
    Active { mfa_at: Option<Timestamp> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    id: SessionId,
    version: Version,
    identity: IdentityId,
    principal: Principal,
    token_hash: TokenHash,
    state: SessionState,
    created_at: Timestamp,
    expires_at: Timestamp,
    revoked_at: Option<Timestamp>,
}

impl Versioned for Session {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Session {
    /// Wrong second-factor codes before a pending session is revoked.
    pub const MAX_ATTEMPTS: u32 = 5;

    /// A logged-in session, lasting `lifetime` (`AuthSettings::session_lifetime`).
    pub fn active(
        id: SessionId,
        identity: IdentityId,
        principal: Principal,
        token_hash: TokenHash,
        now: Timestamp,
        mfa_at: Option<Timestamp>,
        lifetime: Duration,
    ) -> Self {
        Self {
            id,
            version: Version::NEW,
            identity,
            principal,
            token_hash,
            state: SessionState::Active { mfa_at },
            created_at: now,
            expires_at: now.plus(lifetime),
            revoked_at: None,
        }
    }

    /// A session waiting for the second factor for `window`
    /// (`AuthSettings::second_factor_window`).
    pub fn pending(
        id: SessionId,
        identity: IdentityId,
        principal: Principal,
        token_hash: TokenHash,
        now: Timestamp,
        window: Duration,
    ) -> Self {
        Self {
            id,
            version: Version::NEW,
            identity,
            principal,
            token_hash,
            state: SessionState::Pending { failed_attempts: 0 },
            created_at: now,
            expires_at: now.plus(window),
            revoked_at: None,
        }
    }

    pub fn id(&self) -> SessionId {
        self.id
    }

    pub fn identity(&self) -> IdentityId {
        self.identity
    }

    pub fn principal(&self) -> Principal {
        self.principal
    }

    pub fn token_hash(&self) -> TokenHash {
        self.token_hash
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }

    pub fn revoked_at(&self) -> Option<Timestamp> {
        self.revoked_at
    }

    /// Neither revoked nor expired.
    pub fn is_live(&self, now: Timestamp) -> bool {
        self.revoked_at.is_none() && now < self.expires_at
    }

    /// Live and logged in: what every authenticated request needs.
    pub fn is_active(&self, now: Timestamp) -> bool {
        self.is_live(now) && matches!(self.state, SessionState::Active { .. })
    }

    /// Live and waiting for the second factor.
    pub fn is_pending(&self, now: Timestamp) -> bool {
        self.is_live(now) && matches!(self.state, SessionState::Pending { .. })
    }

    /// Finishes the login under a new token, so the pending one can't be replayed. The
    /// `lifetime` starts now. Callers check `is_pending` first.
    pub fn activate(&mut self, token_hash: TokenHash, now: Timestamp, lifetime: Duration) {
        self.token_hash = token_hash;
        self.state = SessionState::Active { mfa_at: Some(now) };
        self.expires_at = now.plus(lifetime);
    }

    /// Counts a wrong second-factor code; the `MAX_ATTEMPTS`th revokes the session.
    pub fn record_failed_attempt(&mut self, now: Timestamp) {
        if let SessionState::Pending { failed_attempts } = &mut self.state {
            *failed_attempts += 1;
            if *failed_attempts >= Self::MAX_ATTEMPTS {
                self.revoke(now);
            }
        }
    }

    /// Ends the session. Revoking twice keeps the first time.
    pub fn revoke(&mut self, now: Timestamp) {
        self.revoked_at.get_or_insert(now);
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            id: self.id,
            version: self.version,
            identity: self.identity,
            principal: self.principal,
            token_hash: self.token_hash,
            state: self.state,
            created_at: self.created_at,
            expires_at: self.expires_at,
            revoked_at: self.revoked_at,
        }
    }

    pub fn restore(s: SessionSnapshot) -> Session {
        Session {
            id: s.id,
            version: s.version,
            identity: s.identity,
            principal: s.principal,
            token_hash: s.token_hash,
            state: s.state,
            created_at: s.created_at,
            expires_at: s.expires_at,
            revoked_at: s.revoked_at,
        }
    }
}

/// Plain-data form of a `Session` for storage adapters. Every combination is a valid
/// session, so `restore` can't fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub id: SessionId,
    pub version: Version,
    pub identity: IdentityId,
    pub principal: Principal,
    pub token_hash: TokenHash,
    pub state: SessionState,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    pub revoked_at: Option<Timestamp>,
}

#[cfg(test)]
mod tests {
    use hypha_core::UserId;
    use uuid::Uuid;

    use super::*;

    const NOW: Timestamp = Timestamp::from_unix_micros(1_000_000_000_000);
    const WINDOW: Duration = Duration::from_secs(300);
    const LIFETIME: Duration = Duration::from_secs(7 * 24 * 3600);

    fn pending() -> Session {
        Session::pending(
            SessionId::from_uuid(Uuid::from_u128(1)),
            IdentityId::from_uuid(Uuid::from_u128(2)),
            Principal::Staff(UserId::from_uuid(Uuid::from_u128(3))),
            TokenHash::from_bytes([1; 32]),
            NOW,
            WINDOW,
        )
    }

    #[test]
    fn a_pending_session_lasts_its_window_and_is_not_active() {
        let s = pending();
        assert!(s.is_pending(NOW));
        assert!(!s.is_active(NOW));
        assert!(s.is_live(NOW.plus_secs(299)));
        assert!(!s.is_live(NOW.plus_secs(300)));
    }

    #[test]
    fn activating_swaps_the_token_and_restarts_the_lifetime() {
        let mut s = pending();
        let later = NOW.plus_secs(60);
        s.activate(TokenHash::from_bytes([2; 32]), later, LIFETIME);
        assert!(s.is_active(later));
        assert_eq!(s.token_hash(), TokenHash::from_bytes([2; 32]));
        assert_eq!(
            s.state(),
            SessionState::Active {
                mfa_at: Some(later)
            }
        );
        assert_eq!(s.expires_at(), later.plus(LIFETIME));
    }

    #[test]
    fn the_fifth_wrong_code_revokes() {
        let mut s = pending();
        for _ in 0..4 {
            s.record_failed_attempt(NOW);
        }
        assert!(s.is_pending(NOW));
        s.record_failed_attempt(NOW);
        assert_eq!(s.revoked_at(), Some(NOW));
        assert!(!s.is_live(NOW));
    }

    #[test]
    fn revoking_keeps_the_first_time() {
        let mut s = pending();
        s.revoke(NOW);
        s.revoke(NOW.plus_secs(1));
        assert_eq!(s.revoked_at(), Some(NOW));
    }
}
