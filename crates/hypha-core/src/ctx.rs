use thiserror::Error;

use crate::{AgentId, ApiKeyId, ChannelId, ErrorCode, UserId, scope::Scope};

/// Who a command runs for, for audit and idempotency (keys are per actor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Actor {
    /// A staff member, through a session (design note 0008).
    User(UserId),
    /// An admin assistant acting with its own scopes (not built yet).
    Agent(AgentId),
    /// A script, integration or storefront, through a secret or publishable key.
    ApiKey(ApiKeyId),
    /// The host itself: startup tasks and tests.
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctx {
    actor: Actor,
    scopes: Vec<Scope>,
    /// The sales channel the caller sells through (design note 0020, CH5): a
    /// storefront's, from its publishable key. `None` for staff, secret keys and the
    /// system, who see every channel.
    channel: Option<ChannelId>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]

pub enum AuthError {
    #[error("missing scope {0}")]
    MissingScope(Scope),
}

impl ErrorCode for AuthError {
    fn code(&self) -> &'static str {
        match self {
            AuthError::MissingScope(_) => "missing_scope",
        }
    }
}

impl Ctx {
    /// A caller in no channel: staff, a secret key, the system.
    pub fn new(actor: Actor, scopes: Vec<Scope>) -> Self {
        Self {
            actor,
            scopes,
            channel: None,
        }
    }

    /// A storefront caller, selling through `channel` (its key's).
    pub fn in_channel(actor: Actor, scopes: Vec<Scope>, channel: ChannelId) -> Self {
        Self {
            actor,
            scopes,
            channel: Some(channel),
        }
    }

    pub fn system() -> Self {
        Self::new(Actor::System, Scope::ALL.to_vec())
    }

    pub fn actor(&self) -> Actor {
        self.actor
    }

    /// The channel the caller sells through; `None` sees every channel.
    pub fn channel(&self) -> Option<ChannelId> {
        self.channel
    }

    /// Whether the caller holds `scope`, directly or through the write scope that
    /// implies it (`Scope::implied_by`).
    pub fn has_scope(&self, scope: Scope) -> bool {
        self.scopes.contains(&scope)
            || scope
                .implied_by()
                .is_some_and(|wider| self.scopes.contains(&wider))
    }

    /// The scopes as granted, without the implied ones.
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    pub fn require(&self, scope: Scope) -> Result<(), AuthError> {
        if !self.has_scope(scope) {
            Err(AuthError::MissingScope(scope))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn reader() -> Ctx {
        Ctx::new(
            Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
            vec![Scope::ProductsRead],
        )
    }

    #[test]
    fn has_only_granted_scopes() {
        let ctx = reader();
        assert!(ctx.has_scope(Scope::ProductsRead));
        assert!(!ctx.has_scope(Scope::ProductsWrite));
    }

    #[test]
    fn require_accepts_granted_scope() {
        assert_eq!(reader().require(Scope::ProductsRead), Ok(()))
    }

    #[test]
    fn require_rejects_missing_scope() {
        assert_eq!(
            reader().require(Scope::ProductsWrite),
            Err(AuthError::MissingScope(Scope::ProductsWrite))
        );
    }

    #[test]
    fn a_write_scope_grants_its_read_scope_but_not_the_reverse() {
        let writer = Ctx::new(Actor::System, vec![Scope::ProductsWrite]);
        assert!(writer.has_scope(Scope::ProductsRead));
        assert!(!writer.has_scope(Scope::MarketsRead));
        assert!(!reader().has_scope(Scope::ProductsWrite));
    }

    #[test]
    fn system_has_every_scope() {
        let ctx = Ctx::system();
        for scope in Scope::ALL {
            assert_eq!(ctx.require(scope), Ok(()), "{scope:?}")
        }
    }
}
