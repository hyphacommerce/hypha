//! Agents acting on the admin surface. An agent never calls a route from outside: its
//! credentials are refused everywhere (`agent_credentials_mcp_only`) but where a host
//! authenticates them (`/mcp`), which then calls an operation through `call`, the one
//! place an agent's request is judged:
//!
//! - the deployment's kill switch (`agents.enabled`) and the agent's own (disabled,
//!   revoked; a connected agent whose staff member was deactivated);
//! - its permissions now: `hypha_agents::effective` over its grant, the operator's cap
//!   (`agents.writes`) and, for a connected agent, its staff member's role read fresh;
//! - the operation: in its allowlist, or `403 operation_not_allowed`; a write under the
//!   cap `off` is `403 agent_writes_off`;
//! - the mode: a read reads; a write is previewed (`Mode::Preview`), proposed
//!   (`Mode::Propose`, or `Mode::Apply` where its grant only proposes: previewed, then kept
//!   as a `Proposal` for a person, answering `202 {"proposal": …}`), or applied;
//! - its rate (`Rule::AgentCall`, and `Rule::AgentWrite` for proposals and writes, per
//!   agent).
//!
//! The request then goes through the host's router in process (`internal::dispatch`) as
//! the agent (`AgentRequest`), so the handlers, redaction, errors, idempotency and
//! previews are the routes' own. Every call is logged (`AgentCall`, no bodies), refused
//! ones too.

use axum::{
    body::Body,
    extract::Request,
    http::{Method, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use hypha_agents::{
    Agent, AgentKind, CallMode, NewAgentCall, Permissions, ProposalRequest, Propose, WriteMode,
    WritesCap, effective,
};
use hypha_core::{
    Actor, AgentId, Clock, Ctx, OperationId, PreviewToken, PreviewTokenRepo, Scope, Timestamp,
    UserId,
};
use hypha_users::{RoleRepo, UserRepo};
use serde_json::{Map, Value, json};

use crate::error::ApiError;
use crate::internal::{AgentRequest, dispatch};
use crate::operations::Operation;
use crate::preview::{PREVIEW_HEADER, PREVIEW_TOKEN_HEADER, PreviewDigest, PreviewIds};
use crate::rate_limit::{Rule, enforce};
use crate::state::{SharedState, Storage};

/// How an agent wants a write to go. Reads ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Only previewed: nothing is kept; the answer is the preview (with its token).
    Preview,
    /// Written, where the agent's grant applies it; proposed where it only proposes.
    #[default]
    Apply,
    /// Previewed and kept for a person to approve, whatever the grant allows.
    Propose,
}

/// One call of an operation by an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationCall {
    pub operation: OperationId,
    /// The path parameters, as text, by name.
    pub params: Map<String, Value>,
    /// The query parameters: text, numbers, booleans, or lists of them.
    pub query: Map<String, Value>,
    pub body: Option<Value>,
    pub mode: Mode,
    pub idempotency_key: Option<String>,
    /// A preview's token, to apply exactly what was previewed (`Mode::Apply` only).
    pub preview_token: Option<String>,
}

/// What a call answered: the route's status and JSON body (Hypha's error body for a
/// failure), or `202 {"proposal": …}` for a proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub status: u16,
    pub body: Value,
    /// For a preview, or a stale apply's fresh one: the IDs its run was handed and its
    /// digest, which a proposal keeps.
    pub(crate) previewed: Option<(Vec<uuid::Uuid>, String)>,
}

impl Answer {
    /// The proposal a write became (`202 {"proposal": …}`), if it did.
    pub fn proposal(&self) -> Option<&str> {
        (self.status == StatusCode::ACCEPTED.as_u16())
            .then(|| self.body.pointer("/proposal/id").and_then(Value::as_str))
            .flatten()
    }

    /// The error's code, for a failure.
    pub fn code(&self) -> Option<&str> {
        (self.status >= 400)
            .then(|| self.body.get("code").and_then(Value::as_str))
            .flatten()
    }
}

