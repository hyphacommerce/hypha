//! The server end to end over a fake `Operations`, in both protocol eras.

use std::cell::RefCell;

use hypha_mcp::*;
use serde_json::{Value, json};

struct FakeOps {
    operations: Vec<OperationSummary>,
    calls: RefCell<Vec<OperationCall>>,
    fail: bool,
}

fn op(id: &str, summary: &str, tags: &[&str], access: Access, mode: Mode) -> OperationSummary {
    OperationSummary {
        id: OperationId::new(id).unwrap_or_else(|e| panic!("{e}")),
        summary: summary.into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        access,
        mode,
    }
}

impl FakeOps {
    fn new() -> Self {
        Self {
            operations: vec![
                op(
                    "admin.orders.list",
                    "List orders",
                    &["orders"],
                    Access::Read,
                    Mode::Read,
                ),
                op(
                    "admin.orders.cancel",
                    "Cancel an order",
                    &["orders"],
                    Access::Write,
                    Mode::Propose,
                ),
                op(
                    "admin.inventory.adjust_level",
                    "Adjust a stock level",
                    &["inventory"],
                    Access::Write,
                    Mode::Apply,
                ),
            ],
            calls: RefCell::new(Vec::new()),
            fail: false,
        }
    }

    fn calls(&self) -> Vec<OperationCall> {
        self.calls.borrow().clone()
    }
}

impl Operations for FakeOps {
    async fn visible(&self) -> Result<Vec<OperationSummary>, McpError> {
        if self.fail {
            return Err(McpError::Internal("database down".into()));
        }
        Ok(self.operations.clone())
    }

    async fn describe(&self, id: &OperationId) -> Result<OperationSchema, McpError> {
        let summary = self
            .operations
            .iter()
            .find(|op| &op.id == id)
            .cloned()
            .ok_or_else(|| McpError::OperationNotFound(id.clone()))?;
        let write = summary.access == Access::Write;
        Ok(OperationSchema {
            summary,
            description: Some("From the OpenAPI document.".into()),
            params: if write {
                json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]})
            } else {
                json!({"type": "object", "properties": {}})
            },
            query: if write {
                json!({"type": "object", "properties": {}})
            } else {
                json!({"type": "object", "properties": {"limit": {"type": "integer"}}})
            },
            body: write.then(|| BodySchema {
                schema: json!({"type": "object", "properties": {"delta": {"type": "integer"}}}),
                required: true,
            }),
            output: Some(json!({"type": "object"})),
            errors: vec![ErrorDoc {
                code: "insufficient_stock".into(),
                status: 409,
                description: None,
            }],
            scopes: vec!["inventory:write".into()],
        })
    }

    async fn call(&self, call: OperationCall) -> Result<OperationResult, McpError> {
        self.calls.borrow_mut().push(call.clone());
        let summary = self
            .operations
            .iter()
            .find(|op| op.id == call.operation)
            .ok_or_else(|| McpError::OperationNotFound(call.operation.clone()))?;
        Ok(match (summary.access, summary.mode, call.mode) {
            (Access::Read, ..) => OperationResult::Response(OperationResponse {
                status: 200,
                body: json!({"items": [{"id": "ord_1"}], "next_cursor": null}),
            }),
            (_, _, CallMode::Preview) => OperationResult::Response(OperationResponse {
                status: 200,
                body: json!({"preview": {"complete": true, "events": [], "token": "hypha_pv_1"}}),
            }),
            (_, Mode::Propose, _) | (_, _, CallMode::Propose) => {
                OperationResult::Proposed(Proposed {
                    proposal: "prop_1".into(),
                    url: Some("https://admin.example/agents/proposals/prop_1".into()),
                })
            }
            _ => OperationResult::Response(OperationResponse {
                status: 409,
                body: json!({
                    "code": "insufficient_stock",
                    "error": "Not enough stock",
                    "details": {"shortages": [{"variant": "var_1", "missing": 2}]},
                }),
            }),
        })
    }

    async fn whoami(&self) -> Result<OperationResponse, McpError> {
        Ok(OperationResponse {
            status: 200,
            body: json!({"agent": {"id": "agent_1"}, "store": {"currency": "EUR"}}),
        })
    }

    async fn proposal(&self, id: &str) -> Result<OperationResponse, McpError> {
        Ok(if id == "prop_1" {
            OperationResponse {
                status: 200,
                body: json!({"id": "prop_1", "status": "pending"}),
            }
        } else {
            OperationResponse {
                status: 404,
                body: json!({"code": "proposal_not_found", "error": "No such proposal", "details": null}),
            }
        })
    }

    async fn proposals(&self, query: ProposalQuery) -> Result<OperationResponse, McpError> {
        Ok(OperationResponse {
            status: 200,
            body: json!({"items": [], "status": query.status, "limit": query.limit}),
        })
    }
}

