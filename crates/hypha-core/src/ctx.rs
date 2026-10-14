use thiserror::Error;

use crate::{AgentId, ErrorCode, UserId, scope::Scope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Actor {
    User(UserId),
    Agent(AgentId),
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctx {
    actor: Actor,
    scopes: Vec<Scope>,
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
    pub fn new(actor: Actor, scopes: Vec<Scope>) -> Self {
        Self { actor, scopes }
    }

    pub fn system() -> Self {
        Self::new(Actor::System, Scope::ALL.to_vec())
    }

    pub fn actor(&self) -> Actor {
        self.actor
    }

    pub fn has_scope(&self, scope: Scope) -> bool {
        self.scopes.contains(&scope)
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
    fn system_has_every_scope() {
        let ctx = Ctx::system();
        for scope in Scope::ALL {
            assert_eq!(ctx.require(scope), Ok(()), "{scope:?}")
        }
    }
}
