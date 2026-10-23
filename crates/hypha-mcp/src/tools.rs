//! The tools: their definitions (what the model reads), their arguments, and how an
//! operation's answer becomes a tool result.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::ops::{
    Access, CallMode, Mode, OperationResponse, OperationSchema, OperationSummary, ProposalStatus,
    Proposed,
};

pub const FIND_OPERATIONS: &str = "find_operations";
pub const DESCRIBE_OPERATION: &str = "describe_operation";
pub const CALL: &str = "call";
pub const GET_PROPOSAL: &str = "get_proposal";
pub const LIST_PROPOSALS: &str = "list_proposals";
pub const WHOAMI: &str = "whoami";

/// The fixed tools, in the order `tools/list` gives them.
pub const FIXED_TOOLS: &[&str] = &[
    FIND_OPERATIONS,
    DESCRIBE_OPERATION,
    CALL,
    GET_PROPOSAL,
    LIST_PROPOSALS,
    WHOAMI,
];

/// The `_meta` key a tool result carries the operation's HTTP status under.
pub const META_STATUS: &str = "hypha/status";

pub const FIND_DEFAULT_LIMIT: u32 = 10;
pub const FIND_MAX_LIMIT: u32 = 50;
pub const PROPOSALS_MAX_LIMIT: u32 = 100;

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn mode_schema() -> Value {
    json!({
        "type": "string",
        "enum": ["preview", "apply", "propose"],
        "default": "apply",
        "description": "preview: run the write and roll it back, answering exactly what it \
    would return and record; nothing changes. apply: do it (a proposal instead when your mode \
    for the operation is propose). propose: store it for a staff member to approve. Reads \
    ignore it.",
    })
}

fn idempotency_key_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": 255,
        "description": "Your key for this write (a UUID you make). Send the same key when you \
    retry it: the first answer is replayed and nothing is written twice. Without one, a retry \
    of a call that timed out may write again.",
    })
}

fn preview_token_schema() -> Value {
    json!({
        "type": "string",
        "description": "With mode \"apply\": the token a preview of this same call answered \
    (preview.token). The write then commits only if it does exactly what the preview showed \
    (else preview_stale, with the new preview).",
    })
}

fn read_only(title: &str) -> Value {
    json!({"title": title, "readOnlyHint": true, "openWorldHint": false})
}