const MODERN: &str = "2026-07-28";

fn server() -> Server {
    let mut config = ServerConfig::new("hypha", "0.1.0");
    config.direct_tools = vec![
        OperationId::new("admin.orders.list").unwrap_or_else(|e| panic!("{e}")),
        OperationId::new("admin.inventory.adjust_level").unwrap_or_else(|e| panic!("{e}")),
        // Curated, but this caller can't see it.
        OperationId::new("admin.customers.list").unwrap_or_else(|e| panic!("{e}")),
    ];
    Server::new(config).unwrap_or_else(|e| panic!("{e}"))
}

fn send(ops: &FakeOps, body: &str, headers: Headers<'_>) -> McpResponse {
    pollster::block_on(server().handle(
        ops,
        McpRequest {
            body: body.as_bytes(),
            headers,
        },
    ))
}

/// A modern request with its `_meta` and the headers mirroring it.
fn modern(ops: &FakeOps, method: &str, params: Value) -> McpResponse {
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "1"},
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let name = params["name"].as_str().map(str::to_string);
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
    send(
        ops,
        &body,
        Headers {
            protocol_version: Some(MODERN),
            method: Some(method),
            name: name.as_deref(),
        },
    )
}

/// A legacy request, after `initialize` settled on 2025-11-25.
fn legacy(ops: &FakeOps, method: &str, params: Value) -> McpResponse {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
    send(
        ops,
        &body,
        Headers {
            protocol_version: Some("2025-11-25"),
            ..Headers::default()
        },
    )
}

fn call_tool(ops: &FakeOps, name: &str, arguments: Value) -> Value {
    let response = modern(
        ops,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    );
    assert_eq!(response.status, 200, "{response:?}");
    response
        .body
        .map(|b| b["result"].clone())
        .unwrap_or(Value::Null)
}

fn body(response: &McpResponse) -> &Value {
    response
        .body
        .as_ref()
        .unwrap_or_else(|| panic!("no body: {response:?}"))
}

fn tool_names(result: &Value) -> Vec<String> {
    result["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|t| t["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn lists_the_fixed_tools_then_the_visible_direct_ones() {
    let ops = FakeOps::new();
    let response = modern(&ops, "tools/list", json!({}));
    assert_eq!(response.status, 200);
    let result = &body(&response)["result"];
    assert_eq!(
        tool_names(result),
        [
            "find_operations",
            "describe_operation",
            "call",
            "get_proposal",
            "list_proposals",
            "whoami",
            "admin.orders.list",
            "admin.inventory.adjust_level",
        ]
    );
    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["cacheScope"], "private");
    assert_eq!(result["ttlMs"], 60_000);
    assert_eq!(
        result["_meta"]["io.modelcontextprotocol/serverInfo"],
        json!({"name": "hypha", "version": "0.1.0"})
    );
}

#[test]
fn a_direct_tool_carries_its_operation_schema() {
    let ops = FakeOps::new();
    let response = modern(&ops, "tools/list", json!({}));
    let tools = body(&response)["result"]["tools"].clone();
    let adjust = tools
        .as_array()
        .and_then(|t| {
            t.iter()
                .find(|t| t["name"] == "admin.inventory.adjust_level")
        })
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(adjust["inputSchema"]["required"], json!(["params", "body"]));
    assert_eq!(
        adjust["inputSchema"]["properties"]["body"],
        json!({"type": "object", "properties": {"delta": {"type": "integer"}}})
    );
}

#[test]
fn a_cursor_we_never_gave_is_invalid_params() {
    let ops = FakeOps::new();
    let response = modern(&ops, "tools/list", json!({"cursor": "abc"}));
    assert_eq!(body(&response)["error"]["code"], -32602);
}

#[test]
fn a_read_call_answers_the_body() {
    let ops = FakeOps::new();
    let result = call_tool(
        &ops,
        "call",
        json!({"operation": "admin.orders.list", "query": {"limit": 5}}),
    );
    assert_eq!(result["isError"], false);
    assert_eq!(
        result["structuredContent"],
        json!({"items": [{"id": "ord_1"}], "next_cursor": null})
    );
    assert_eq!(result["_meta"]["hypha/status"], 200);
    assert_eq!(result["resultType"], "complete");
    let calls = ops.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].query.get("limit"), Some(&json!(5)));
    assert_eq!(calls[0].mode, CallMode::Apply);
}