/// Calls `call.operation` as `acting`, an agent `acting` found may act now (what `/mcp`
/// does once per request), judged as the module says, and logs it.
pub async fn call_as<S: Storage>(
    state: &SharedState<S>,
    acting: &Acting,
    call: OperationCall,
) -> Answer {
    let started = state.clock.now();
    let sent = judge_and_send(state, acting, &call, None).await;
    // One connection after the routed request (which takes its own): for the proposal
    // the write may become, and the log.
    let ports = state.public_ports().await;
    let (mode, answer) = match (sent, &ports) {
        (Ok((mode, Sent::Answered(answer))), _) => (mode, answer),
        (Ok((mode, Sent::Proposing(proposing))), Ok(ports)) => {
            match propose(state, ports, &call, proposing).await {
                Ok(answer) => (mode, answer),
                Err(e) => (mode, answer_of(e.into_response()).await),
            }
        }
        (Ok((mode, Sent::Proposing(_))), Err(_)) => {
            let e = ApiError::internal("no connection to keep a proposal on");
            (mode, answer_of(e.into_response()).await)
        }
        (Err((mode, e)), _) => (mode, answer_of(e.into_response()).await),
    };
    let agent = acting.agent.id();
    let ports = ports.as_ref().ok();
    log(state, ports, agent, &call.operation, mode, &answer, started).await;
    answer
}

/// Applies an approved proposal's write as its agent: with a token made from the
/// proposal's preview (its IDs and digest) when the preview was complete, so it commits
/// only if it does what was previewed; as a plain write otherwise (an effect a preview
/// couldn't run, which the approver saw). The agent's grant may only propose it: the
/// approval is the person's say. A person's act, not the agent's: it spends none of the
/// agent's rate limits, isn't its use and isn't in its call log (the proposal records
/// who approved it and what the write answered).
pub(crate) async fn apply_approved<S: Storage>(
    state: &SharedState<S>,
    proposal: &hypha_agents::Proposal,
) -> Answer {
    let call = OperationCall {
        operation: proposal.operation().clone(),
        params: proposal.request().params.clone(),
        query: proposal.request().query.clone(),
        body: proposal.request().body.clone(),
        mode: Mode::Apply,
        idempotency_key: proposal.idempotency_key().map(|k| k.as_str().to_string()),
        preview_token: None,
    };
    let sent = match acting(state, proposal.agent(), NoteUse::No).await {
        Ok(acting) => judge_and_send(state, &acting, &call, Some(proposal)).await,
        Err(e) => Err((CallMode::Apply, e)),
    };
    match sent {
        Ok((_, Sent::Answered(answer))) => answer,
        // An approved write is applied, never proposed again.
        Ok((_, Sent::Proposing(proposing))) => answer_of(proposing.response).await,
        Err((_, e)) => answer_of(e.into_response()).await,
    }
}

/// What sending a judged call came to: its answer, or a preview to keep as a proposal.
enum Sent {
    Answered(Answer),
    Proposing(Proposing),
}

/// A write the agent proposes: previewed as `ctx`, answered `response`.
struct Proposing {
    ctx: Ctx,
    operation: Operation,
    response: Response,
}

/// How a call is logged until it's judged: by what it asked for.
fn asked(operation: Option<&Operation>, mode: Mode) -> CallMode {
    match (operation, mode) {
        (Some(op), _) if !op.write => CallMode::Read,
        (_, Mode::Preview) => CallMode::Preview,
        (_, Mode::Propose) => CallMode::Propose,
        (_, Mode::Apply) => CallMode::Apply,
    }
}

/// What the agent may do now: its permissions, as the scopes of the `Ctx` its request
/// runs with.
struct Judged {
    ctx: Ctx,
    mode: CallMode,
}

