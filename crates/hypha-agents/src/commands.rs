//! Managing agents, their proposals and their call log. Staff manage agents with
//! `agents:*`; a proposal is made by the agent itself (`Actor::Agent`, the agent from the
//! context, never from input) and decided by a staff session holding its scopes.
//!
//! Credentials are not this crate's: a connected agent's OAuth pair and an autonomous
//! agent's secret are `hypha-auth` token kinds. A workflow above both makes the agent with
//! `prepare_autonomous_agent` / `prepare_connected_agent` and stages it (`StageAgents`)
//! in the same batch as its credential; `create_*` write an agent alone.

use core::time::Duration;

use hypha_core::{
    Actor, AgentCallId, AgentId, AuthError, Clock, Ctx, ErrorCode, IdGenerator, IdempotencyError,
    IdempotencyKey, Idempotent, Page, PageRequest, Progress, ProposalId, RepoError, Scope,
    Timestamp, UserId,
};
use serde_json::Value;
use thiserror::Error;

use crate::{
    Agent, AgentCall, AgentCallRepo, AgentClient, AgentError, AgentFilter, AgentKind, AgentRepo,
    CallError, Grant, GrantError, GrantInput, HasAgentCalls, HasAgents, HasProposals, NewAgent,
    NewAgentCall, NewProposal, OperationId, OperationIdError, Proposal, ProposalError,
    ProposalFilter, ProposalRepo, ProposalRequest, ProposalResponse, ProposalStatus, WriteMode,
    repo::keys,
};

/// How long proposals wait and the call log is kept (`agents.proposal_lifetime`,
/// `jobs.prune.agent_calls_after`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSettings {
    pub proposal_lifetime: Duration,
    pub keep_calls: Duration,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            // A week: long enough to span a weekend, short enough that a stale proposal
            // doesn't wait forever.
            proposal_lifetime: Duration::from_secs(7 * 24 * 3600),
            // As long as events and extension calls.
            keep_calls: Duration::from_secs(30 * 24 * 3600),
        }
    }
}