/// The fixed tools' definitions.
pub fn fixed_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": FIND_OPERATIONS,
            "title": "Find operations",
            "description": "Search the store's admin operations you may use, by what you \
        want to do (\"cancel order\", \"stock level\", \"price list\"). Answers each match's \
        operation ID, summary, whether it reads or writes, and your mode for it: read, propose (an \
        apply becomes a proposal a staff member approves) or apply. Best matches first. Then use \
        describe_operation for its inputs. Operation IDs are dotted (admin.orders.list) and are \
        what describe_operation and call take; an operation that also has a direct tool shows \
        that tool's name (admin__orders__list: the same ID, each dot as two underscores).",
            "inputSchema": object_schema(json!({
                "query": {
                    "type": "string",
                    "description": "Words describing the job. Leave out to list everything \
        you may call, by operation ID.",
                },
                "access": {
                    "type": "string",
                    "enum": ["read", "write"],
                    "description": "Only reads, or only writes.",
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": FIND_MAX_LIMIT,
                    "default": FIND_DEFAULT_LIMIT,
                },
            }), &[]),
            "annotations": read_only("Find operations"),
        }),
        json!({
            "name": DESCRIBE_OPERATION,
            "title": "Describe an operation",
            "description": "Everything needed to call one operation: its path parameters, \
        query parameters and body as JSON Schema, what it answers, the error codes it can return \
        with their HTTP status, the scopes it needs and your mode for it. Read this before the \
        first call of an operation.",
            "inputSchema": object_schema(json!({
                "operation": {
                    "type": "string",
                    "description": "The operation ID, e.g. admin.products.update.",
                },
            }), &["operation"]),
            "annotations": read_only("Describe an operation"),
        }),
        json!({
            "name": CALL,
            "title": "Call an operation",
            "description": "Run any operation you may use, by its operation ID, with the \
        inputs describe_operation lists. Reads answer the data. For a write, send mode \"preview\" \
        first to see exactly what it would change, events and emails included, with nothing \
        saved; then \"apply\". Under a propose grant, apply stores a proposal for a staff member \
        to approve instead (the answer names it; follow it with get_proposal). A failure answers \
        Hypha's {code, error, details}: fix what they point to and retry. Give the dotted \
        operation ID (admin.orders.fetch), never a direct tool's name.",
            "inputSchema": object_schema(json!({
                "operation": {
                    "type": "string",
                    "description": "The operation ID, e.g. admin.orders.fetch.",
                },
                "params": {
                    "type": "object",
                    "description": "Path parameters by name, e.g. {\"id\": \"ord_…\"}.",
                },
                "query": {
                    "type": "object",
                    "description": "Query parameters by name, e.g. {\"limit\": 20}.",
                },
                "body": {
                    "description": "The request body, for operations that take one.",
                },
                "mode": mode_schema(),
                "idempotency_key": idempotency_key_schema(),
                "preview_token": preview_token_schema(),
            }), &["operation"]),
            "annotations": {
                "title": "Call an operation",
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": false,
                "openWorldHint": false,
            },
        }),
        json!({
            "name": GET_PROPOSAL,
            "title": "Get a proposal",
            "description": "One of your proposals: the operation and request, the preview \
        a staff member saw, and its status (pending, applied, rejected, expired or failed) with \
        the decider's note and, once applied, the response.",
            "inputSchema": object_schema(json!({
                "proposal": {
                    "type": "string",
                    "description": "The proposal's ID (prop_…).",
                },
            }), &["proposal"]),
            "annotations": read_only("Get a proposal"),
        }),
        json!({
            "name": LIST_PROPOSALS,
            "title": "List proposals",
            "description": "Your proposals, newest first, to learn which were approved, \
        rejected or expired. Pass the answer's next cursor for the following page.",
            "inputSchema": object_schema(json!({
                "status": {
                    "type": "string",
                    "enum": ["pending", "applying", "applied", "rejected", "expired", "failed"],
                },
                "cursor": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": PROPOSALS_MAX_LIMIT},
            }), &[]),
            "annotations": read_only("List proposals"),
        }),
        json!({
            "name": WHOAMI,
            "title": "Who am I",
            "description": "Who you act as and for whom, your grant (scopes and their \
        modes, customer data you may see), and the store: its name, currency and time zone. \
        Call it first to know what you may do.",
            "inputSchema": object_schema(json!({}), &[]),
            "annotations": read_only("Who am I"),
        }),
    ]
}

/// A direct tool: one operation under its own name, with `call`'s arguments minus the
/// operation, typed by its schema.
pub fn direct_definition(schema: &OperationSchema) -> Value {
    let summary = &schema.summary;
    let mut properties = Map::new();
    let mut required: Vec<&str> = Vec::new();
    // Each part stands alone with its own `$defs`; nested under `properties`, their
    // `#/$defs/…` would resolve against the input schema's root, so the root holds them.
    let mut defs = Map::new();
    if has_properties(&schema.params) {
        properties.insert("params".into(), hoist_defs(&schema.params, &mut defs));
        if has_required(&schema.params) {
            required.push("params");
        }
    }
    if has_properties(&schema.query) {
        properties.insert("query".into(), hoist_defs(&schema.query, &mut defs));
    }
    if let Some(body) = &schema.body {
        properties.insert("body".into(), hoist_defs(&body.schema, &mut defs));
        if body.required {
            required.push("body");
        }
    }
    if summary.access == Access::Write {
        properties.insert("mode".into(), mode_schema());
        properties.insert("idempotency_key".into(), idempotency_key_schema());
        properties.insert("preview_token".into(), preview_token_schema());
    }

    let mut description = summary.summary.clone();
    if let Some(more) = schema.description.as_deref().filter(|d| !d.is_empty()) {
        description.push_str("\n\n");
        description.push_str(more);
    }
    if summary.access == Access::Write {
        description.push_str(match summary.mode {
            Mode::Propose => {
                "\n\nThis changes the store. Your mode for it is propose: preview it \
first (mode \"preview\"), then apply, which stores a proposal for a staff member to \
approve; follow it with get_proposal."
            }
            _ => {
                "\n\nThis changes the store. Preview it first (mode \"preview\": nothing is \
saved, the answer shows exactly what would change), then apply."
            }
        });
    }

    let mut input = object_schema(Value::Object(properties), &required);
    if !defs.is_empty() {
        input["$defs"] = Value::Object(defs);
    }
    let read = summary.access == Access::Read;
    json!({
        "name": summary.id.tool_name(),
        "title": summary.summary,
        "description": description,
        "inputSchema": input,
        "annotations": {
            "title": summary.summary,
            "readOnlyHint": read,
            "destructiveHint": !read,
            "openWorldHint": false,
        },
    })
}

