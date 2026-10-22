//! The server: one HTTP POST's body and headers in, the status and body to answer out.

use hypha_core::ErrorCode;
use serde_json::{Map, Value, json};

use crate::jsonrpc::{self, INTERNAL_ERROR, INVALID_PARAMS, METHOD_NOT_FOUND, Message, RpcError};
use crate::ops::{
    CallMode, McpError, OperationCall, OperationId, OperationResult, Operations, ProposalQuery,
};
use crate::protocol::{self, Era, Headers, META_SERVER_INFO};
use crate::tools::{
    self, CALL, CallArgs, DESCRIBE_OPERATION, DescribeArgs, DirectArgs, FIND_DEFAULT_LIMIT,
    FIND_MAX_LIMIT, FIND_OPERATIONS, FIXED_TOOLS, FindArgs, GET_PROPOSAL, LIST_PROPOSALS,
    ListProposalsArgs, NoArgs, PROPOSALS_MAX_LIMIT, ProposalArgs, WHOAMI,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// `serverInfo.name`.
    pub name: String,
    /// `serverInfo.title`, for display.
    pub title: Option<String>,
    pub version: String,
    /// What the client may hand its model about using this server.
    pub instructions: Option<String>,
    /// Operations offered as tools of their own, in this order, when the caller can
    /// see them.
    pub direct_tools: Vec<OperationId>,
    /// How long a client may keep a `tools/list` answer, in milliseconds. It differs
    /// per caller (their grant), so it's always answered as private.
    pub list_ttl_ms: u64,
}

impl ServerConfig {
    pub const DEFAULT_LIST_TTL_MS: u64 = 60_000;

    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: None,
            version: version.into(),
            instructions: Some(tools::DEFAULT_INSTRUCTIONS.to_string()),
            direct_tools: Vec::new(),
            list_ttl_ms: Self::DEFAULT_LIST_TTL_MS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("direct tool {0} has the name of a fixed tool")]
    DirectToolShadowsFixed(OperationId),
    #[error("direct tool {0} is listed twice")]
    DuplicateDirectTool(OperationId),
    #[error("direct tool {0}'s name is over 64 characters, which model APIs refuse")]
    DirectToolNameTooLong(OperationId),
}

impl ErrorCode for ConfigError {
    fn code(&self) -> &'static str {
        match self {
            Self::DirectToolShadowsFixed(_) => "direct_tool_shadows_fixed",
            Self::DuplicateDirectTool(_) => "duplicate_direct_tool",
            Self::DirectToolNameTooLong(_) => "direct_tool_name_too_long",
        }
    }
}

/// One POST to the MCP endpoint, as the transport read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpRequest<'a> {
    pub body: &'a [u8],
    pub headers: Headers<'a>,
}

/// What the transport answers: `body` as `application/json` when there is one, else
/// an empty body (`202 Accepted` for a notification).
#[derive(Debug, Clone, PartialEq)]
pub struct McpResponse {
    pub status: u16,
    pub body: Option<Value>,
}

impl McpResponse {
    fn accepted() -> Self {
        Self {
            status: 202,
            body: None,
        }
    }

    fn rejected(error: RpcError) -> Self {
        Self {
            status: 400,
            body: Some(error.to_json()),
        }
    }
}

/// The MCP server. Stateless: one per deployment, shared by every request.
#[derive(Debug, Clone)]
pub struct Server {
    config: ServerConfig,
}

impl Server {
    pub fn new(config: ServerConfig) -> Result<Self, ConfigError> {
        for (i, id) in config.direct_tools.iter().enumerate() {
            let name = id.tool_name();
            if name.len() > MAX_TOOL_NAME_LEN {
                return Err(ConfigError::DirectToolNameTooLong(id.clone()));
            }
            if FIXED_TOOLS.contains(&name.as_str()) {
                return Err(ConfigError::DirectToolShadowsFixed(id.clone()));
            }
            if config.direct_tools[..i].contains(id) {
                return Err(ConfigError::DuplicateDirectTool(id.clone()));
            }
        }
        Ok(Self { config })
    }

    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Answers one POST. Transport concerns stay with the caller: authentication,
    /// the `Origin` check, `405` for GET and DELETE, the content type.
    pub async fn handle<O: Operations>(&self, ops: &O, request: McpRequest<'_>) -> McpResponse {
        let (id, method, params) = match jsonrpc::parse(request.body) {
            Err(error) => return McpResponse::rejected(error),
            Ok(Message::Notification { .. } | Message::Response) => {
                return McpResponse::accepted();
            }
            Ok(Message::Request { id, method, params }) => (id, method, params),
        };
        let era = match protocol::check_request(&id, &method, &params, request.headers) {
            Ok(era) => era,
            Err(error) => return McpResponse::rejected(error),
        };
        match self.dispatch(ops, era, &id, &method, params).await {
            Ok(result) => McpResponse {
                status: 200,
                body: Some(jsonrpc::result(&id, self.finish(era, result))),
            },
            Err(error) => McpResponse {
                status: match error.code {
                    METHOD_NOT_FOUND if era == Era::Modern => 404,
                    INTERNAL_ERROR => 500,
                    _ => 200,
                },
                body: Some(error.to_json()),
            },
        }
    }