/// How often `record_agent_use` writes: agents call in bursts, and "last used" needs no
/// finer grain.
pub const USE_RESOLUTION: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error(transparent)]
    Grant(#[from] GrantError),
    #[error(transparent)]
    Operation(#[from] OperationIdError),
    #[error(transparent)]
    Proposal(#[from] ProposalError),
    #[error(transparent)]
    Call(#[from] CallError),
    #[error(transparent)]
    Idempotency(#[from] IdempotencyError),
    #[error("agent {0} not found")]
    AgentNotFound(AgentId),
    /// Staff turned the agent off: its credentials act again once it's enabled.
    #[error("agent {0} is disabled")]
    AgentDisabled(AgentId),
    #[error("proposal {0} not found")]
    ProposalNotFound(ProposalId),
    /// Nobody grants more than they hold.
    #[error("you can't grant {0}: you don't have it")]
    CannotGrant(Scope),
    /// `apply` lets an agent write without anyone approving: only someone who could
    /// approve (a staff member, through a session) grants it.
    #[error("only a staff member may grant {0} in apply mode")]
    CannotGrantApply(Scope),
    /// Connecting a client and deciding a proposal are a person's acts.
    #[error("this takes a staff member's session")]
    SessionRequired,
    /// Proposing is an agent's act.
    #[error("only an agent proposes")]
    AgentRequired,
    #[error(transparent)]
    Repo(#[from] RepoError),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Agent(e) => e.code(),
            CommandError::Grant(e) => e.code(),
            CommandError::Operation(e) => e.code(),
            CommandError::Proposal(e) => e.code(),
            CommandError::Call(e) => e.code(),
            CommandError::Idempotency(e) => e.code(),
            CommandError::AgentNotFound(_) => "agent_not_found",
            CommandError::AgentDisabled(_) => "agent_disabled",
            CommandError::ProposalNotFound(_) => "proposal_not_found",
            CommandError::CannotGrant(_) => "not_allowed_to_grant",
            CommandError::CannotGrantApply(_) => "not_allowed_to_grant_apply",
            CommandError::SessionRequired => "session_required",
            CommandError::AgentRequired => "agent_required",
            CommandError::Repo(e) => e.code(),
        }
    }
}

/// Fails unless `ctx` may hand out `grant`: it holds every scope the grant names, and only
/// a staff session grants `apply` (someone who could approve the same write).
pub fn require_grantable(ctx: &Ctx, grant: &Grant) -> Result<(), CommandError> {
    if let Some(missing) = grant.scopes().into_iter().find(|s| !ctx.has_scope(*s)) {
        return Err(CommandError::CannotGrant(missing));
    }
    if let Some(applied) = grant.writes_in(WriteMode::Apply).first()
        && !ctx.is_staff()
    {
        return Err(CommandError::CannotGrantApply(*applied));
    }
    Ok(())
}

async fn load_agent<R: AgentRepo>(repo: &R, id: AgentId) -> Result<Agent, CommandError> {
    repo.get(id).await?.ok_or(CommandError::AgentNotFound(id))
}

async fn load_proposal<R: ProposalRepo>(
    repo: &R,
    id: ProposalId,
) -> Result<Proposal, CommandError> {
    repo.get(id)
        .await?
        .ok_or(CommandError::ProposalNotFound(id))
}

/// An autonomous agent as staff create it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAgent {
    pub name: String,
    pub grant: GrantInput,
}

/// Checks and builds an autonomous agent without writing it, for a workflow that stages
/// it with its secret. Needs `agents:write` and a grantable grant.
pub fn prepare_autonomous_agent(
    ctx: &Ctx,
    ids: &impl IdGenerator,
    input: CreateAgent,
) -> Result<Agent, CommandError> {
    ctx.require(Scope::AgentsWrite)?;
    let grant = Grant::new(input.grant)?;
    require_grantable(ctx, &grant)?;
    Ok(Agent::new(
        AgentId::from_uuid(ids.new_uuid()),
        NewAgent {
            name: input.name,
            kind: AgentKind::Autonomous,
            client: None,
            grant,
        },
    )?)
}

/// Creates an autonomous agent: staff's, for a job, with its own grant.
pub async fn create_autonomous_agent<S: HasAgents>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    input: CreateAgent,
) -> Result<Agent, CommandError> {
    let mut agent = prepare_autonomous_agent(ctx, ids, input)?;
    ports.agents().insert(&mut agent).await?;
    Ok(agent)
}

/// The client that asked to connect, as its metadata document names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInput {
    pub client_id: String,
    pub name: String,
}

/// A connection a staff member approved: the agent will act for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectAgent {
    pub name: String,
    pub client: Option<ClientInput>,
    pub grant: GrantInput,
}

/// Checks and builds a connected agent without writing it, for the OAuth workflow that
/// stages it with the code it issues. The staff member is the caller (a session), never
/// input. No `agents:write`: connecting one's own client is a person's own act, and the
/// agent can never do more than their role allows (it's capped by it at every request);
/// the grant must still be one they could give.
pub fn prepare_connected_agent(
    ctx: &Ctx,
    ids: &impl IdGenerator,
    input: ConnectAgent,
) -> Result<Agent, CommandError> {
    let Actor::User(user) = ctx.actor() else {
        return Err(CommandError::SessionRequired);
    };
    let grant = Grant::new(input.grant)?;
    require_grantable(ctx, &grant)?;
    let client = input
        .client
        .map(|c| AgentClient::new(&c.client_id, &c.name))
        .transpose()?;
    Ok(Agent::new(
        AgentId::from_uuid(ids.new_uuid()),
        NewAgent {
            name: input.name,
            kind: AgentKind::Connected { user },
            client,
            grant,
        },
    )?)
}

/// Creates a connected agent for the calling staff member.
pub async fn create_connected_agent<S: HasAgents>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    input: ConnectAgent,
) -> Result<Agent, CommandError> {
    let mut agent = prepare_connected_agent(ctx, ids, input)?;
    ports.agents().insert(&mut agent).await?;
    Ok(agent)
}

/// Changes only what's `Some`, like a JSON Merge Patch. `grant` replaces the whole grant
/// (its allowlist included); `operations` then replaces just the allowlist (`Some(None)`
/// removes it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateAgent {
    pub name: Option<String>,
    pub grant: Option<GrantInput>,
    pub operations: Option<Option<Vec<String>>>,
    pub enabled: Option<bool>,
}

