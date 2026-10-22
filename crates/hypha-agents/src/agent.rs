use hypha_core::{
    AgentId, CallbackUrl, PendingEvent, Recorded, Recorder, Stamped, Stamps, Subject, Timestamp,
    UserId, Version, Versioned, required_text,
};

use crate::{AgentCreated, AgentError, AgentRevoked, AgentUpdated, Grant, events::AGENT};

/// Whom an agent acts for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    /// A staff member connected an MCP client: it acts for them, capped at every request by
    /// their role, and stops when they're deactivated.
    Connected { user: UserId },
    /// Staff made it for a job: its own grant, no person behind it.
    Autonomous,
}

impl AgentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentKind::Connected { .. } => "connected",
            AgentKind::Autonomous => "autonomous",
        }
    }

    /// The staff member a connected agent acts for.
    pub fn user(&self) -> Option<UserId> {
        match self {
            AgentKind::Connected { user } => Some(*user),
            AgentKind::Autonomous => None,
        }
    }
}

/// The MCP client that connected an agent, as its metadata document names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentClient {
    client_id: CallbackUrl,
    name: String,
}

impl AgentClient {
    /// `client_id` is the URL of the client's metadata document (`https://`, or `http://`
    /// to localhost for development); the name is trimmed and required.
    pub fn new(client_id: &str, name: &str) -> Result<Self, AgentError> {
        Ok(Self {
            client_id: CallbackUrl::new(client_id)?,
            name: required_text("client name", name)?,
        })
    }

    pub fn client_id(&self) -> &str {
        self.client_id.as_str()
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// What a new agent is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgent {
    pub name: String,
    pub kind: AgentKind,
    pub client: Option<AgentClient>,
    pub grant: Grant,
}

/// An admin agent: a principal of its own (`Actor::Agent`), with what staff granted it.
/// Its credentials (an OAuth pair for a connected one, a secret for an autonomous one)
/// are `hypha-auth`'s, issued in the same batch as the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    id: AgentId,
    version: Version,
    stamps: Option<Stamps>,
    name: String,
    kind: AgentKind,
    client: Option<AgentClient>,
    grant: Grant,
    enabled: bool,
    last_used_at: Option<Timestamp>,
    revoked_at: Option<Timestamp>,
    events: Recorder,
    /// Fields changed since the last write, for one `agent.updated` listing them.
    changed: Vec<&'static str>,
}

impl Agent {
    /// A new agent, enabled.
    pub fn new(id: AgentId, new: NewAgent) -> Result<Self, AgentError> {
        let mut agent = Self {
            id,
            version: Version::NEW,
            stamps: None,
            name: required_text("name", &new.name)?,
            kind: new.kind,
            client: new.client,
            grant: new.grant,
            enabled: true,
            last_used_at: None,
            revoked_at: None,
            events: Recorder::default(),
            changed: Vec::new(),
        };
        let created = AgentCreated {
            agent: id,
            kind: agent.kind.as_str(),
            user: agent.kind.user(),
            client_id: agent.client.as_ref().map(|c| c.client_id().to_string()),
        };
        agent.events.record(agent.subject(), &created);
        Ok(agent)
    }

    fn subject(&self) -> Subject {
        Subject::new(AGENT, self.id.as_uuid())
    }

    fn live(&self) -> Result<(), AgentError> {
        match self.revoked_at {
            Some(_) => Err(AgentError::Revoked(self.id)),
            None => Ok(()),
        }
    }

    fn mark(&mut self, field: &'static str) {
        if !self.changed.contains(&field) {
            self.changed.push(field);
        }
    }

    /// Renames it; the same name changes nothing.
    pub fn rename(&mut self, name: &str) -> Result<(), AgentError> {
        self.live()?;
        let name = required_text("name", name)?;
        if name != self.name {
            self.name = name;
            self.mark("name");
        }
        Ok(())
    }

    /// Replaces the grant; the same grant changes nothing. Who may grant what is the
    /// commands' rule (`commands::require_grantable`).
    pub fn set_grant(&mut self, grant: Grant) -> Result<(), AgentError> {
        self.live()?;
        if grant != self.grant {
            self.grant = grant;
            self.mark("grant");
        }
        Ok(())
    }