async fn judge_and_send<S: Storage>(
    state: &SharedState<S>,
    acting: &Acting,
    call: &OperationCall,
    approved: Option<&hypha_agents::Proposal>,
) -> Result<(CallMode, Sent), (CallMode, ApiError)> {
    let agent = acting.agent.id();
    let operation = state.operations().get(&call.operation).cloned();
    let asked = asked(operation.as_ref(), call.mode);
    let operation = operation.ok_or_else(|| {
        (
            asked,
            ApiError::operation_not_found(call.operation.as_str()),
        )
    })?;
    let judged = judge(state, acting, &operation, call.mode, approved.is_some())
        .await
        .map_err(|e| (asked, e))?;
    let mode = judged.mode;
    let request =
        request_for(&operation, call, mode, approved.and_then(complete)).map_err(|e| (mode, e))?;
    let token = match (&request.mint, approved) {
        (Some(mint), Some(proposal)) => Some(
            mint_token(state, proposal, mint)
                .await
                .map_err(|e| (mode, e))?,
        ),
        _ => None,
    };
    let http = request.into_http(token.as_deref()).map_err(|e| (mode, e))?;
    let as_agent = AgentRequest {
        agent,
        ctx: judged.ctx.clone(),
    };
    let response = dispatch(state, http, Some(as_agent)).await;
    match mode {
        CallMode::Propose => Ok((
            mode,
            Sent::Proposing(Proposing {
                ctx: judged.ctx,
                operation,
                response,
            }),
        )),
        _ => Ok((mode, Sent::Answered(answer_of(response).await))),
    }
}

/// Judges the call of `operation` by `acting` in `asked` mode (`approved`: a person
/// approved it), as the module says. The `Ctx` it answers carries the write scopes the
/// agent only proposes solely where nothing is kept without a person: a preview or a
/// proposal's (sent with `Hypha-Preview`, so rolled back) and an approved proposal's write.
/// A direct read or write runs without them, so it can't commit what the agent may only
/// propose, whatever else its operation names.
async fn judge<S: Storage>(
    state: &SharedState<S>,
    acting: &Acting,
    operation: &Operation,
    asked: Mode,
    approved: bool,
) -> Result<Judged, ApiError> {
    let permissions = &acting.permissions;
    if !permissions.allows_operation(&operation.id) {
        return Err(ApiError::operation_not_allowed(operation.id.as_str()));
    }
    let mode = match (operation.write, approved, asked) {
        (false, _, _) => CallMode::Read,
        _ if state.config.agents.writes == WritesCap::Off => {
            return Err(ApiError::agent_writes_off());
        }
        (true, true, _) => CallMode::Apply,
        (true, false, Mode::Preview) => CallMode::Preview,
        (true, false, Mode::Propose) => CallMode::Propose,
        (true, false, Mode::Apply) => match permissions.mode_for(&operation.scopes) {
            WriteMode::Propose => CallMode::Propose,
            WriteMode::Apply => CallMode::Apply,
        },
    };
    if !approved {
        let key = acting.agent.id().to_string();
        enforce(&state.limiter, Rule::AgentCall, &key).await?;
        if matches!(mode, CallMode::Propose | CallMode::Apply) {
            enforce(&state.limiter, Rule::AgentWrite, &key).await?;
        }
    }
    let scopes = match (mode, approved) {
        (CallMode::Preview | CallMode::Propose, _) | (CallMode::Apply, true) => {
            permissions.scopes()
        }
        (CallMode::Read, _) | (CallMode::Apply, false) => permissions.applied_scopes(),
    };
    Ok(Judged {
        ctx: Ctx::new(Actor::Agent(acting.agent.id()), scopes),
        mode,
    })
}

/// An agent as it may act now, and what it may do.
pub struct Acting {
    pub agent: Agent,
    /// Its grant under the operator's cap, within its staff member's role for a connected
    /// agent: read fresh.
    pub permissions: Permissions,
}

/// Whether finding an agent that may act counts as its use (`last_used_at`): its own
/// requests do, a person approving its proposal doesn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteUse {
    Yes,
    No,
}