#[test]
fn a_hypha_error_is_a_tool_error_with_its_body() {
    let ops = FakeOps::new();
    let result = call_tool(
        &ops,
        "call",
        json!({"operation": "admin.inventory.adjust_level", "params": {"id": "inv_1"},
               "body": {"delta": -5}, "mode": "apply"}),
    );
    assert_eq!(result["isError"], true);
    assert_eq!(result["structuredContent"]["code"], "insufficient_stock");
    assert_eq!(
        result["structuredContent"]["details"],
        json!({"shortages": [{"variant": "var_1", "missing": 2}]})
    );
    assert_eq!(result["_meta"]["hypha/status"], 409);
}

#[test]
fn preview_mode_reaches_the_port() {
    let ops = FakeOps::new();
    let result = call_tool(
        &ops,
        "call",
        json!({"operation": "admin.inventory.adjust_level", "params": {"id": "inv_1"},
               "body": {"delta": 1}, "mode": "preview"}),
    );
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"]["preview"]["complete"], true);
    assert_eq!(ops.calls()[0].mode, CallMode::Preview);
    assert_eq!(ops.calls()[0].body, Some(json!({"delta": 1})));
}

#[test]
fn a_proposal_answers_its_id_and_link() {
    let ops = FakeOps::new();
    let result = call_tool(
        &ops,
        "call",
        json!({"operation": "admin.orders.cancel", "params": {"id": "ord_1"}}),
    );
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"]["proposed"], "prop_1");
    assert_eq!(
        result["structuredContent"]["url"],
        "https://admin.example/agents/proposals/prop_1"
    );
    assert_eq!(result["structuredContent"]["status"], "pending");
}

#[test]
fn a_direct_tool_calls_its_operation() {
    let ops = FakeOps::new();
    let result = call_tool(&ops, "admin.orders.list", json!({"query": {"limit": 2}}));
    assert_eq!(result["isError"], false);
    let calls = ops.calls();
    assert_eq!(calls[0].operation.as_str(), "admin.orders.list");
    assert_eq!(calls[0].query.get("limit"), Some(&json!(2)));
}

#[test]
fn a_direct_tool_the_caller_cannot_see_is_unknown() {
    let ops = FakeOps::new();
    let response = modern(
        &ops,
        "tools/call",
        json!({"name": "admin.customers.list", "arguments": {}}),
    );
    assert_eq!(response.status, 200);
    assert_eq!(
        body(&response)["error"],
        json!({"code": -32602, "message": "Unknown tool: admin.customers.list"})
    );
    assert!(ops.calls().is_empty());
}

#[test]
fn an_unknown_tool_is_invalid_params() {
    let ops = FakeOps::new();
    let response = modern(
        &ops,
        "tools/call",
        json!({"name": "drop_tables", "arguments": {}}),
    );
    assert_eq!(body(&response)["error"]["code"], -32602);
    assert_eq!(body(&response)["id"], 1);
}

#[test]
fn an_operation_the_caller_cannot_see_is_a_tool_error() {
    let ops = FakeOps::new();
    let result = call_tool(&ops, "call", json!({"operation": "admin.users.create"}));
    assert_eq!(result["isError"], true);
    assert_eq!(result["structuredContent"]["code"], "operation_not_found");
    assert_eq!(
        result["structuredContent"]["details"],
        json!({"operation": "admin.users.create"})
    );
}