    /// Turns it on or off: a disabled agent's credentials are refused until enabled again.
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), AgentError> {
        self.live()?;
        if enabled != self.enabled {
            self.enabled = enabled;
            self.mark("enabled");
        }
        Ok(())
    }

    /// Revokes it for good: its credentials stop working and it changes no more.
    pub fn revoke(&mut self, at: Timestamp) -> Result<(), AgentError> {
        self.live()?;
        self.revoked_at = Some(at);
        self.events
            .record(self.subject(), &AgentRevoked { agent: self.id });
        Ok(())
    }

    pub fn id(&self) -> AgentId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> AgentKind {
        self.kind
    }

    pub fn client(&self) -> Option<&AgentClient> {
        self.client.as_ref()
    }

    pub fn grant(&self) -> &Grant {
        &self.grant
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// When a request last authenticated as it (`AgentRepo::record_use`), roughly: hosts
    /// write it at most once a minute.
    pub fn last_used_at(&self) -> Option<Timestamp> {
        self.last_used_at
    }

    pub fn revoked_at(&self) -> Option<Timestamp> {
        self.revoked_at
    }

    /// Whether its credentials may act: enabled and not revoked.
    pub fn is_active(&self) -> bool {
        self.enabled && self.revoked_at.is_none()
    }

    pub fn snapshot(&self) -> AgentSnapshot {
        AgentSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            name: self.name.clone(),
            user: self.kind.user(),
            client: self.client.clone(),
            grant: self.grant.clone(),
            enabled: self.enabled,
            last_used_at: self.last_used_at,
            revoked_at: self.revoked_at,
        }
    }

    /// Rebuilds an agent from storage, with the same checks as `new`.
    pub fn restore(s: AgentSnapshot) -> Result<Agent, AgentError> {
        let kind = match s.user {
            Some(user) => AgentKind::Connected { user },
            None => AgentKind::Autonomous,
        };
        let mut agent = Agent::new(
            s.id,
            NewAgent {
                name: s.name,
                kind,
                client: s.client,
                grant: s.grant,
            },
        )?;
        agent.version = s.version;
        agent.stamps = s.stamps;
        agent.enabled = s.enabled;
        agent.last_used_at = s.last_used_at;
        agent.revoked_at = s.revoked_at;
        agent.events = Recorder::default();
        Ok(agent)
    }
}

/// Plain-data form of an `Agent` for storage adapters. `user` is set for a connected agent
/// and only for one, which is its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSnapshot {
    pub id: AgentId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
    pub user: Option<UserId>,
    pub client: Option<AgentClient>,
    pub grant: Grant,
    pub enabled: bool,
    pub last_used_at: Option<Timestamp>,
    pub revoked_at: Option<Timestamp>,
}

impl Recorded for Agent {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let mut events = self.events.take();
        if !self.changed.is_empty() {
            let updated = AgentUpdated {
                agent: self.id,
                changed: std::mem::take(&mut self.changed),
            };
            events.push(PendingEvent::new(self.subject(), &updated));
        }
        events
    }
}

impl Versioned for Agent {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Agent {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{CallbackUrlError, Scope, TextError};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::{GrantInput, WriteMode};

    fn id() -> AgentId {
        AgentId::from_uuid(Uuid::from_u128(1))
    }

    fn user() -> UserId {
        UserId::from_uuid(Uuid::from_u128(7))
    }

    fn new(name: &str, kind: AgentKind) -> NewAgent {
        NewAgent {
            name: name.to_string(),
            kind,
            client: None,
            grant: Grant::default(),
        }
    }

    fn agent() -> Agent {
        let mut a = Agent::new(id(), new("Reorder bot", AgentKind::Autonomous)).unwrap();
        a.take_events();
        a
    }

    fn data(events: &[PendingEvent]) -> Vec<(&'static str, serde_json::Value)> {
        events
            .iter()
            .map(|e| (e.event_type, e.data.clone()))
            .collect()
    }

    #[test]
    fn a_name_is_required_and_trimmed() {
        assert_eq!(
            Agent::new(id(), new(" ", AgentKind::Autonomous)),
            Err(AgentError::Text(TextError::Missing("name")))
        );
        assert_eq!(agent().name(), "Reorder bot");
        assert!(agent().is_active());
    }