/// `agent` if it may act now: agents are on in this deployment (`agents_disabled`), it's
/// enabled and not revoked, and a connected one's staff member is active
/// (`agent_disabled`); with what it may do. Its use is noted when `note` says so, on the
/// same connection.
pub async fn acting<S: Storage>(
    state: &SharedState<S>,
    agent: AgentId,
    note: NoteUse,
) -> Result<Acting, ApiError> {
    if !state.config.agents.enabled {
        return Err(ApiError::agents_disabled());
    }
    acting_on(state, &state.public_ports().await?, agent, note).await
}

/// `acting` on the request's `ports`.
pub(crate) async fn acting_on<S: Storage>(
    state: &SharedState<S>,
    ports: &S::Ports<'_>,
    agent: AgentId,
    note: NoteUse,
) -> Result<Acting, ApiError> {
    let agents = &state.config.agents;
    if !agents.enabled {
        return Err(ApiError::agents_disabled());
    }
    let found = match hypha_agents::active_agent(ports, agent).await {
        Ok(found) => found,
        Err(
            hypha_agents::CommandError::AgentDisabled(_)
            | hypha_agents::CommandError::Agent(hypha_agents::AgentError::Revoked(_)),
        ) => return Err(ApiError::agent_disabled()),
        Err(e) => return Err(e.into()),
    };
    let role = match found.kind() {
        AgentKind::Connected { user } => Some(role_of(ports, user).await?),
        AgentKind::Autonomous => None,
    };
    if note == NoteUse::Yes {
        hypha_agents::record_agent_use(ports, &found, state.clock.now()).await?;
    }
    let permissions = effective(found.grant(), agents.writes, role.as_deref());
    Ok(Acting {
        agent: found,
        permissions,
    })
}

/// The scopes of the role of `user`, a connected agent's staff member, read now. A
/// deactivated member's agents stop: `agent_disabled`.
async fn role_of<P: hypha_users::HasUsers + hypha_users::HasRoles>(
    ports: &P,
    user: UserId,
) -> Result<Vec<Scope>, ApiError> {
    let Some(user) = ports.users().get(user).await?.filter(|u| u.is_active()) else {
        return Err(ApiError::agent_disabled());
    };
    let Some(role) = ports.roles().get(user.role()).await? else {
        return Err(ApiError::agent_disabled());
    };
    Ok(role.granted())
}

/// The proposal's preview IDs and digest, when its preview was complete.
fn complete(proposal: &hypha_agents::Proposal) -> Option<(Vec<uuid::Uuid>, String)> {
    let preview = proposal.preview();
    if preview.get("complete") != Some(&Value::Bool(true)) {
        return None;
    }
    let ids = preview
        .get("ids")?
        .as_array()?
        .iter()
        .map(|id| id.as_str().and_then(|id| uuid::Uuid::parse_str(id).ok()))
        .collect::<Option<Vec<_>>>()?;
    Some((ids, proposal.digest().to_string()))
}

/// A request for an operation, before it's sent: what its preview token (if one is to be
/// made for it) binds to.
struct OperationRequest {
    method: Method,
    path: String,
    query: String,
    body: Option<Vec<u8>>,
    mode: CallMode,
    idempotency_key: Option<String>,
    preview_token: Option<String>,
    /// For an approved proposal with a complete preview: its IDs and digest, and the
    /// request's fingerprint, to make the token that applies it.
    mint: Option<Mint>,
}

struct Mint {
    ids: Vec<uuid::Uuid>,
    digest: String,
    fingerprint: [u8; 32],
}

fn request_for(
    operation: &Operation,
    call: &OperationCall,
    mode: CallMode,
    approved: Option<(Vec<uuid::Uuid>, String)>,
) -> Result<OperationRequest, ApiError> {
    let method = Method::from_bytes(operation.method.as_bytes()).map_err(ApiError::internal)?;
    let path = operation
        .request_path(&call.params)
        .map_err(ApiError::invalid_operation_call)?;
    let query = query_string(&call.query)?;
    let body = call
        .body
        .as_ref()
        .map(serde_json::to_vec)
        .transpose()
        .map_err(ApiError::internal)?;
    let mint = approved.map(|(ids, digest)| Mint {
        ids,
        digest,
        fingerprint: crate::extract::fingerprint(
            method.as_str(),
            &path,
            &query,
            body.as_deref().unwrap_or_default(),
        ),
    });
    Ok(OperationRequest {
        method,
        path,
        query,
        body,
        mode,
        idempotency_key: call.idempotency_key.clone(),
        preview_token: match mode {
            CallMode::Apply => call.preview_token.clone(),
            _ => None,
        },
        mint,
    })
}

