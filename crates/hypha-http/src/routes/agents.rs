//! Admin agents and their proposals. Agents themselves call through `/mcp` (the
//! `agents` module); these routes are for staff: making autonomous agents (their key
//! shown once), changing grants, disabling and revoking, reading the call log, and the
//! proposals inbox, where a staff member holding a proposal's scopes approves or rejects
//! it.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hypha_agents::{
    Agent, AgentCall, AgentFilter, CreateAgent, GrantInput, ListProposals, Outcome, Proposal,
    ProposalStatus, UpdateAgent,
};
use hypha_auth::{Identity, NewToken, StageIdentities, TokenKind};
use hypha_core::{
    AgentCallId, AgentId, HasBatch, IdGenerator, IdentityId, ProposalId, Scope, Stamped, UserId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    page::{ListQuery, PageParams, PageResponse},
    rfc3339, stamp_times,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId, StaffCaller},
    patch,
    state::{SharedState, Storage},
};

/// A grant as staff send it: scope names, each write scope with its mode.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentGrantRequest {
    /// Read scopes (`orders:read`).
    #[serde(default)]
    read: Vec<String>,
    /// Write scopes, each `propose` (previewed and kept for a person to approve) or
    /// `apply` (written at once; only a staff session grants it).
    #[serde(default)]
    write: BTreeMap<String, String>,
    /// Personal data the agent may see (`customer_data:email`); none by default.
    #[serde(default)]
    customer_data: Vec<String>,
    /// Only these operations (`admin.orders.list`); `null` or absent: every operation
    /// its scopes allow.
    #[serde(default)]
    operations: Option<Vec<String>>,
}

impl AgentGrantRequest {
    fn input(self) -> GrantInput {
        GrantInput {
            read: self.read,
            write: self.write.into_iter().collect(),
            customer_data: self.customer_data,
            operations: self.operations,
        }
    }
}

/// What an agent was granted, as staff set it. What it may do at a request is this,
/// under the deployment's `writes_cap`, within its staff member's role for a connected
/// agent.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AgentGrantResponse {
    read: Vec<Scope>,
    /// Each write scope's mode: `propose` or `apply`.
    write: BTreeMap<String, String>,
    customer_data: Vec<Scope>,
    operations: Option<Vec<String>>,
}

impl AgentGrantResponse {
    fn new(grant: &hypha_agents::Grant) -> Self {
        Self {
            read: grant.read().to_vec(),
            write: grant
                .write()
                .iter()
                .map(|(scope, mode)| (scope.as_str().to_string(), mode.as_str().to_string()))
                .collect(),
            customer_data: grant.customer_data().to_vec(),
            operations: grant
                .operations()
                .map(|ops| ops.iter().map(|o| o.as_str().to_string()).collect()),
        }
    }
}