/// `part` without its `$defs`, which join `defs` (the same name is the same schema: both
/// come from one document's components).
fn hoist_defs(part: &Value, defs: &mut Map<String, Value>) -> Value {
    let mut part = part.clone();
    if let Some(Value::Object(own)) = part.as_object_mut().and_then(|p| p.remove("$defs")) {
        defs.extend(own);
    }
    part
}

fn has_properties(schema: &Value) -> bool {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|p| !p.is_empty())
}

fn has_required(schema: &Value) -> bool {
    schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|r| !r.is_empty())
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindArgs {
    pub query: Option<String>,
    pub access: Option<Access>,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescribeArgs {
    pub operation: String,
}

/// A direct tool's arguments, and `call`'s besides `operation`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectArgs {
    #[serde(default)]
    pub params: Map<String, Value>,
    #[serde(default)]
    pub query: Map<String, Value>,
    #[serde(default)]
    pub body: Option<Value>,
    #[serde(default)]
    pub mode: Option<CallMode>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub preview_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalArgs {
    pub proposal: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListProposalsArgs {
    pub status: Option<ProposalStatus>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

/// Reads a tool's arguments; a mismatch is a tool error the model can fix.
pub fn parse_args<T: serde::de::DeserializeOwned>(args: Map<String, Value>) -> Result<T, Value> {
    serde_json::from_value(Value::Object(args)).map_err(|e| {
        tool_error(
            "invalid_arguments",
            format!("Invalid arguments: {e}"),
            Value::Null,
        )
    })
}

/// The operations matching a query, best first; everything by ID without one.
pub fn rank<'a>(
    operations: &'a [OperationSummary],
    query: Option<&str>,
    access: Option<Access>,
) -> Vec<&'a OperationSummary> {
    let terms: Vec<String> = query.map(words).unwrap_or_default();
    let mut scored: Vec<(u32, &OperationSummary)> = operations
        .iter()
        .filter(|op| access.is_none_or(|a| op.access == a))
        .map(|op| (score(op, &terms), op))
        .filter(|(score, _)| terms.is_empty() || *score > 0)
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| sb.cmp(sa).then_with(|| a.id.cmp(&b.id)));
    scored.into_iter().map(|(_, op)| op).collect()
}

/// Per term: a whole segment of the ID weighs most, then a tag, then a word of the
/// summary; a term only found inside the ID (`stock` in `stocktake`) counts least.
fn score(op: &OperationSummary, terms: &[String]) -> u32 {
    let id = op.id.as_str().to_ascii_lowercase();
    let id_words = words(&id);
    let tag_words: Vec<String> = op.tags.iter().flat_map(|t| words(t)).collect();
    let summary_words = words(&op.summary);
    terms
        .iter()
        .map(|term| {
            let mut points = 0;
            if id_words.contains(term) {
                points += 4;
            } else if term.len() >= 3 && id.contains(term.as_str()) {
                points += 1;
            }
            if tag_words.contains(term) {
                points += 3;
            }
            if summary_words.contains(term) {
                points += 2;
            }
            points
        })
        .sum()
}