    async fn dispatch<O: Operations>(
        &self,
        ops: &O,
        era: Era,
        id: &Value,
        method: &str,
        params: Map<String, Value>,
    ) -> Result<Value, RpcError> {
        match (era, method) {
            (Era::Modern, "server/discover") => Ok(self.discover()),
            (Era::Legacy, "initialize") => Ok(self.initialize(&params)),
            (Era::Legacy, "ping") => Ok(json!({})),
            (_, "tools/list") => self.list_tools(ops, era, id, &params).await,
            (_, "tools/call") => self.call_tool(ops, id, params).await,
            _ => Err(RpcError::new(
                Some(id.clone()),
                METHOD_NOT_FOUND,
                format!("Method not found: {method}"),
            )),
        }
    }

    /// Modern results say they're complete and who answered.
    fn finish(&self, era: Era, result: Value) -> Value {
        let Value::Object(mut result) = result else {
            return result;
        };
        if era == Era::Modern {
            result.insert("resultType".into(), json!("complete"));
            let meta = result
                .entry("_meta")
                .or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(meta) = meta {
                meta.insert(META_SERVER_INFO.into(), self.server_info());
            }
        }
        Value::Object(result)
    }

    fn server_info(&self) -> Value {
        let mut info = Map::new();
        info.insert("name".into(), json!(self.config.name));
        if let Some(title) = &self.config.title {
            info.insert("title".into(), json!(title));
        }
        info.insert("version".into(), json!(self.config.version));
        Value::Object(info)
    }

    fn discover(&self) -> Value {
        let mut result = Map::new();
        result.insert(
            "supportedVersions".into(),
            json!(protocol::supported_versions()),
        );
        result.insert("capabilities".into(), json!({"tools": {}}));
        if let Some(instructions) = &self.config.instructions {
            result.insert("instructions".into(), json!(instructions));
        }
        result.insert("ttlMs".into(), json!(self.config.list_ttl_ms));
        result.insert("cacheScope".into(), json!("public"));
        Value::Object(result)
    }

    fn initialize(&self, params: &Map<String, Value>) -> Value {
        let requested = params.get("protocolVersion").and_then(Value::as_str);
        let mut result = Map::new();
        result.insert(
            "protocolVersion".into(),
            json!(protocol::negotiate_legacy(requested)),
        );
        result.insert(
            "capabilities".into(),
            json!({"tools": {"listChanged": false}}),
        );
        result.insert("serverInfo".into(), self.server_info());
        if let Some(instructions) = &self.config.instructions {
            result.insert("instructions".into(), json!(instructions));
        }
        Value::Object(result)
    }

    async fn list_tools<O: Operations>(
        &self,
        ops: &O,
        era: Era,
        id: &Value,
        params: &Map<String, Value>,
    ) -> Result<Value, RpcError> {
        // Every tool fits one page, so no cursor was ever handed out.
        if params.get("cursor").is_some_and(|c| !c.is_null()) {
            return Err(RpcError::new(
                Some(id.clone()),
                INVALID_PARAMS,
                "Invalid params: unknown cursor",
            ));
        }
        let mut list = tools::fixed_definitions();
        if !self.config.direct_tools.is_empty() {
            let visible = ops.visible().await.map_err(|e| internal(id, e))?;
            for direct in &self.config.direct_tools {
                if !visible.iter().any(|op| &op.id == direct) {
                    continue;
                }
                match ops.describe(direct).await {
                    Ok(schema) => list.push(tools::direct_definition(&schema)),
                    // Gone between the two reads: leave it out.
                    Err(McpError::OperationNotFound(_)) => {}
                    Err(error) => return Err(internal(id, error)),
                }
            }
        }
        let mut result = Map::new();
        result.insert("tools".into(), Value::Array(list));
        if era == Era::Modern {
            result.insert("ttlMs".into(), json!(self.config.list_ttl_ms));
            result.insert("cacheScope".into(), json!("private"));
        }
        Ok(Value::Object(result))
    }