/// The client a staff member connected (its metadata document's URL and name).
#[derive(Serialize, utoipa::ToSchema)]
pub struct AgentClientResponse {
    client_id: String,
    name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AgentResponse {
    id: AgentId,
    name: String,
    /// `connected` (acts for `user`) or `autonomous`.
    kind: &'static str,
    /// The staff member a connected agent acts for.
    user: Option<UserId>,
    client: Option<AgentClientResponse>,
    grant: AgentGrantResponse,
    /// The most any agent may write in this deployment (`agents.writes`): `apply`,
    /// `propose` or `off`. It wins over the grant.
    writes_cap: &'static str,
    enabled: bool,
    last_used_at: Option<String>,
    revoked_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl AgentResponse {
    fn new(agent: &Agent, cap: hypha_agents::WritesCap) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(agent.stamps())?;
        Ok(Self {
            id: agent.id(),
            name: agent.name().to_string(),
            kind: agent.kind().as_str(),
            user: agent.kind().user(),
            client: agent.client().map(|c| AgentClientResponse {
                client_id: c.client_id().to_string(),
                name: c.name().to_string(),
            }),
            grant: AgentGrantResponse::new(agent.grant()),
            writes_cap: cap.as_str(),
            enabled: agent.enabled(),
            last_used_at: agent.last_used_at().map(rfc3339).transpose()?,
            revoked_at: agent.revoked_at().map(rfc3339).transpose()?,
            created_at,
            updated_at,
        })
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAgentRequest {
    name: String,
    grant: AgentGrantRequest,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CreatedAgentResponse {
    #[serde(flatten)]
    agent: AgentResponse,
    /// The agent's secret key (`hypha_ak_…`), shown only in this response: the bearer
    /// token its MCP client sends to `/mcp`, and nowhere else.
    key: String,
}

/// Makes an autonomous agent: staff's, for a job, with its own grant (no more than the
/// caller holds; `apply` only from a staff session). Its key is shown only here.
#[utoipa::path(
    post,
    path = "/admin/agents",
    request_body = CreateAgentRequest,
    responses((status = 201, description = "Created", body = CreatedAgentResponse)),
    extensions(("x-hypha-scopes" = json!(["agents:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateAgentRequest>,
) -> Result<(StatusCode, Json<CreatedAgentResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateAgent {
        name: req.name,
        grant: req.grant.input(),
    };
    let mut agent = hypha_agents::prepare_autonomous_agent(&ctx, &state.ids, input)?;
    let key = NewToken::generate(TokenKind::AgentKey, &state.env);
    let mut identity = Identity::with_agent_key(
        IdentityId::from_uuid(state.ids.new_uuid()),
        agent.id(),
        &key,
    );
    // The agent and its key together: neither exists without the other.
    let mut batch = ports.batch();
    hypha_agents::StageAgents::insert_agent(&mut batch, &mut agent);
    batch.insert_identity(&mut identity);
    ports.commit(batch).await?;
    let response = CreatedAgentResponse {
        agent: AgentResponse::new(&agent, state.config.agents.writes)?,
        key: key.expose().to_string(),
    };
    Ok((StatusCode::CREATED, Json(response)))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct AgentFilters {
    /// Only the agents connected for this staff member.
    #[param(value_type = Option<UserId>)]
    user: Option<UserId>,
    /// Revoked agents too (left out by default).
    include_revoked: Option<bool>,
}

type AgentPage = PageResponse<AgentResponse>;

/// Agents, newest first: connected and autonomous.
#[utoipa::path(
    get,
    path = "/admin/agents",
    params(PageParams, AgentFilters),
    responses((status = 200, description = "OK", body = AgentPage)),
    extensions(("x-hypha-scopes" = json!(["agents:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<AgentFilters>,
) -> Result<Json<AgentPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse()?;
    let filter = AgentFilter {
        user: filters.user,
        include_revoked: filters.include_revoked.unwrap_or(false),
    };
    let page = hypha_agents::list_agents(&ctx, &ports, &filter, page).await?;
    let cap = state.config.agents.writes;
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |a| a.id(),
        |a| AgentResponse::new(&a, cap),
    )?))
}

/// One agent.
#[utoipa::path(
    get,
    path = "/admin/agents/{id}",
    params(("id" = AgentId, Path)),
    responses((status = 200, description = "OK", body = AgentResponse)),
    extensions(("x-hypha-scopes" = json!(["agents:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<AgentId>,
) -> Result<Json<AgentResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let agent = hypha_agents::get_agent(&ctx, &ports, id).await?;
    Ok(Json(AgentResponse::new(
        &agent,
        state.config.agents.writes,
    )?))
}

/// A JSON Merge Patch of an agent.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAgentRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    name: Option<String>,
    /// Replaces the whole grant, its allowlist included.
    #[serde(default, deserialize_with = "patch::non_null")]
    grant: Option<AgentGrantRequest>,
    /// Replaces just the allowlist; `null` removes it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<Vec<String>>)]
    operations: Option<Option<Vec<String>>>,
    /// `false` stops its credentials until it's enabled again.
    #[serde(default, deserialize_with = "patch::non_null")]
    enabled: Option<bool>,
}

/// Renames an agent, changes its grant (no more than the caller holds; `apply` only from
/// a staff session) or its allowlist, disables or enables it.
#[utoipa::path(
    patch,
    path = "/admin/agents/{id}",
    params(("id" = AgentId, Path)),
    request_body = UpdateAgentRequest,
    responses((status = 200, description = "OK", body = AgentResponse)),
    extensions(("x-hypha-scopes" = json!(["agents:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<AgentId>,
    body: JsonBody<UpdateAgentRequest>,
) -> Result<Json<AgentResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateAgent {
        name: req.name,
        grant: req.grant.map(AgentGrantRequest::input),
        operations: req.operations,
        enabled: req.enabled,
    };
    let agent = hypha_agents::update_agent(&ctx, &ports, id, input).await?;
    Ok(Json(AgentResponse::new(
        &agent,
        state.config.agents.writes,
    )?))
}

/// Revokes an agent for good: its credentials stop at once, its pending proposals can't
/// be approved. 409 if it already was.
#[utoipa::path(
    post,
    path = "/admin/agents/{id}/revoke",
    params(("id" = AgentId, Path)),
    responses((status = 200, description = "OK", body = AgentResponse)),
    extensions(("x-hypha-scopes" = json!(["agents:write"]))),
)]
pub async fn revoke<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<AgentId>,
) -> Result<Json<AgentResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let agent = hypha_agents::revoke_agent(&ctx, &ports, &state.clock, id).await?;
    Ok(Json(AgentResponse::new(
        &agent,
        state.config.agents.writes,
    )?))
}

/// One call an agent made.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AgentCallResponse {
    id: AgentCallId,
    /// The operation's ID (`admin.orders.list`).
    operation: String,
    /// `read`, `preview`, `propose` or `apply`.
    mode: &'static str,
    /// The HTTP status it answered.
    status: u16,
    /// Its error's code, for a failure.
    error_code: Option<String>,
    duration_ms: u32,
    created_at: String,
}

impl AgentCallResponse {
    fn new(call: &AgentCall) -> Result<Self, ApiError> {
        Ok(Self {
            id: call.id(),
            operation: call.operation().as_str().to_string(),
            mode: call.mode().as_str(),
            status: call.status(),
            error_code: call.error_code().map(str::to_string),
            duration_ms: call.duration_ms(),
            created_at: rfc3339(call.created_at())?,
        })
    }
}

type AgentCallPage = PageResponse<AgentCallResponse>;

/// What an agent called, newest first, reads too (no bodies: they may hold personal
/// data). Kept for `jobs.prune.agent_calls_after`.
#[utoipa::path(
    get,
    path = "/admin/agents/{id}/calls",
    params(("id" = AgentId, Path), PageParams),
    responses((status = 200, description = "OK", body = AgentCallPage)),
    extensions(("x-hypha-scopes" = json!(["agents:read"]))),
)]
pub async fn calls<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<AgentId>,
    query: ListQuery,
) -> Result<Json<AgentCallPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, _, scope) = query.parse()?;
    let page = hypha_agents::list_agent_calls(&ctx, &ports, id, page).await?;
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |c| c.id(),
        |c| AgentCallResponse::new(&c),
    )?))
}