/// Whether two grants give the same scopes in the same modes (allowlists aside).
fn same_scopes(a: &Grant, b: &Grant) -> bool {
    a.read() == b.read() && a.write() == b.write() && a.customer_data() == b.customer_data()
}

pub async fn update_agent<S: HasAgents>(
    ctx: &Ctx,
    ports: &S,
    id: AgentId,
    input: UpdateAgent,
) -> Result<Agent, CommandError> {
    ctx.require(Scope::AgentsWrite)?;
    let mut agent = load_agent(ports.agents(), id).await?;
    let before = agent.snapshot();
    if let Some(name) = input.name {
        agent.rename(&name)?;
    }
    let mut grant = input.grant.map(Grant::new).transpose()?;
    if let Some(operations) = input.operations {
        let operations = operations
            .map(|ops| {
                ops.iter()
                    .map(OperationId::new)
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let base = grant.unwrap_or_else(|| agent.grant().clone());
        grant = Some(base.with_operations(operations)?);
    }
    if let Some(grant) = grant {
        // Narrowing the allowlist alone needs no scopes; changing scopes needs them all.
        if !same_scopes(&grant, agent.grant()) {
            require_grantable(ctx, &grant)?;
        }
        agent.set_grant(grant)?;
    }
    if let Some(enabled) = input.enabled {
        agent.set_enabled(enabled)?;
    }
    // Nothing to change writes nothing: no new version for a concurrent edit to lose to.
    if agent.snapshot() == before {
        return Ok(agent);
    }
    ports.agents().update(&mut agent).await?;
    Ok(agent)
}

/// Revokes an agent for good. Its credentials are refused from then on (the host checks
/// `active_agent` at every request); its pending proposals can't be approved.
pub async fn revoke_agent<S: HasAgents>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: AgentId,
) -> Result<Agent, CommandError> {
    ctx.require(Scope::AgentsWrite)?;
    let mut agent = load_agent(ports.agents(), id).await?;
    agent.revoke(clock.now())?;
    ports.agents().update(&mut agent).await?;
    Ok(agent)
}

pub async fn get_agent<S: HasAgents>(
    ctx: &Ctx,
    ports: &S,
    id: AgentId,
) -> Result<Agent, CommandError> {
    ctx.require(Scope::AgentsRead)?;
    load_agent(ports.agents(), id).await
}

pub async fn list_agents<S: HasAgents>(
    ctx: &Ctx,
    ports: &S,
    filter: &AgentFilter,
    page: PageRequest<AgentId>,
) -> Result<Page<Agent>, CommandError> {
    ctx.require(Scope::AgentsRead)?;
    Ok(ports.agents().list(filter, page).await?)
}

/// The agent a credential names, if it may act: found, not revoked, enabled. For the host
/// authenticating a request; no scope, like any credential check.
pub async fn active_agent<S: HasAgents>(ports: &S, id: AgentId) -> Result<Agent, CommandError> {
    let agent = load_agent(ports.agents(), id).await?;
    if agent.revoked_at().is_some() {
        return Err(AgentError::Revoked(id).into());
    }
    if !agent.enabled() {
        return Err(CommandError::AgentDisabled(id));
    }
    Ok(agent)
}

/// Notes that `agent` was used at `now`, at most once per `USE_RESOLUTION`.
pub async fn record_agent_use<S: HasAgents>(
    ports: &S,
    agent: &Agent,
    now: Timestamp,
) -> Result<(), RepoError> {
    let recent = agent
        .last_used_at()
        .is_some_and(|at| at > now.minus(USE_RESOLUTION));
    if recent {
        return Ok(());
    }
    ports.agents().record_use(agent.id(), now).await
}

/// A write an agent previewed and proposes, as the host hands it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Propose {
    pub operation: String,
    pub request: ProposalRequest,
    pub idempotency_key: Option<String>,
    pub preview: Value,
    pub digest: String,
    /// The scopes the operation needs.
    pub scopes: Vec<Scope>,
}

/// The earlier proposal made with the same key, if it was for the same write.
fn replay(
    found: Proposal,
    operation: &OperationId,
    request: &ProposalRequest,
) -> Result<Proposal, CommandError> {
    if found.operation() == operation && found.request() == request {
        Ok(found)
    } else {
        Err(IdempotencyError::KeyReused.into())
    }
}