    async fn call_tool<O: Operations>(
        &self,
        ops: &O,
        id: &Value,
        params: Map<String, Value>,
    ) -> Result<Value, RpcError> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err(RpcError::new(
                Some(id.clone()),
                INVALID_PARAMS,
                "Invalid params: \"name\" must be a string",
            ));
        };
        let args = match params.get("arguments") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(args)) => args.clone(),
            Some(_) => {
                return Err(RpcError::new(
                    Some(id.clone()),
                    INVALID_PARAMS,
                    "Invalid params: \"arguments\" must be an object",
                ));
            }
        };
        let outcome = match name {
            FIND_OPERATIONS => find_operations(ops, &self.config.direct_tools, args).await,
            DESCRIBE_OPERATION => describe_operation(ops, args).await,
            CALL => call(ops, args).await,
            GET_PROPOSAL => get_proposal(ops, args).await,
            LIST_PROPOSALS => list_proposals(ops, args).await,
            WHOAMI => whoami(ops, args).await,
            _ => match self.direct_tool(ops, id, name).await? {
                Some(operation) => direct(ops, operation, args).await,
                None => {
                    return Err(RpcError::new(
                        Some(id.clone()),
                        INVALID_PARAMS,
                        format!("Unknown tool: {name}"),
                    ));
                }
            },
        };
        match outcome {
            Ok(result) => Ok(result),
            Err(McpError::OperationNotFound(operation)) => Ok(tools::tool_error(
                "operation_not_found",
                McpError::OperationNotFound(operation.clone()).to_string(),
                json!({"operation": operation}),
            )),
            Err(error) => Err(internal(id, error)),
        }
    }

    /// A curated operation the caller can see, by its tool name.
    async fn direct_tool<O: Operations>(
        &self,
        ops: &O,
        id: &Value,
        name: &str,
    ) -> Result<Option<OperationId>, RpcError> {
        let Ok(wanted) = OperationId::from_tool_name(name) else {
            return Ok(None);
        };
        let Some(operation) = self.config.direct_tools.iter().find(|op| **op == wanted) else {
            return Ok(None);
        };
        let visible = ops.visible().await.map_err(|e| internal(id, e))?;
        Ok(visible
            .iter()
            .any(|op| &op.id == operation)
            .then(|| operation.clone()))
    }
}

/// The longest tool name the Anthropic and OpenAI APIs take. Clients that prefix
/// names (Claude Code's `mcp__<server>__`) eat into it, so the curated IDs stay well
/// under.
pub const MAX_TOOL_NAME_LEN: usize = 64;

/// A failure behind the port: the client learns only that it happened.
fn internal(id: &Value, _error: McpError) -> RpcError {
    RpcError::new(Some(id.clone()), INTERNAL_ERROR, "Internal error")
}

/// A tool's outcome: a result (perhaps a tool error), or a port failure.
type Outcome = Result<Value, McpError>;

macro_rules! args {
    ($args:expr) => {
        match tools::parse_args($args) {
            Ok(args) => args,
            Err(tool_error) => return Ok(tool_error),
        }
    };
}

fn operation_id(raw: String) -> Result<OperationId, Value> {
    OperationId::new(raw).map_err(|e| {
        tools::tool_error(
            "invalid_arguments",
            format!("Invalid arguments: {e}"),
            Value::Null,
        )
    })
}

async fn find_operations<O: Operations>(
    ops: &O,
    direct_tools: &[OperationId],
    args: Map<String, Value>,
) -> Outcome {
    let args: FindArgs = args!(args);
    let limit = args
        .limit
        .unwrap_or(FIND_DEFAULT_LIMIT)
        .clamp(1, FIND_MAX_LIMIT) as usize;
    let visible = ops.visible().await?;
    let ranked = tools::rank(&visible, args.query.as_deref(), args.access);
    let matches = ranked.len();
    let operations: Vec<Value> = ranked
        .into_iter()
        .take(limit)
        .map(|op| tools::summary_json(op, direct_tools.contains(&op.id)))
        .collect();
    Ok(tools::json_result(
        json!({"operations": operations, "matches": matches}),
        false,
    ))
}