/// The write a proposal keeps: everything to send it again.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ProposalRequestResponse {
    /// Its path parameters, by name.
    #[schema(value_type = Object)]
    params: serde_json::Map<String, Value>,
    #[schema(value_type = Object)]
    query: serde_json::Map<String, Value>,
    #[schema(value_type = Option<Object>)]
    body: Option<Value>,
}

/// What an approved proposal's write answered (Hypha's error body for a failure).
#[derive(Serialize, utoipa::ToSchema)]
pub struct ProposalOutcomeResponse {
    status: u16,
    #[schema(value_type = Object)]
    body: Value,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ProposalResponse {
    id: ProposalId,
    agent: AgentId,
    /// The operation it would call (`admin.products.update`).
    operation: String,
    request: ProposalRequestResponse,
    idempotency_key: Option<String>,
    /// The write's preview when it was proposed (`#/components/schemas/PreviewBody`,
    /// without a token): its response, events, emails and calls, or what blocked it.
    #[schema(value_type = Object)]
    preview: Value,
    /// Every scope approving it takes.
    scopes: Vec<Scope>,
    /// `pending`, `applied`, `rejected`, `expired` or `failed` (approved, but its write
    /// failed: stale, a conflict, an app's refusal; `response` says why).
    status: &'static str,
    decided_by: Option<UserId>,
    decided_at: Option<String>,
    /// Why it was rejected, for the agent.
    note: Option<String>,
    response: Option<ProposalOutcomeResponse>,
    expires_at: String,
    created_at: String,
    updated_at: String,
}

impl ProposalResponse {
    pub(crate) fn new(proposal: &Proposal) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(proposal.stamps())?;
        let mut preview = proposal.preview().clone();
        // The IDs the approval reuses: the host's, not the reader's.
        if let Value::Object(fields) = &mut preview {
            fields.remove("ids");
        }
        Ok(Self {
            id: proposal.id(),
            agent: proposal.agent(),
            operation: proposal.operation().as_str().to_string(),
            request: ProposalRequestResponse {
                params: proposal.request().params.clone(),
                query: proposal.request().query.clone(),
                body: proposal.request().body.clone(),
            },
            idempotency_key: proposal.idempotency_key().map(|k| k.as_str().to_string()),
            preview,
            scopes: proposal.scopes().to_vec(),
            status: proposal.status().as_str(),
            decided_by: proposal.decided_by(),
            decided_at: proposal.decided_at().map(rfc3339).transpose()?,
            note: proposal.note().map(str::to_string),
            response: proposal.response().map(|r| ProposalOutcomeResponse {
                status: r.status(),
                body: r.body().clone(),
            }),
            expires_at: rfc3339(proposal.expires_at())?,
            created_at,
            updated_at,
        })
    }
}

