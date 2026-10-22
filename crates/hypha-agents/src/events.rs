//! What happens to agents and their proposals, as events. Contract types: a renamed or
//! reshaped field is a new `VERSION`, never an edit. `proposal.created` and
//! `proposal.decided` let an app or a webhook tell staff there's something to approve.

use hypha_core::{AgentId, Event, ProposalId, UserId};
use serde::Serialize;

pub const AGENT: &str = "agent";
pub const PROPOSAL: &str = "proposal";

/// `agent.created`: staff made an autonomous agent, or connected a client.
#[derive(Debug, Serialize)]
pub struct AgentCreated {
    pub agent: AgentId,
    /// `connected` or `autonomous`.
    pub kind: &'static str,
    /// The staff member a connected agent acts for.
    pub user: Option<UserId>,
    /// The client's ID (its metadata document's URL), when a client connected it.
    pub client_id: Option<String>,
}

impl Event for AgentCreated {
    const TYPE: &'static str = "agent.created";
    const VERSION: u16 = 1;
}

/// `agent.updated`: renamed, its grant changed, enabled or disabled. One event per write,
/// listing what changed.
#[derive(Debug, Serialize)]
pub struct AgentUpdated {
    pub agent: AgentId,
    /// `name`, `grant`, `enabled`.
    pub changed: Vec<&'static str>,
}

impl Event for AgentUpdated {
    const TYPE: &'static str = "agent.updated";
    const VERSION: u16 = 1;
}

/// `agent.revoked`: the agent's credentials stop working, for good.
#[derive(Debug, Serialize)]
pub struct AgentRevoked {
    pub agent: AgentId,
}

impl Event for AgentRevoked {
    const TYPE: &'static str = "agent.revoked";
    const VERSION: u16 = 1;
}

/// `proposal.created`: an agent proposed a write; a staff member holding `scopes` may
/// approve it.
#[derive(Debug, Serialize)]
pub struct ProposalCreated {
    pub proposal: ProposalId,
    pub agent: AgentId,
    pub operation: String,
    pub scopes: Vec<&'static str>,
}

impl Event for ProposalCreated {
    const TYPE: &'static str = "proposal.created";
    const VERSION: u16 = 1;
}

/// `proposal.updated`: approving found the store changed since the preview, so the
/// proposal now holds what the write would do today, for a person to review again.
#[derive(Debug, Serialize)]
pub struct ProposalUpdated {
    pub proposal: ProposalId,
    pub agent: AgentId,
    /// How many times it was previewed again.
    pub refreshes: u32,
    /// Who tried to approve it.
    pub by: UserId,
}

impl Event for ProposalUpdated {
    const TYPE: &'static str = "proposal.updated";
    const VERSION: u16 = 1;
}

/// `proposal.decided`: `applied`, `failed` or `rejected` by a staff member, or `expired`
/// (no one).
#[derive(Debug, Serialize)]
pub struct ProposalDecided {
    pub proposal: ProposalId,
    pub agent: AgentId,
    pub status: &'static str,
    pub decided_by: Option<UserId>,
}

impl Event for ProposalDecided {
    const TYPE: &'static str = "proposal.decided";
    const VERSION: u16 = 1;
}