/// Lowercase words, split on anything not a letter or digit, each reduced to a
/// crude singular so `orders` finds `order`. Deduplicated, in order.
fn words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        let word = singular(&word.to_lowercase());
        if !out.contains(&word) {
            out.push(word);
        }
    }
    out
}

fn singular(word: &str) -> String {
    if word.len() > 4
        && let Some(stem) = word.strip_suffix("ies")
    {
        return format!("{stem}y");
    }
    if word.len() > 3 && word.ends_with('s') && !word.ends_with("ss") {
        return word[..word.len() - 1].to_string();
    }
    word.to_string()
}

/// One found operation; `direct` when it also has a tool of its own, whose name is
/// given beside the ID.
pub fn summary_json(op: &OperationSummary, direct: bool) -> Value {
    let mut found = json!({
        "operation": op.id,
        "summary": op.summary,
        "tags": op.tags,
        "access": op.access,
        "mode": op.mode,
    });
    if direct && let Value::Object(found) = &mut found {
        found.insert("tool".into(), json!(op.id.tool_name()));
    }
    found
}

pub fn schema_json(schema: &OperationSchema) -> Value {
    let op = &schema.summary;
    json!({
        "operation": op.id,
        "summary": op.summary,
        "description": schema.description,
        "tags": op.tags,
        "access": op.access,
        "mode": op.mode,
        "scopes": schema.scopes,
        "input": {
            "params": schema.params,
            "query": schema.query,
            "body": schema.body.as_ref().map(|b| json!({"schema": b.schema, "required": b.required})),
        },
        "output": schema.output,
        "errors": schema.errors.iter().map(|e| json!({
            "code": e.code,
            "status": e.status,
            "description": e.description,
        })).collect::<Vec<_>>(),
    })
}

/// A tool result holding JSON: the text for every client, `structuredContent` for
/// the ones that read it. Clients before 2026-07-28 take only an object there.
pub fn json_result(value: Value, is_error: bool) -> Value {
    let text = match &value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let mut result = Map::new();
    result.insert("content".into(), json!([{"type": "text", "text": text}]));
    if value.is_object() {
        result.insert("structuredContent".into(), value);
    }
    result.insert("isError".into(), json!(is_error));
    Value::Object(result)
}

/// A tool error in Hypha's own shape, for problems the server finds before Hypha
/// answers (bad arguments, an unknown operation).
pub fn tool_error(code: &str, message: String, details: Value) -> Value {
    json_result(
        json!({"code": code, "error": message, "details": details}),
        true,
    )
}

/// An operation's answer: its body as is, an error when the status isn't 2xx, the
/// status in `_meta`.
pub fn response_result(response: OperationResponse) -> Value {
    let is_error = !response.is_success();
    let status = response.status;
    let mut result = if response.body.is_null() {
        json_result(Value::String(format!("HTTP {status}, no body")), is_error)
    } else {
        json_result(response.body, is_error)
    };
    if let Value::Object(result) = &mut result {
        result.insert("_meta".into(), json!({META_STATUS: status}));
    }
    result
}