#[derive(Deserialize, Clone, Copy, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatusRequest {
    Pending,
    Applied,
    Rejected,
    Expired,
    Failed,
}

impl From<ProposalStatusRequest> for ProposalStatus {
    fn from(s: ProposalStatusRequest) -> Self {
        match s {
            ProposalStatusRequest::Pending => ProposalStatus::Pending,
            ProposalStatusRequest::Applied => ProposalStatus::Applied,
            ProposalStatusRequest::Rejected => ProposalStatus::Rejected,
            ProposalStatusRequest::Expired => ProposalStatus::Expired,
            ProposalStatusRequest::Failed => ProposalStatus::Failed,
        }
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ProposalFilters {
    /// Only this agent's.
    #[param(value_type = Option<AgentId>)]
    agent: Option<AgentId>,
    #[param(inline)]
    status: Option<ProposalStatusRequest>,
    /// Only those the caller could approve: every scope of theirs held.
    approvable: Option<bool>,
}

type ProposalPage = PageResponse<ProposalResponse>;

/// Agents' proposals, newest first: the inbox.
#[utoipa::path(
    get,
    path = "/admin/agent-proposals",
    params(PageParams, ProposalFilters),
    responses((status = 200, description = "OK", body = ProposalPage)),
    extensions(("x-hypha-scopes" = json!(["agents:read"]))),
)]
pub async fn list_proposals<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<ProposalFilters>,
) -> Result<Json<ProposalPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse()?;
    let input = ListProposals {
        agent: filters.agent,
        status: filters.status.map(Into::into),
        approvable: filters.approvable.unwrap_or(false),
    };
    let page = hypha_agents::list_proposals(&ctx, &ports, input, page).await?;
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |p| p.id(),
        |p| ProposalResponse::new(&p),
    )?))
}