impl OperationRequest {
    /// As the HTTP request the router gets; `minted` is the token made for an approved
    /// proposal, which wins over one the agent sent.
    fn into_http(self, minted: Option<&str>) -> Result<Request<Body>, ApiError> {
        let uri = match self.query.is_empty() {
            true => self.path,
            false => format!("{}?{}", self.path, self.query),
        };
        let mut builder = Request::builder().method(self.method).uri(uri);
        if self.body.is_some() {
            builder = builder.header(CONTENT_TYPE, "application/json");
        }
        if let Some(key) = &self.idempotency_key {
            builder = builder.header("idempotency-key", key);
        }
        if matches!(self.mode, CallMode::Preview | CallMode::Propose) {
            builder = builder.header(PREVIEW_HEADER, "1");
        }
        if let Some(token) = minted.or(self.preview_token.as_deref()) {
            builder = builder.header(PREVIEW_TOKEN_HEADER, token);
        }
        builder
            .body(self.body.map_or_else(Body::empty, Body::from))
            .map_err(ApiError::invalid_operation_call)
    }
}

/// `query` as a URL's query string: each value text, a number or a boolean, a list
/// repeating its key. Anything else (an object, `null`) isn't a query parameter.
fn query_string(query: &Map<String, Value>) -> Result<String, ApiError> {
    let scalar = |key: &str, value: &Value| match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        _ => Err(ApiError::invalid_operation_call(format!(
            "query parameter {key:?} is text, a number, a boolean or a list of them"
        ))),
    };
    let mut pairs = Vec::new();
    for (key, value) in query {
        match value {
            Value::Array(items) => {
                for item in items {
                    pairs.push((key, scalar(key, item)?));
                }
            }
            value => pairs.push((key, scalar(key, value)?)),
        }
    }
    Ok(pairs
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                crate::operations::encode(key, false),
                crate::operations::encode(value, false)
            )
        })
        .collect::<Vec<_>>()
        .join("&"))
}

/// Stores a token applying `proposal` as its agent previewed it, for the approved write
/// about to be sent; the token itself.
async fn mint_token<S: Storage>(
    state: &SharedState<S>,
    proposal: &hypha_agents::Proposal,
    mint: &Mint,
) -> Result<String, ApiError> {
    let token = hypha_auth::NewToken::generate(
        hypha_auth::TokenKind::PreviewToken,
        &state.env.runtime().random,
    );
    let record = PreviewToken {
        hash: *token.hash().as_bytes(),
        actor: Actor::Agent(proposal.agent()),
        request: mint.fingerprint,
        ids: mint.ids.clone(),
        digest: mint.digest.clone(),
        expires_at: state
            .clock
            .now()
            .plus(state.config.agents.preview_token_lifetime),
        used_at: None,
    };
    let ports = state.public_ports().await?;
    hypha_core::HasPreviewTokens::preview_tokens(&ports)
        .insert(&record)
        .await?;
    Ok(token.expose().to_string())
}