pub fn proposed_result(proposed: Proposed) -> Value {
    let message = match &proposed.url {
        Some(url) => format!(
            "Not applied: proposal {} is waiting for a staff member to approve it at {url}. \
Follow it with get_proposal.",
            proposed.proposal
        ),
        None => format!(
            "Not applied: proposal {} is waiting for a staff member to approve it. Follow it \
with get_proposal.",
            proposed.proposal
        ),
    };
    json_result(
        json!({
            "proposed": proposed.proposal,
            "url": proposed.url,
            "status": "pending",
            "message": message,
        }),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{BodySchema, OperationId};

    fn op(id: &str, summary: &str, tags: &[&str], access: Access) -> OperationSummary {
        OperationSummary {
            id: OperationId::new(id).unwrap_or_else(|e| panic!("{e}")),
            summary: summary.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            access,
            mode: if access == Access::Read {
                Mode::Read
            } else {
                Mode::Propose
            },
        }
    }

    fn catalog() -> Vec<OperationSummary> {
        vec![
            op(
                "admin.orders.list",
                "List orders",
                &["orders"],
                Access::Read,
            ),
            op(
                "admin.orders.cancel",
                "Cancel an order",
                &["orders"],
                Access::Write,
            ),
            op(
                "admin.products.update",
                "Update a product",
                &["products"],
                Access::Write,
            ),
            op(
                "admin.inventory.adjust_level",
                "Adjust a stock level",
                &["inventory"],
                Access::Write,
            ),
        ]
    }

    fn ids(ops: Vec<&OperationSummary>) -> Vec<&str> {
        ops.into_iter().map(|op| op.id.as_str()).collect()
    }

    #[test]
    fn without_a_query_everything_is_listed_by_id() {
        let ops = catalog();
        assert_eq!(
            ids(rank(&ops, None, None)),
            [
                "admin.inventory.adjust_level",
                "admin.orders.cancel",
                "admin.orders.list",
                "admin.products.update"
            ]
        );
    }

    #[test]
    fn an_id_segment_outranks_a_summary_word() {
        let ops = catalog();
        // "cancel" is a segment of one ID and a summary word of it too; "orders"
        // matches both orders operations, so the one matching both terms leads.
        assert_eq!(
            ids(rank(&ops, Some("cancel orders"), None)),
            ["admin.orders.cancel", "admin.orders.list"]
        );
    }

    #[test]
    fn plurals_find_singulars() {
        let ops = catalog();
        assert_eq!(
            ids(rank(&ops, Some("product"), None)),
            ["admin.products.update"]
        );
        assert_eq!(
            ids(rank(&ops, Some("ORDER"), None)),
            ["admin.orders.cancel", "admin.orders.list"]
        );
    }

    #[test]
    fn summary_words_match() {
        let ops = catalog();
        assert_eq!(
            ids(rank(&ops, Some("stock"), None)),
            ["admin.inventory.adjust_level"]
        );
    }

    #[test]
    fn nothing_matching_lists_nothing() {
        let ops = catalog();
        assert!(rank(&ops, Some("refund"), None).is_empty());
    }

    #[test]
    fn access_filters() {
        let ops = catalog();
        assert_eq!(
            ids(rank(&ops, Some("orders"), Some(Access::Read))),
            ["admin.orders.list"]
        );
    }

    #[test]
    fn ties_break_by_id() {
        let ops = catalog();
        assert_eq!(
            ids(rank(&ops, Some("admin"), None)),
            [
                "admin.inventory.adjust_level",
                "admin.orders.cancel",
                "admin.orders.list",
                "admin.products.update"
            ]
        );
    }

    #[test]
    fn singular_rules() {
        assert_eq!(singular("categories"), "category");
        assert_eq!(singular("orders"), "order");
        assert_eq!(singular("address"), "address");
        assert_eq!(singular("bus"), "bus");
    }

    #[test]
    fn a_direct_write_offers_mode_and_requires_what_the_operation_does() {
        let schema = OperationSchema {
            summary: op("admin.orders.cancel", "Cancel an order", &[], Access::Write),
            description: None,
            params: json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}),
            query: json!({"type": "object", "properties": {}}),
            body: Some(BodySchema {
                schema: json!({"type": "object"}),
                required: false,
            }),
            output: None,
            errors: vec![],
            scopes: vec!["orders:write".into()],
        };
        let tool = direct_definition(&schema);
        assert_eq!(tool["name"], "admin__orders__cancel");
        assert_eq!(tool["inputSchema"]["required"], json!(["params"]));
        assert_eq!(
            tool["inputSchema"]["properties"]["mode"]["default"],
            "apply"
        );
        assert!(tool["inputSchema"]["properties"].get("query").is_none());
        assert_eq!(tool["annotations"]["readOnlyHint"], false);
    }

    #[test]
    fn a_direct_tools_refs_resolve_from_its_input_schema() {
        let item = json!({"type": "object", "properties": {"sku": {"type": "string"}}});
        let schema = OperationSchema {
            summary: op("admin.products.update", "Update", &[], Access::Write),
            description: None,
            params: json!({"type": "object", "properties": {"id": {"$ref": "#/$defs/Id"}},
                "$defs": {"Id": {"type": "string"}}}),
            query: json!({"type": "object", "properties": {}}),
            body: Some(BodySchema {
                schema: json!({"allOf": [{"$ref": "#/$defs/Update"}],
                    "$defs": {"Update": {"type": "object",
                        "properties": {"item": {"$ref": "#/$defs/Item"}}},
                        "Item": item.clone(), "Id": {"type": "string"}}}),
                required: true,
            }),
            output: None,
            errors: vec![],
            scopes: vec![],
        };
        let input = &direct_definition(&schema)["inputSchema"];
        assert_eq!(input["$defs"]["Item"], item);
        assert_eq!(input["$defs"]["Id"], json!({"type": "string"}));
        assert!(input["properties"]["body"].get("$defs").is_none());
        assert!(input["properties"]["params"].get("$defs").is_none());
        assert_eq!(
            input["properties"]["body"]["allOf"][0]["$ref"],
            "#/$defs/Update"
        );
    }

    #[test]
    fn a_direct_read_has_no_mode() {
        let schema = OperationSchema {
            summary: op("admin.orders.list", "List orders", &[], Access::Read),
            description: Some("Newest first.".into()),
            params: json!({"type": "object", "properties": {}}),
            query: json!({"type": "object", "properties": {"limit": {"type": "integer"}}}),
            body: None,
            output: None,
            errors: vec![],
            scopes: vec![],
        };
        let tool = direct_definition(&schema);
        assert_eq!(tool["description"], "List orders\n\nNewest first.");
        assert_eq!(
            tool["inputSchema"]["properties"],
            json!({"query": {"type": "object", "properties": {"limit": {"type": "integer"}}}})
        );
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
    }

    #[test]
    fn an_error_status_is_a_tool_error_with_the_body_untouched() {
        let body =
            json!({"code": "insufficient_stock", "error": "Not enough", "details": {"short": 2}});
        let result = response_result(OperationResponse {
            status: 409,
            body: body.clone(),
        });
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"], body);
        assert_eq!(result["content"][0]["text"], body.to_string());
        assert_eq!(result["_meta"][META_STATUS], 409);
    }

    #[test]
    fn a_body_that_is_not_an_object_is_text_only() {
        let result = response_result(OperationResponse {
            status: 200,
            body: json!([1, 2]),
        });
        assert_eq!(result["isError"], false);
        assert!(result.get("structuredContent").is_none());
        assert_eq!(result["content"][0]["text"], "[1,2]");
    }

    #[test]
    fn an_empty_body_says_so() {
        let result = response_result(OperationResponse {
            status: 204,
            body: Value::Null,
        });
        assert_eq!(result["content"][0]["text"], "HTTP 204, no body");
        assert_eq!(result["isError"], false);
    }

    #[test]
    fn unknown_arguments_are_a_tool_error() {
        let args = json!({"params": {}, "extra": 1})
            .as_object()
            .cloned()
            .unwrap_or_default();
        let error = parse_args::<DirectArgs>(args).err();
        assert_eq!(
            error
                .as_ref()
                .map(|e| e["structuredContent"]["code"].clone()),
            Some(json!("invalid_arguments"))
        );
    }

    #[test]
    fn fixed_tools_are_listed_in_order() {
        let names: Vec<Value> = fixed_definitions()
            .into_iter()
            .map(|t| t["name"].clone())
            .collect();
        assert_eq!(
            names,
            FIXED_TOOLS.iter().map(|n| json!(n)).collect::<Vec<_>>()
        );
    }
}