/// One proposal, with the preview it was made from.
#[utoipa::path(
    get,
    path = "/admin/agent-proposals/{id}",
    params(("id" = ProposalId, Path)),
    responses((status = 200, description = "OK", body = ProposalResponse)),
    extensions(("x-hypha-scopes" = json!(["agents:read"]))),
)]
pub async fn fetch_proposal<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProposalId>,
) -> Result<Json<ProposalResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let proposal = hypha_agents::get_proposal(&ctx, &ports, id).await?;
    Ok(Json(ProposalResponse::new(&proposal)?))
}

/// Approves a pending proposal: its write is sent again as its agent, exactly as
/// previewed (the preview's IDs; committed only if it records the same events). Takes a
/// staff session holding every scope the proposal names. Answers the proposal, `applied`
/// or `failed` with the write's answer in `response`; a write that would no longer do
/// what was previewed is recorded `failed` and answered `409 preview_stale`, with what it
/// would do now in `details.preview` and the proposal in `details.proposal`. A proposal
/// whose preview stopped at an effect it couldn't run (`complete: false`) is sent as a
/// plain write.
#[utoipa::path(
    post,
    path = "/admin/agent-proposals/{id}/approve",
    params(("id" = ProposalId, Path)),
    responses((status = 200, description = "OK", body = ProposalResponse)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn approve<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: StaffCaller,
    PathId(id): PathId<ProposalId>,
) -> Result<Response, ApiError> {
    let (ports, staff) = caller.authenticate(&state).await?;
    let proposal = hypha_agents::approve_proposal(&staff.ctx, &ports, &state.clock, id).await?;
    if let Some(preview) = crate::preview::current() {
        // Its write is its own request: previewed when it was proposed.
        preview.block(crate::preview::BlockedBy::Proposal);
        return Err(ApiError::proposal_not_previewable());
    }
    drop(ports);
    let answer = crate::agents::apply_approved(&state, &proposal).await;
    let ports = state.public_ports().await?;
    hypha_core::ActingAs::act_as(&ports, staff.ctx.actor());
    let outcome = Outcome {
        status: answer.status,
        body: answer.body.clone(),
    };
    let decided = match (200..300).contains(&answer.status) {
        true => hypha_agents::record_applied(&staff.ctx, &ports, &state.clock, id, outcome).await?,
        false => hypha_agents::record_failed(&staff.ctx, &ports, &state.clock, id, outcome).await?,
    };
    let response = ProposalResponse::new(&decided)?;
    if answer.code() == Some("preview_stale") {
        let preview = answer.body["details"]["preview"].clone();
        let proposal = serde_json::to_value(&response).map_err(ApiError::internal)?;
        return Ok(ApiError::proposal_stale(preview, proposal).into_response());
    }
    Ok(Json(response).into_response())
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RejectProposalRequest {
    /// Why, for the agent (at most 2000 characters).
    note: Option<String>,
}

/// Rejects a pending proposal, with a note the agent can read. Takes a staff session
/// holding every scope the proposal names. The body is optional.
#[utoipa::path(
    post,
    path = "/admin/agent-proposals/{id}/reject",
    params(("id" = ProposalId, Path)),
    request_body = RejectProposalRequest,
    responses((status = 200, description = "OK", body = ProposalResponse)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn reject<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: StaffCaller,
    PathId(id): PathId<ProposalId>,
    body: JsonBody<RejectProposalRequest>,
) -> Result<Json<ProposalResponse>, ApiError> {
    let (ports, staff) = caller.authenticate(&state).await?;
    let note = body.parse_optional()?.and_then(|r| r.note);
    let proposal =
        hypha_agents::reject_proposal(&staff.ctx, &ports, &state.clock, id, note).await?;
    Ok(Json(ProposalResponse::new(&proposal)?))
}