/// Keeps what the agent's write previewed as a proposal: a preview that failed answers its
/// failure, and nothing is kept.
async fn propose<S: Storage>(
    state: &SharedState<S>,
    ports: &S::Ports<'_>,
    call: &OperationCall,
    proposing: Proposing,
) -> Result<Answer, ApiError> {
    let Proposing {
        ctx,
        operation,
        response,
    } = proposing;
    let ctx = &ctx;
    let answer = answer_of(response).await;
    let (Some((ids, digest)), 200) = (answer.previewed.clone(), answer.status) else {
        return Ok(answer);
    };
    let preview = kept_preview(answer.body.get("preview"), &ids);
    let input = Propose {
        operation: operation.id.as_str().to_string(),
        request: ProposalRequest {
            params: call.params.clone(),
            query: call.query.clone(),
            body: call.body.clone(),
        },
        idempotency_key: call.idempotency_key.clone(),
        preview,
        digest,
        scopes: operation.scopes.clone(),
    };
    let proposal = hypha_agents::create_proposal(
        ctx,
        ports,
        &state.ids,
        &state.clock,
        &state.config.agents.settings,
        input,
    )
    .await?;
    let dto =
        crate::routes::agents::ProposalResponse::new(ctx, &proposal.value, state.operations())?;
    Ok(Answer {
        status: StatusCode::ACCEPTED.as_u16(),
        body: json!({ "proposal": dto }),
        previewed: None,
    })
}

/// A preview as a proposal keeps it: without its token (the approval makes its own), with
/// the IDs the run was handed (`ids`, hidden from readers), which approving reuses.
pub(crate) fn kept_preview(preview: Option<&Value>, ids: &[uuid::Uuid]) -> Value {
    let mut preview = preview.cloned().unwrap_or(Value::Null);
    if let Value::Object(fields) = &mut preview {
        fields.remove("token");
        fields.remove("expires_at");
        let ids = ids.iter().map(|id| Value::String(id.to_string())).collect();
        fields.insert("ids".to_string(), Value::Array(ids));
    }
    preview
}

/// A response as an `Answer`: its status and JSON body (`null` when empty or not JSON).
pub async fn answer_of(response: Response) -> Answer {
    let ids = response.extensions().get::<PreviewIds>().cloned();
    let digest = response.extensions().get::<PreviewDigest>().cloned();
    let previewed = match (ids, digest) {
        (Some(PreviewIds(ids)), Some(PreviewDigest(digest))) => Some((ids, digest)),
        _ => None,
    };
    let status = response.status().as_u16();
    let body = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
        Ok(bytes) if !bytes.is_empty() => serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    Answer {
        status,
        body,
        previewed,
    }
}

/// Logs the call; a log that can't be written is only reported, never the call's
/// failure.
async fn log<S: Storage>(
    state: &SharedState<S>,
    ports: Option<&S::Ports<'_>>,
    agent: AgentId,
    operation: &OperationId,
    mode: CallMode,
    answer: &Answer,
    started: Timestamp,
) {
    let now = state.clock.now();
    let micros = now
        .unix_micros()
        .saturating_sub(started.unix_micros())
        .max(0);
    let call = NewAgentCall {
        agent,
        operation: operation.clone(),
        mode,
        status: answer.status,
        error_code: answer.code().map(str::to_string),
        duration_ms: u32::try_from(micros / 1000).unwrap_or(u32::MAX),
        at: now,
    };
    let logged = match ports {
        Some(ports) => hypha_agents::record_call(ports, &state.ids, call)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        None => Err("no connection".to_string()),
    };
    if let Err(error) = logged {
        tracing::warn!(%agent, %error, "an agent's call wasn't logged");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_is_scalars_and_lists_of_them() {
        let query = json!({"status": "open", "limit": 5, "tag": ["a b", "c&d"], "on": true});
        assert_eq!(
            query_string(query.as_object().unwrap()).ok().as_deref(),
            Some("limit=5&on=true&status=open&tag=a%20b&tag=c%26d")
        );
        for bad in [
            json!({"a": {"b": 1}}),
            json!({"a": null}),
            json!({"a": [[1]]}),
        ] {
            assert!(query_string(bad.as_object().unwrap()).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_answers_code_is_a_failures() {
        let failed = Answer {
            status: 403,
            body: json!({"code": "missing_scope"}),
            previewed: None,
        };
        assert_eq!(failed.code(), Some("missing_scope"));
        let ok = Answer {
            status: 200,
            body: json!({"code": "x"}),
            previewed: None,
        };
        assert_eq!(ok.code(), None);
    }
}