    #[test]
    fn a_client_is_its_metadata_url_and_a_name() {
        let c = AgentClient::new("https://claude.ai/oauth/client.json", " Claude ").unwrap();
        assert_eq!(c.client_id(), "https://claude.ai/oauth/client.json");
        assert_eq!(c.name(), "Claude");
        assert_eq!(
            AgentClient::new("ftp://claude.ai/x", "Claude"),
            Err(AgentError::ClientId(CallbackUrlError(
                "ftp://claude.ai/x".to_string()
            )))
        );
        assert_eq!(
            AgentClient::new("https://claude.ai/x", ""),
            Err(AgentError::Text(TextError::Missing("client name")))
        );
    }

    #[test]
    fn creating_records_its_kind_and_person() {
        let mut a = Agent::new(
            id(),
            NewAgent {
                client: Some(AgentClient::new("https://claude.ai/c.json", "Claude").unwrap()),
                ..new("Claude", AgentKind::Connected { user: user() })
            },
        )
        .unwrap();
        assert_eq!(
            data(&a.take_events()),
            [(
                "agent.created",
                json!({
                    "agent": id().to_string(),
                    "kind": "connected",
                    "user": user().to_string(),
                    "client_id": "https://claude.ai/c.json"
                })
            )]
        );
    }

    #[test]
    fn changes_in_one_write_are_one_event_listing_them() {
        let mut a = agent();
        a.rename("Restock").unwrap();
        a.set_enabled(false).unwrap();
        a.rename("Restocker").unwrap();
        let grant = Grant::from_parts(
            vec![Scope::OrdersRead],
            vec![(Scope::InventoryWrite, WriteMode::Propose)],
            vec![],
            None,
        )
        .unwrap();
        a.set_grant(grant).unwrap();
        assert_eq!(
            data(&a.take_events()),
            [(
                "agent.updated",
                json!({"agent": id().to_string(), "changed": ["name", "enabled", "grant"]})
            )]
        );
        assert!(a.take_events().is_empty());
        assert!(!a.is_active());
    }

    #[test]
    fn the_same_values_change_nothing() {
        let mut a = agent();
        a.rename(" Reorder bot ").unwrap();
        a.set_enabled(true).unwrap();
        a.set_grant(Grant::default()).unwrap();
        assert!(a.take_events().is_empty());
    }

    #[test]
    fn a_revoked_agent_changes_no_more() {
        let mut a = agent();
        let at = Timestamp::from_unix_micros(5);
        a.revoke(at).unwrap();
        assert_eq!(a.revoked_at(), Some(at));
        assert!(!a.is_active());
        assert_eq!(
            data(&a.take_events()),
            [("agent.revoked", json!({"agent": id().to_string()}))]
        );
        let revoked = Err(AgentError::Revoked(id()));
        assert_eq!(a.revoke(at), revoked);
        assert_eq!(a.rename("x"), revoked);
        assert_eq!(a.set_enabled(true), revoked);
        assert_eq!(a.set_grant(Grant::default()), revoked);
    }

    #[test]
    fn restore_round_trips() {
        let grant = Grant::new(GrantInput {
            read: vec!["orders:read".to_string()],
            write: vec![("products:write".to_string(), "apply".to_string())],
            customer_data: vec!["customer_data:email".to_string()],
            operations: Some(vec!["admin.orders.list".to_string()]),
        })
        .unwrap();
        let mut a = Agent::new(
            id(),
            NewAgent {
                name: "Claude".to_string(),
                kind: AgentKind::Connected { user: user() },
                client: Some(AgentClient::new("https://claude.ai/c.json", "Claude").unwrap()),
                grant,
            },
        )
        .unwrap();
        a.set_enabled(false).unwrap();
        a.revoke(Timestamp::from_unix_micros(9)).unwrap();
        a.set_version(Version::new(3));
        a.last_used_at = Some(Timestamp::from_unix_micros(8));
        a.take_events();
        assert_eq!(Agent::restore(a.snapshot()), Ok(a));
    }
}