/// Keeps a write the calling agent previewed, for a person to approve. The agent is the
/// caller's (`Actor::Agent`), active, holding every scope the write needs (in either
/// mode). With an idempotency key, a retry of the same write answers the first proposal
/// (`replayed`); the same key for another write is `idempotency_key_reused`.
pub async fn create_proposal<S: HasAgents + HasProposals>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    clock: &impl Clock,
    settings: &AgentSettings,
    input: Propose,
) -> Result<Idempotent<Proposal>, CommandError> {
    let Actor::Agent(agent) = ctx.actor() else {
        return Err(CommandError::AgentRequired);
    };
    for scope in &input.scopes {
        ctx.require(*scope)?;
    }
    let operation = OperationId::new(&input.operation)?;
    let key = input
        .idempotency_key
        .as_deref()
        .map(IdempotencyKey::new)
        .transpose()?;
    active_agent(ports, agent).await?;
    if let Some(key) = &key
        && let Some(found) = ports.proposals().find_by_key(agent, key).await?
    {
        return replay(found, &operation, &input.request).map(Idempotent::replayed);
    }
    let mut proposal = Proposal::new(
        ProposalId::from_uuid(ids.new_uuid()),
        NewProposal {
            agent,
            operation: operation.clone(),
            request: input.request.clone(),
            idempotency_key: key.clone(),
            preview: input.preview,
            digest: input.digest,
            scopes: input.scopes,
            expires_at: clock.now().plus(settings.proposal_lifetime),
        },
    )?;
    match ports.proposals().insert(&mut proposal).await {
        Ok(()) => Ok(Idempotent::fresh(proposal)),
        // A concurrent retry with the same key won: answer as if it came first.
        Err(RepoError::Duplicate(keys::PROPOSAL_KEY)) => {
            let found = match &key {
                Some(key) => ports.proposals().find_by_key(agent, key).await?,
                None => None,
            };
            match found {
                Some(found) => replay(found, &operation, &input.request).map(Idempotent::replayed),
                None => Err(RepoError::Duplicate(keys::PROPOSAL_KEY).into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// The staff member deciding `proposal`: a session holding every scope it needs.
fn decider(ctx: &Ctx, proposal: &Proposal) -> Result<UserId, CommandError> {
    let Actor::User(user) = ctx.actor() else {
        return Err(CommandError::SessionRequired);
    };
    for scope in proposal.scopes() {
        ctx.require(*scope)?;
    }
    Ok(user)
}

/// Checks that the caller may approve the proposal now and returns it for the host to
/// apply (exactly as previewed); the host then records the result with `record_applied`
/// or `record_failed`. Writes nothing. Refused for a revoked or disabled agent's.
pub async fn approve_proposal<S: HasAgents + HasProposals>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: ProposalId,
) -> Result<Proposal, CommandError> {
    let proposal = load_proposal(ports.proposals(), id).await?;
    decider(ctx, &proposal)?;
    proposal.check_decidable(clock.now())?;
    active_agent(ports, proposal.agent()).await?;
    Ok(proposal)
}

/// An HTTP answer as the host saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub status: u16,
    pub body: Value,
}

/// Records that the approved write committed, answering `outcome`.
pub async fn record_applied<S: HasProposals>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: ProposalId,
    outcome: Outcome,
) -> Result<Proposal, CommandError> {
    let mut proposal = load_proposal(ports.proposals(), id).await?;
    let user = decider(ctx, &proposal)?;
    let response = ProposalResponse::new(outcome.status, outcome.body)?;
    proposal.applied(user, clock.now(), response)?;
    ports.proposals().update(&mut proposal).await?;
    Ok(proposal)
}

/// Records that the approved write failed, with Hypha's error answer.
pub async fn record_failed<S: HasProposals>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: ProposalId,
    outcome: Outcome,
) -> Result<Proposal, CommandError> {
    let mut proposal = load_proposal(ports.proposals(), id).await?;
    let user = decider(ctx, &proposal)?;
    let response = ProposalResponse::new(outcome.status, outcome.body)?;
    proposal.failed(user, clock.now(), response)?;
    ports.proposals().update(&mut proposal).await?;
    Ok(proposal)
}

/// Rejects a pending proposal, with a note the agent can read.
pub async fn reject_proposal<S: HasProposals>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: ProposalId,
    note: Option<String>,
) -> Result<Proposal, CommandError> {
    let mut proposal = load_proposal(ports.proposals(), id).await?;
    let user = decider(ctx, &proposal)?;
    proposal.reject(user, clock.now(), note)?;
    ports.proposals().update(&mut proposal).await?;
    Ok(proposal)
}

