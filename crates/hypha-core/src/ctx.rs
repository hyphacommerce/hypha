use thiserror::Error;

use crate::{AgentId, ApiKeyId, AppId, ChannelId, CustomerId, ErrorCode, UserId, scope::Scope};

/// Who a command runs for, for audit and idempotency (keys are per actor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Actor {
    /// A staff member, through a session.
    User(UserId),
    /// An admin agent: an MCP client a staff member connected, or an autonomous one staff
    /// made for a job, acting with what its grant allows.
    Agent(AgentId),
    /// A script, integration or storefront, through a secret or publishable key.
    ApiKey(ApiKeyId),
    /// An app installed through OAuth, through one of its access tokens, with the scopes
    /// staff granted it. Its ID rather than its key (`app_<name>`), so `Actor` stays
    /// `Copy`.
    App(AppId),
    /// A shopper signed in to their account on the storefront: the customer the session
    /// belongs to. Account commands act on this customer only.
    Customer(CustomerId),
    /// The host itself: startup tasks and tests.
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctx {
    actor: Actor,
    scopes: Vec<Scope>,
    /// The sales channel the caller sells through: a storefront's, from its publishable
    /// key. `None` for staff, secret keys and the system, who see every channel.
    channel: Option<ChannelId>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]

pub enum AuthError {
    #[error("missing scope {0}")]
    MissingScope(Scope),
    /// An app's token lacks the scope: the merchant hasn't granted it, which only a new
    /// authorization staff approve can change.
    #[error("the app is not authorized for {0}")]
    AppNotAuthorized(Scope),
}

impl ErrorCode for AuthError {
    fn code(&self) -> &'static str {
        match self {
            AuthError::MissingScope(_) => "missing_scope",
            AuthError::AppNotAuthorized(_) => "app_not_authorized",
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
        match self.actor {
            _ if self.has_scope(scope) => Ok(()),
            Actor::App(_) => Err(AuthError::AppNotAuthorized(scope)),
            _ => Err(AuthError::MissingScope(scope)),
        }
    }

    /// Whether the caller is a staff member through a session: the only callers who see
    /// personal data without the `customer_data:*` scopes.
    pub fn is_staff(&self) -> bool {
        matches!(self.actor, Actor::User(_))
    }

    /// The signed-in customer, when the caller is one.
    pub fn customer(&self) -> Option<CustomerId> {
        match self.actor {
            Actor::Customer(id) => Some(id),
            _ => None,
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
        assert_eq!(reader().require(Scope::ProductsRead), Ok(()));
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
    fn an_app_missing_a_scope_is_not_authorized_for_it() {
        let app = Ctx::new(
            Actor::App(AppId::from_uuid(Uuid::from_u128(1))),
            vec![Scope::OrdersRead],
        );
        assert_eq!(app.require(Scope::OrdersRead), Ok(()));
        assert_eq!(
            app.require(Scope::CustomerDataEmail),
            Err(AuthError::AppNotAuthorized(Scope::CustomerDataEmail))
        );
        assert!(!app.is_staff());
        assert!(reader().is_staff());
    }

    #[test]
    fn system_has_every_scope() {
        let ctx = Ctx::system();
        for scope in Scope::ALL {
            assert_eq!(ctx.require(scope), Ok(()), "{scope:?}");
        }
    }

    #[test]
    fn only_a_customer_actor_is_a_customer() {
        let id = CustomerId::from_uuid(Uuid::from_u128(2));
        assert_eq!(Ctx::new(Actor::Customer(id), vec![]).customer(), Some(id));
        assert_eq!(reader().customer(), None);
    }

    #[test]
    fn a_channel_comes_only_from_a_channel_context() {
        let channel = ChannelId::from_uuid(Uuid::from_u128(3));
        let actor = Actor::User(UserId::from_uuid(Uuid::from_u128(1)));
        assert_eq!(
            Ctx::in_channel(actor, vec![], channel).channel(),
            Some(channel)
        );
        assert_eq!(reader().channel(), None);
    }
}