async fn describe_operation<O: Operations>(ops: &O, args: Map<String, Value>) -> Outcome {
    let args: DescribeArgs = args!(args);
    let id = match operation_id(args.operation) {
        Ok(id) => id,
        Err(tool_error) => return Ok(tool_error),
    };
    let schema = ops.describe(&id).await?;
    Ok(tools::json_result(tools::schema_json(&schema), false))
}

async fn call<O: Operations>(ops: &O, args: Map<String, Value>) -> Outcome {
    let args: CallArgs = args!(args);
    let (operation, inputs) = args.split();
    let operation = match operation_id(operation) {
        Ok(id) => id,
        Err(tool_error) => return Ok(tool_error),
    };
    run(ops, operation, inputs).await
}

async fn direct<O: Operations>(
    ops: &O,
    operation: OperationId,
    args: Map<String, Value>,
) -> Outcome {
    let inputs: DirectArgs = args!(args);
    run(ops, operation, inputs).await
}

async fn run<O: Operations>(ops: &O, operation: OperationId, inputs: DirectArgs) -> Outcome {
    let call = OperationCall {
        operation,
        params: inputs.params,
        query: inputs.query,
        body: inputs.body,
        mode: inputs.mode.unwrap_or(CallMode::Apply),
        idempotency_key: inputs.idempotency_key,
        preview_token: inputs.preview_token,
    };
    Ok(match ops.call(call).await? {
        OperationResult::Response(response) => tools::response_result(response),
        OperationResult::Proposed(proposed) => tools::proposed_result(proposed),
    })
}

async fn get_proposal<O: Operations>(ops: &O, args: Map<String, Value>) -> Outcome {
    let args: ProposalArgs = args!(args);
    Ok(tools::response_result(ops.proposal(&args.proposal).await?))
}

async fn list_proposals<O: Operations>(ops: &O, args: Map<String, Value>) -> Outcome {
    let args: ListProposalsArgs = args!(args);
    let query = ProposalQuery {
        status: args.status,
        cursor: args.cursor,
        limit: args.limit.map(|l| l.clamp(1, PROPOSALS_MAX_LIMIT)),
    };
    Ok(tools::response_result(ops.proposals(query).await?))
}

async fn whoami<O: Operations>(ops: &O, args: Map<String, Value>) -> Outcome {
    let _: NoArgs = args!(args);
    Ok(tools::response_result(ops.whoami().await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> OperationId {
        OperationId::new(raw).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_direct_tool_may_not_take_a_fixed_name() {
        // Every operation ID has a dot, so its tool name has a "__" no fixed tool has:
        // the check in `Server::new` holds by construction.
        for fixed in FIXED_TOOLS {
            assert!(OperationId::from_tool_name(fixed).is_err(), "{fixed}");
        }
    }

    #[test]
    fn a_direct_tool_name_fits_model_apis() {
        // 32 segments of "ab": 95 characters as an ID, 126 as a tool name.
        let long = ["ab"; 32].join(".");
        let mut config = ServerConfig::new("hypha", "0.1.0");
        config.direct_tools = vec![id(&long)];
        assert_eq!(
            Server::new(config).err(),
            Some(ConfigError::DirectToolNameTooLong(id(&long)))
        );
        // Exactly 64 is fine: each ".ab" becomes "__ab", so "abcd" and 15 of them.
        let edge = format!("abcd{}", ".ab".repeat(15));
        assert_eq!(id(&edge).tool_name().len(), 64);
        let mut config = ServerConfig::new("hypha", "0.1.0");
        config.direct_tools = vec![id(&edge)];
        assert!(Server::new(config).is_ok());
    }

    #[test]
    fn a_direct_tool_is_listed_once() {
        let mut config = ServerConfig::new("hypha", "0.1.0");
        config.direct_tools = vec![id("admin.orders.list"), id("admin.orders.list")];
        assert_eq!(
            Server::new(config).err(),
            Some(ConfigError::DuplicateDirectTool(id("admin.orders.list")))
        );
    }
}
