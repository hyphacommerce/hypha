use hypha_core::{
    AgentCallId, AgentId, IdempotencyKey, Page, PageRequest, ProposalId, RepoError, Scope,
    Timestamp, UserId,
};

use crate::{Agent, AgentCall, Proposal, ProposalStatus};

/// Uniqueness rules the agents' stores enforce, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const AGENT_ID: &str = "agent_id";
    pub const PROPOSAL_ID: &str = "proposal_id";
    /// One proposal per agent and idempotency key, so an agent's retry finds the first.
    pub const PROPOSAL_KEY: &str = "proposal_idempotency_key";
    pub const AGENT_CALL_ID: &str = "agent_call_id";
}

/// Which agents a list holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentFilter {
    /// Only the agents connected for this staff member.
    pub user: Option<UserId>,
    /// Revoked agents too (left out by default).
    pub include_revoked: bool,
}

#[allow(async_fn_in_trait)]
pub trait AgentRepo {
    async fn get(&self, id: AgentId) -> Result<Option<Agent>, RepoError>;
    /// Newest first.
    async fn list(
        &self,
        filter: &AgentFilter,
        page: PageRequest<AgentId>,
    ) -> Result<Page<Agent>, RepoError>;
    /// Stores a new agent (version 1). Enforces `AGENT_ID`.
    async fn insert(&self, agent: &mut Agent) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`. Leaves
    /// `last_used_at` as stored: only `record_use` writes it.
    async fn update(&self, agent: &mut Agent) -> Result<(), RepoError>;
    /// Notes that a request authenticated as the agent at `at`, unless a later use is
    /// stored. Bumps no version and records no event: it would make every staff edit lose
    /// to the agent's own traffic. A missing agent is ignored.
    async fn record_use(&self, id: AgentId, at: Timestamp) -> Result<(), RepoError>;
}

/// Which proposals a list holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProposalFilter {
    pub agent: Option<AgentId>,
    pub status: Option<ProposalStatus>,
    /// Only those every scope of which is in this list: the ones a caller holding these
    /// could approve.
    pub within: Option<Vec<Scope>>,
}

#[allow(async_fn_in_trait)]
pub trait ProposalRepo {
    async fn get(&self, id: ProposalId) -> Result<Option<Proposal>, RepoError>;
    /// The agent's proposal made with `key`, if any.
    async fn find_by_key(
        &self,
        agent: AgentId,
        key: &IdempotencyKey,
    ) -> Result<Option<Proposal>, RepoError>;
    /// Newest first.
    async fn list(
        &self,
        filter: &ProposalFilter,
        page: PageRequest<ProposalId>,
    ) -> Result<Page<Proposal>, RepoError>;
    /// Up to `limit` pending proposals whose lifetime ended at or before `now`, the
    /// earliest to expire first.
    async fn due(&self, now: Timestamp, limit: u32) -> Result<Vec<Proposal>, RepoError>;
    /// Stores a new proposal (version 1). Enforces `keys`.
    ///
    /// Precondition: its agent exists. Otherwise adapters fail rather than store a
    /// proposal no one made.
    async fn insert(&self, proposal: &mut Proposal) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, proposal: &mut Proposal) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait AgentCallRepo {
    /// Stores a call. IDs are new for every call; a duplicate is the caller's bug and
    /// fails with `Duplicate(keys::AGENT_CALL_ID)`.
    async fn insert(&self, call: &AgentCall) -> Result<(), RepoError>;
    /// An agent's calls, newest first (by ID: UUIDv7, so by time).
    async fn list_for_agent(
        &self,
        agent: AgentId,
        page: PageRequest<AgentCallId>,
    ) -> Result<Page<AgentCall>, RepoError>;
    /// Deletes up to `limit` calls made before `before`; how many it deleted.
    async fn prune_before(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError>;
}

/// Stages agents in a `hypha_core::HasBatch` batch: an agent and its credentials
/// (`hypha-auth`'s) are written together, so none exists without the other.
pub trait StageAgents {
    /// What `AgentRepo::insert` would write; `AGENT_ID` on commit.
    fn insert_agent(&mut self, agent: &mut Agent);
}

pub trait HasAgents {
    type Agents: AgentRepo;
    fn agents(&self) -> &Self::Agents;
}

impl<T: HasAgents + ?Sized> HasAgents for &T {
    type Agents = T::Agents;
    fn agents(&self) -> &Self::Agents {
        (**self).agents()
    }
}

pub trait HasProposals {
    type Proposals: ProposalRepo;
    fn proposals(&self) -> &Self::Proposals;
}

impl<T: HasProposals + ?Sized> HasProposals for &T {
    type Proposals = T::Proposals;
    fn proposals(&self) -> &Self::Proposals {
        (**self).proposals()
    }
}

pub trait HasAgentCalls {
    type AgentCalls: AgentCallRepo;
    fn agent_calls(&self) -> &Self::AgentCalls;
}

impl<T: HasAgentCalls + ?Sized> HasAgentCalls for &T {
    type AgentCalls = T::AgentCalls;
    fn agent_calls(&self) -> &Self::AgentCalls {
        (**self).agent_calls()
    }
}