#[test]
fn bad_arguments_are_a_tool_error() {
    let ops = FakeOps::new();
    let result = call_tool(
        &ops,
        "call",
        json!({"operation": "admin.orders.list", "mode": "yolo"}),
    );
    assert_eq!(result["isError"], true);
    assert_eq!(result["structuredContent"]["code"], "invalid_arguments");
    let result = call_tool(&ops, "call", json!({"operation": "admin orders"}));
    assert_eq!(result["structuredContent"]["code"], "invalid_arguments");
    assert!(ops.calls().is_empty());
}

#[test]
fn find_operations_ranks_what_the_caller_sees() {
    let ops = FakeOps::new();
    let result = call_tool(&ops, "find_operations", json!({"query": "cancel order"}));
    assert_eq!(
        result["structuredContent"],
        json!({
            "operations": [
                {"operation": "admin.orders.cancel", "summary": "Cancel an order",
                 "tags": ["orders"], "access": "write", "mode": "propose"},
                {"operation": "admin.orders.list", "summary": "List orders",
                 "tags": ["orders"], "access": "read", "mode": "read"},
            ],
            "matches": 2,
        })
    );
}

#[test]
fn find_operations_honours_its_limit() {
    let ops = FakeOps::new();
    let result = call_tool(&ops, "find_operations", json!({"limit": 1}));
    assert_eq!(result["structuredContent"]["matches"], 3);
    assert_eq!(
        result["structuredContent"]["operations"][0]["operation"],
        "admin.inventory.adjust_level"
    );
    assert_eq!(
        result["structuredContent"]["operations"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn describe_operation_answers_the_schema() {
    let ops = FakeOps::new();
    let result = call_tool(
        &ops,
        "describe_operation",
        json!({"operation": "admin.inventory.adjust_level"}),
    );
    let schema = &result["structuredContent"];
    assert_eq!(schema["mode"], "apply");
    assert_eq!(schema["input"]["body"]["required"], true);
    assert_eq!(
        schema["errors"],
        json!([{"code": "insufficient_stock", "status": 409, "description": null}])
    );
    assert_eq!(schema["scopes"], json!(["inventory:write"]));
}

#[test]
fn whoami_and_proposals_pass_through() {
    let ops = FakeOps::new();
    let me = call_tool(&ops, "whoami", json!({}));
    assert_eq!(me["structuredContent"]["store"]["currency"], "EUR");

    let found = call_tool(&ops, "get_proposal", json!({"proposal": "prop_1"}));
    assert_eq!(found["structuredContent"]["status"], "pending");
    let missing = call_tool(&ops, "get_proposal", json!({"proposal": "prop_9"}));
    assert_eq!(missing["isError"], true);
    assert_eq!(missing["structuredContent"]["code"], "proposal_not_found");

    let page = call_tool(
        &ops,
        "list_proposals",
        json!({"status": "applied", "limit": 500}),
    );
    assert_eq!(page["structuredContent"]["status"], "applied");
    assert_eq!(page["structuredContent"]["limit"], 100);
}

#[test]
fn a_port_failure_is_an_internal_error_hiding_its_cause() {
    let mut ops = FakeOps::new();
    ops.fail = true;
    let response = modern(&ops, "tools/call", json!({"name": "find_operations"}));
    assert_eq!(response.status, 500);
    assert_eq!(
        body(&response)["error"],
        json!({"code": -32603, "message": "Internal error"})
    );
}

#[test]
fn discover_lists_both_eras() {
    let ops = FakeOps::new();
    let response = modern(&ops, "server/discover", json!({}));
    let result = &body(&response)["result"];
    assert_eq!(
        result["supportedVersions"],
        json!(["2026-07-28", "2025-11-25", "2025-06-18", "2025-03-26"])
    );
    assert_eq!(result["capabilities"], json!({"tools": {}}));
    assert_eq!(result["instructions"], DEFAULT_INSTRUCTIONS);
    assert_eq!(result["cacheScope"], "public");
    assert_eq!(result["resultType"], "complete");
}

#[test]
fn an_unknown_modern_method_is_404() {
    let ops = FakeOps::new();
    let response = modern(&ops, "resources/list", json!({}));
    assert_eq!(response.status, 404);
    assert_eq!(body(&response)["error"]["code"], -32601);
    // Gone from the modern protocol.
    let response = modern(&ops, "ping", json!({}));
    assert_eq!(response.status, 404);
}

#[test]
fn an_unsupported_version_is_400_with_ours() {
    let ops = FakeOps::new();
    let body_text = json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": {"_meta": {
        "io.modelcontextprotocol/protocolVersion": "2031-01-01",
        "io.modelcontextprotocol/clientCapabilities": {},
    }}})
    .to_string();
    let response = send(
        &ops,
        &body_text,
        Headers {
            protocol_version: Some("2031-01-01"),
            method: Some("tools/list"),
            name: None,
        },
    );
    assert_eq!(response.status, 400);
    assert_eq!(body(&response)["error"]["code"], -32022);
    assert_eq!(body(&response)["error"]["data"]["requested"], "2031-01-01");
}

