//! Admin agents: principals of their own (`Actor::Agent`) that act on the admin surface
//! through MCP. A connected agent acts for the staff member who connected its client,
//! capped by their role at every request; an autonomous one is staff's, for a job. What
//! each may do is its `Grant`: read scopes, write scopes each to `propose` or `apply`,
//! personal data, an optional allowlist of operations; `effective` is what that comes to
//! at a request under the operator's cap. A write an agent may only propose is kept as a
//! `Proposal` for a person to approve. Every tool call is logged (`AgentCall`).
//!
//! A leaf domain: it depends on core only. Credentials are `hypha-auth`'s; previews and
//! the MCP protocol are the host's.

pub mod agent;
pub mod call;
pub mod commands;
pub mod error;
pub mod events;
pub mod grant;
pub mod proposal;
pub mod repo;

pub use agent::{Agent, AgentClient, AgentKind, AgentSnapshot, NewAgent};
pub use call::{AgentCall, CallMode, MAX_ERROR_CODE_LEN, NewAgentCall};
pub use commands::{
    AgentSettings, ClientInput, CommandError, ConnectAgent, CreateAgent, ListProposals, Outcome,
    Propose, USE_RESOLUTION, UpdateAgent, active_agent, approve_proposal, create_autonomous_agent,
    create_connected_agent, create_proposal, expire_proposals, get_agent, get_proposal,
    list_agent_calls, list_agents, list_proposals, prepare_autonomous_agent,
    prepare_connected_agent, prune_agent_calls, record_agent_use, record_applied, record_call,
    record_failed, record_refreshed, reject_proposal, require_grantable, revoke_agent,
    update_agent,
};
pub use error::{AgentError, CallError, ProposalError};
pub use events::{
    AGENT, AgentCreated, AgentRevoked, AgentUpdated, PROPOSAL, ProposalCreated, ProposalDecided,
    ProposalUpdated,
};
pub use grant::{
    Access, Grant, GrantError, GrantField, GrantInput, MAX_OPERATIONS, NEVER_GRANTED, PROPOSE_ONLY,
    Permissions, WriteMode, WritesCap, effective, suggested_grant,
};
pub use hypha_core::{OperationId, OperationIdError};
pub use proposal::{
    MAX_DIGEST_LEN, MAX_NOTE_LEN, MAX_PREVIEW_BYTES, MAX_REQUEST_BYTES, NewProposal, Proposal,
    ProposalRequest, ProposalResponse, ProposalSnapshot, ProposalStatus,
};
pub use repo::{
    AgentCallRepo, AgentFilter, AgentRepo, HasAgentCalls, HasAgents, HasProposals, ProposalFilter,
    ProposalRepo, StageAgents, StageProposals, keys,
};