/// Expires up to `batch` proposals past their lifetime: a job's step. One decided in the
/// meantime (a stale write) is left as decided.
pub async fn expire_proposals<S: HasProposals>(
    ports: &S,
    clock: &impl Clock,
    batch: u32,
) -> Result<Progress, RepoError> {
    let now = clock.now();
    let due = ports.proposals().due(now, batch).await?;
    let found = u32::try_from(due.len()).unwrap_or(u32::MAX);
    for mut proposal in due {
        if proposal.expire(now).is_err() {
            continue;
        }
        match ports.proposals().update(&mut proposal).await {
            Ok(()) | Err(RepoError::StaleVersion) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(Progress::of(found, batch))
}

/// A proposal: staff with `agents:read`, or the agent that made it (another agent's is
/// not found).
pub async fn get_proposal<S: HasProposals>(
    ctx: &Ctx,
    ports: &S,
    id: ProposalId,
) -> Result<Proposal, CommandError> {
    let own = match ctx.actor() {
        Actor::Agent(agent) => Some(agent),
        _ => {
            ctx.require(Scope::AgentsRead)?;
            None
        }
    };
    let proposal = load_proposal(ports.proposals(), id).await?;
    if own.is_some_and(|agent| agent != proposal.agent()) {
        return Err(CommandError::ProposalNotFound(id));
    }
    Ok(proposal)
}

/// Which proposals to list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListProposals {
    pub agent: Option<AgentId>,
    pub status: Option<ProposalStatus>,
    /// Only those the caller could approve: every scope of theirs held.
    pub approvable: bool,
}

/// Proposals, newest first: staff with `agents:read`, or an agent its own (whatever
/// `agent` says).
pub async fn list_proposals<S: HasProposals>(
    ctx: &Ctx,
    ports: &S,
    input: ListProposals,
    page: PageRequest<ProposalId>,
) -> Result<Page<Proposal>, CommandError> {
    let agent = match ctx.actor() {
        Actor::Agent(agent) => Some(agent),
        _ => {
            ctx.require(Scope::AgentsRead)?;
            input.agent
        }
    };
    let within = input.approvable.then(|| {
        Scope::ALL
            .into_iter()
            .filter(|s| ctx.has_scope(*s))
            .collect()
    });
    let filter = ProposalFilter {
        agent,
        status: input.status,
        within,
    };
    Ok(ports.proposals().list(&filter, page).await?)
}

/// Records one tool call, as the host saw it end.
pub async fn record_call<S: HasAgentCalls>(
    ports: &S,
    ids: &impl IdGenerator,
    call: NewAgentCall,
) -> Result<AgentCall, CommandError> {
    let call = AgentCall::new(AgentCallId::from_uuid(ids.new_uuid()), call)?;
    ports.agent_calls().insert(&call).await?;
    Ok(call)
}

/// An agent's calls, newest first.
pub async fn list_agent_calls<S: HasAgents + HasAgentCalls>(
    ctx: &Ctx,
    ports: &S,
    agent: AgentId,
    page: PageRequest<AgentCallId>,
) -> Result<Page<AgentCall>, CommandError> {
    ctx.require(Scope::AgentsRead)?;
    load_agent(ports.agents(), agent).await?;
    Ok(ports.agent_calls().list_for_agent(agent, page).await?)
}

/// Deletes calls older than `keep_calls`, up to `batch` of them: the prune job's step.
pub async fn prune_agent_calls<S: HasAgentCalls>(
    ports: &S,
    clock: &impl Clock,
    settings: &AgentSettings,
    batch: u32,
) -> Result<Progress, RepoError> {
    let before = clock.now().minus(settings.keep_calls);
    let done = ports.agent_calls().prune_before(before, batch).await?;
    Ok(Progress::of(done, batch))
}
