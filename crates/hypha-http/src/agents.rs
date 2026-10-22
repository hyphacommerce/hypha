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
    AgentKind, CallMode, NewAgentCall, ProposalRequest, Propose, WriteMode, WritesCap, effective,
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
}

impl Answer {
    /// The error's code, for a failure.
    pub fn code(&self) -> Option<&str> {
        (self.status >= 400)
            .then(|| self.body.get("code").and_then(Value::as_str))
            .flatten()
    }
}

/// Calls `call.operation` as `agent`, judged as the module says, and logs it.
pub async fn call<S: Storage>(
    state: &SharedState<S>,
    agent: AgentId,
    call: OperationCall,
) -> Answer {
    run(state, agent, call, None).await
}

/// Applies an approved proposal's write as its agent: with a token made from the
/// proposal's preview (its IDs and digest) when the preview was complete, so it commits
/// only if it does what was previewed; as a plain write otherwise (an effect a preview
/// couldn't run, which the approver saw). The agent's grant may only propose it: the
/// approval is the person's say.
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
    run(state, proposal.agent(), call, Some(proposal)).await
}

async fn run<S: Storage>(
    state: &SharedState<S>,
    agent: AgentId,
    call: OperationCall,
    approved: Option<&hypha_agents::Proposal>,
) -> Answer {
    let started = state.clock.now();
    let (mode, answer) = match judge_and_send(state, agent, &call, approved).await {
        Ok(done) => done,
        Err((mode, e)) => (mode, answer_of(e.into_response()).await),
    };
    log(state, agent, &call.operation, mode, &answer, started).await;
    answer
}

/// What the agent may do now: its permissions, as the scopes of the `Ctx` its request
/// runs with.
struct Judged {
    ctx: Ctx,
    mode: CallMode,
}

async fn judge_and_send<S: Storage>(
    state: &SharedState<S>,
    agent: AgentId,
    call: &OperationCall,
    approved: Option<&hypha_agents::Proposal>,
) -> Result<(CallMode, Answer), (CallMode, ApiError)> {
    let operation = state.operations().get(&call.operation).cloned();
    // How the call is logged until it's judged: by what it asked for.
    let asked = match (&operation, call.mode) {
        (Some(op), _) if !op.write => CallMode::Read,
        (_, Mode::Preview) => CallMode::Preview,
        (_, Mode::Propose) => CallMode::Propose,
        (_, Mode::Apply) => CallMode::Apply,
    };
    let operation = operation.ok_or_else(|| {
        (
            asked,
            ApiError::operation_not_found(call.operation.as_str()),
        )
    })?;
    let judged = judge(state, agent, &operation, call.mode, approved.is_some())
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
        CallMode::Propose => propose(state, &judged.ctx, &operation, call, response)
            .await
            .map(|answer| (mode, answer))
            .map_err(|e| (mode, e)),
        _ => Ok((mode, answer_of(response).await)),
    }
}

/// Judges `agent`'s call of `operation` in `asked` mode (`approved`: a person approved
/// it), as the module says.
async fn judge<S: Storage>(
    state: &SharedState<S>,
    agent: AgentId,
    operation: &Operation,
    asked: Mode,
    approved: bool,
) -> Result<Judged, ApiError> {
    let agents = &state.config.agents;
    if !agents.enabled {
        return Err(ApiError::agents_disabled());
    }
    let ports = state.public_ports().await?;
    let found = match hypha_agents::active_agent(&ports, agent).await {
        Ok(found) => found,
        Err(
            hypha_agents::CommandError::AgentDisabled(_)
            | hypha_agents::CommandError::Agent(hypha_agents::AgentError::Revoked(_)),
        ) => return Err(ApiError::agent_disabled()),
        Err(e) => return Err(e.into()),
    };
    let role = match found.kind() {
        AgentKind::Connected { user } => Some(role_of(&ports, user).await?),
        AgentKind::Autonomous => None,
    };
    let permissions = effective(found.grant(), agents.writes, role.as_deref());
    if !permissions.allows_operation(&operation.id) {
        return Err(ApiError::operation_not_allowed(operation.id.as_str()));
    }
    let mode = match (operation.write, approved, asked) {
        (false, _, _) => CallMode::Read,
        _ if agents.writes == WritesCap::Off => return Err(ApiError::agent_writes_off()),
        (true, true, _) => CallMode::Apply,
        (true, false, Mode::Preview) => CallMode::Preview,
        (true, false, Mode::Propose) => CallMode::Propose,
        (true, false, Mode::Apply) => match permissions.mode_for(&operation.scopes) {
            Some(WriteMode::Propose) => CallMode::Propose,
            // Missing a scope: the route refuses it as it would anyone.
            Some(WriteMode::Apply) | None => CallMode::Apply,
        },
    };
    let key = agent.to_string();
    enforce(&state.limiter, Rule::AgentCall, &key).await?;
    if matches!(mode, CallMode::Propose | CallMode::Apply) {
        enforce(&state.limiter, Rule::AgentWrite, &key).await?;
    }
    hypha_agents::record_agent_use(&ports, &found, state.clock.now()).await?;
    Ok(Judged {
        ctx: Ctx::new(Actor::Agent(agent), permissions.scopes()),
        mode,
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
    ctx: &Ctx,
    operation: &Operation,
    call: &OperationCall,
    response: Response,
) -> Result<Answer, ApiError> {
    let ids = response.extensions().get::<PreviewIds>().cloned();
    let digest = response.extensions().get::<PreviewDigest>().cloned();
    let answer = answer_of(response).await;
    let (Some(PreviewIds(ids)), Some(PreviewDigest(digest)), 200) = (ids, digest, answer.status)
    else {
        return Ok(answer);
    };
    let mut preview = answer.body.get("preview").cloned().unwrap_or(Value::Null);
    if let Value::Object(fields) = &mut preview {
        // The approval makes its own token, from these IDs.
        fields.remove("token");
        fields.remove("expires_at");
        let ids = ids.iter().map(|id| Value::String(id.to_string())).collect();
        fields.insert("ids".to_string(), Value::Array(ids));
    }
    let ports = state.public_ports().await?;
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
        &ports,
        &state.ids,
        &state.clock,
        &state.config.agents.settings,
        input,
    )
    .await?;
    let dto = crate::routes::agents::ProposalResponse::new(&proposal.value)?;
    Ok(Answer {
        status: StatusCode::ACCEPTED.as_u16(),
        body: json!({ "proposal": dto }),
    })
}

/// A response as an `Answer`: its status and JSON body (`null` when empty or not JSON).
pub(crate) async fn answer_of(response: Response) -> Answer {
    let status = response.status().as_u16();
    let body = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
        Ok(bytes) if !bytes.is_empty() => serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    Answer { status, body }
}

/// Logs the call; a log that can't be written is only reported, never the call's
/// failure.
async fn log<S: Storage>(
    state: &SharedState<S>,
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
    let logged = match state.public_ports().await {
        Ok(ports) => hypha_agents::record_call(&ports, &state.ids, call)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Err(_) => Err("no connection".to_string()),
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
        };
        assert_eq!(failed.code(), Some("missing_scope"));
        let ok = Answer {
            status: 200,
            body: json!({"code": "x"}),
        };
        assert_eq!(ok.code(), None);
    }
}