#[test]
fn a_mismatched_name_header_is_400() {
    let ops = FakeOps::new();
    let body_text = json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {
        "name": "whoami",
        "_meta": {
            "io.modelcontextprotocol/protocolVersion": MODERN,
            "io.modelcontextprotocol/clientCapabilities": {},
        },
    }})
    .to_string();
    let response = send(
        &ops,
        &body_text,
        Headers {
            protocol_version: Some(MODERN),
            method: Some("tools/call"),
            name: Some("call"),
        },
    );
    assert_eq!(response.status, 400);
    assert_eq!(body(&response)["error"]["code"], -32020);
}

#[test]
fn legacy_initialize_negotiates_and_describes_the_server() {
    let ops = FakeOps::new();
    let body_text = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {"name": "claude-ai", "version": "0.1.0"},
    }})
    .to_string();
    let response = send(&ops, &body_text, Headers::default());
    assert_eq!(response.status, 200);
    assert_eq!(
        body(&response),
        &json!({"jsonrpc": "2.0", "id": 0, "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "hypha", "version": "0.1.0"},
            "instructions": DEFAULT_INSTRUCTIONS,
        }})
    );
}

#[test]
fn legacy_tools_list_has_no_modern_fields() {
    let ops = FakeOps::new();
    let response = legacy(&ops, "tools/list", json!({}));
    let result = &body(&response)["result"];
    assert_eq!(tool_names(result).len(), 8);
    assert!(result.get("resultType").is_none());
    assert!(result.get("ttlMs").is_none());
    assert!(result.get("_meta").is_none());
}

#[test]
fn legacy_tools_call_works_without_meta() {
    let ops = FakeOps::new();
    let response = legacy(
        &ops,
        "tools/call",
        json!({"name": "whoami", "arguments": {}}),
    );
    let result = &body(&response)["result"];
    assert_eq!(result["isError"], false);
    assert!(result.get("resultType").is_none());
}

#[test]
fn legacy_ping_answers_empty() {
    let ops = FakeOps::new();
    let response = legacy(&ops, "ping", json!({}));
    assert_eq!(
        body(&response),
        &json!({"jsonrpc": "2.0", "id": 1, "result": {}})
    );
}

#[test]
fn an_unknown_legacy_method_is_an_error_in_a_200() {
    let ops = FakeOps::new();
    let response = legacy(&ops, "prompts/list", json!({}));
    assert_eq!(response.status, 200);
    assert_eq!(body(&response)["error"]["code"], -32601);
}

#[test]
fn notifications_get_no_response() {
    let ops = FakeOps::new();
    for body_text in [
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
        r#"{"jsonrpc":"2.0","id":9,"result":{}}"#,
    ] {
        let response = send(&ops, body_text, Headers::default());
        assert_eq!(
            response,
            McpResponse {
                status: 202,
                body: None
            }
        );
    }
}

#[test]
fn malformed_json_is_a_400_parse_error() {
    let ops = FakeOps::new();
    let response = send(&ops, "{not json", Headers::default());
    assert_eq!(response.status, 400);
    assert_eq!(body(&response)["error"]["code"], -32700);
    assert!(body(&response).get("id").is_none());
}

#[test]
fn a_batch_is_refused() {
    let ops = FakeOps::new();
    let response = send(
        &ops,
        r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#,
        Headers::default(),
    );
    assert_eq!(response.status, 400);
    assert_eq!(body(&response)["error"]["code"], -32600);
}
