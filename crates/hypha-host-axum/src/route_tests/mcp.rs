//! `/mcp` over HTTP: the protocol as clients speak it (both eras), what an agent sees and
//! does through it (reads, previews, proposals, applies, errors, the cap), its transport
//! rules (credentials, `Origin`, methods, the kill switch), and connecting an MCP client
//! with OAuth from its metadata document to a refreshed token.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header::WWW_AUTHENTICATE},
    routing::get,
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::oauth::{encode, query_param, raw};
use super::*;

const MODERN: &str = "2026-07-28";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

/// An autonomous agent with `grant`, made by the owner's session; its key.
async fn agent_key(app: &TestApp, grant: Value) -> (String, String) {
    let session = owner_session(app).await;
    let body = json!({"name": "Shop helper", "grant": grant}).to_string();
    let (status, json) = send_as(app, &session, "POST", "/admin/agents", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    (
        json["id"].as_str().unwrap().to_string(),
        json["key"].as_str().unwrap().to_string(),
    )
}

/// What `/mcp` answered: the status, the `WWW-Authenticate` header and the body.
struct Answered {
    status: StatusCode,
    challenge: Option<String>,
    body: Value,
}

/// One POST to `/mcp` with `headers` and `body` (as is).
async fn post(app: &TestApp, headers: &[(&str, &str)], body: &str) -> Answered {
    let mut request = Request::builder().method("POST").uri("/mcp");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request.body(Body::from(body.to_string())).unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let challenge = response
        .headers()
        .get(WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    Answered {
        status,
        challenge,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

/// A 2026-07-28 request as `token`: `_meta` and the headers mirroring it.
async fn modern(app: &TestApp, token: &str, method: &str, params: Value) -> Answered {
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientInfo": {"name": "tests", "version": "1"},
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let name = params["name"].as_str().map(str::to_string);
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let auth = format!("Bearer {token}");
    let mut headers = vec![
        ("authorization", auth.as_str()),
        ("content-type", "application/json"),
        ("accept", "application/json, text/event-stream"),
        ("mcp-protocol-version", MODERN),
        ("mcp-method", method),
    ];
    if let Some(name) = &name {
        headers.push(("mcp-name", name.as_str()));
    }
    post(app, &headers, &body.to_string()).await
}

/// A tool's result, as `token`.
async fn tool(app: &TestApp, token: &str, name: &str, arguments: Value) -> Value {
    let answered = modern(
        app,
        token,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
    .await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    answered.body["result"].clone()
}

/// The names `tools/list` gives `token`.
async fn tool_names(app: &TestApp, token: &str) -> Vec<String> {
    let answered = modern(app, token, "tools/list", json!({})).await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    names(&answered.body)
}

fn names(body: &Value) -> Vec<String> {
    body["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("no tools: {body}"))
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

fn mug(handle: &str) -> Value {
    json!({"handle": handle, "title": "Mug", "price": {"amount": 1000, "currency": "EUR"}})
}

#[tokio::test]
async fn tools_list_follows_the_grant_in_both_protocol_eras() {
    let app = app().await;
    let (_, catalog) = agent_key(
        &app,
        json!({"read": ["orders:read"], "write": {"products:write": "propose"}}),
    )
    .await;
    let (_, support) = agent_key(&app, json!({"read": ["customers:read"]})).await;
    let (_, stock) = agent_key(&app, json!({"read": ["inventory:read", "products:read"]})).await;

    let tools = tool_names(&app, &catalog).await;
    for fixed in [
        "find_operations",
        "describe_operation",
        "call",
        "get_proposal",
        "list_proposals",
        "whoami",
    ] {
        assert!(tools.contains(&fixed.to_string()), "{fixed}: {tools:?}");
    }
    assert!(tools.contains(&"admin__orders__list".to_string()));
    assert!(tools.contains(&"admin__products__update".to_string()));
    assert!(!tools.contains(&"admin__customers__list".to_string()));

    // What's running low, across products.
    let tools = tool_names(&app, &stock).await;
    assert!(tools.contains(&"admin__inventory__list_items".to_string()));

    let tools = tool_names(&app, &support).await;
    assert!(tools.contains(&"admin__customers__list".to_string()));
    assert!(!tools.contains(&"admin__orders__list".to_string()));
    assert!(!tools.contains(&"admin__products__update".to_string()));

    // A client of the handshake era: initialize, then tools/list under its version.
    let auth = format!("Bearer {support}");
    let initialize = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-11-25", "capabilities": {},
        "clientInfo": {"name": "old", "version": "1"}}});
    let headers = [
        ("authorization", auth.as_str()),
        ("content-type", "application/json"),
    ];
    let answered = post(&app, &headers, &initialize.to_string()).await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    assert_eq!(answered.body["result"]["protocolVersion"], "2025-11-25");
    assert!(
        answered.body["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("whoami")
    );
    let headers = [
        ("authorization", auth.as_str()),
        ("content-type", "application/json"),
        ("mcp-protocol-version", "2025-11-25"),
    ];
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    let answered = post(&app, &headers, &list.to_string()).await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    let tools = names(&answered.body);
    assert!(tools.contains(&"admin__customers__list".to_string()));
    assert!(!tools.contains(&"admin__products__update".to_string()));
}

#[tokio::test]
async fn every_tools_refs_resolve_from_its_input_schema() {
    let app = app().await;
    let (_, token) = agent_key(
        &app,
        json!({
            "read": ["orders:read", "customers:read", "inventory:read", "products:read"],
            "write": {"products:write": "apply", "orders:write": "apply",
                "customers:write": "apply", "inventory:write": "apply"},
        }),
    )
    .await;
    let answered = modern(&app, &token, "tools/list", json!({})).await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    let tools = answered.body["result"]["tools"].as_array().unwrap();
    assert!(tools.len() > 12, "{}", tools.len());
    fn refs<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(target)) = map.get("$ref") {
                    found.push(target);
                }
                map.values().for_each(|v| refs(v, found));
            }
            Value::Array(items) => items.iter().for_each(|v| refs(v, found)),
            _ => {}
        }
    }
    let mut checked = 0;
    for tool in tools {
        let input = &tool["inputSchema"];
        let mut found = Vec::new();
        refs(input, &mut found);
        for target in found {
            let pointer = target
                .strip_prefix('#')
                .unwrap_or_else(|| panic!("{target}"));
            assert!(
                input.pointer(pointer).is_some(),
                "{}: {target} doesn't resolve",
                tool["name"]
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no tool uses a $ref");
}

#[tokio::test]
async fn an_agent_finds_describes_and_reads() {
    let app = app().await;
    let (_, key) = agent_key(
        &app,
        json!({"read": ["orders:read", "products:read"], "write": {"products:write": "propose"}}),
    )
    .await;

    let found = tool(
        &app,
        &key,
        "find_operations",
        json!({"query": "list orders"}),
    )
    .await;
    let operations = found["structuredContent"]["operations"].as_array().unwrap();
    let list = operations
        .iter()
        .find(|o| o["operation"] == "admin.orders.list")
        .unwrap_or_else(|| panic!("{found}"));
    assert_eq!(list["mode"], "read");
    assert_eq!(list["tool"], "admin__orders__list");
    let all = tool(&app, &key, "find_operations", json!({"limit": 50})).await;
    let listed = all["structuredContent"]["operations"].as_array().unwrap();
    assert!(
        listed
            .iter()
            .all(|o| o["operation"] != "admin.customers.list" && o["operation"] != "admin.me.fetch"),
        "only what it may call: {all}"
    );

    let described = tool(
        &app,
        &key,
        "describe_operation",
        json!({"operation": "admin.products.update"}),
    )
    .await;
    let schema = &described["structuredContent"];
    assert_eq!(schema["mode"], "propose");
    assert_eq!(schema["input"]["params"]["required"], json!(["id"]));
    assert!(schema["input"]["body"]["schema"].is_object(), "{schema}");
    assert_eq!(schema["scopes"], json!(["products:write"]));
    assert!(!schema["errors"].as_array().unwrap().is_empty());
    assert!(
        !described.to_string().contains("#/components/"),
        "self-contained"
    );
    let hidden = tool(
        &app,
        &key,
        "describe_operation",
        json!({"operation": "admin.customers.list"}),
    )
    .await;
    assert_eq!(hidden["isError"], true);
    assert_eq!(hidden["structuredContent"]["code"], "operation_not_found");

    create_product(&app, "kettle").await;
    let read = tool(
        &app,
        &key,
        "call",
        json!({"operation": "admin.products.list", "query": {"limit": 5}}),
    )
    .await;
    assert_eq!(read["isError"], false, "{read}");
    assert_eq!(read["_meta"]["hypha/status"], 200);
    assert_eq!(read["structuredContent"]["items"][0]["handle"], "kettle");
    let direct = tool(&app, &key, "admin__orders__list", json!({})).await;
    assert_eq!(direct["isError"], false, "{direct}");

    let me = tool(&app, &key, "whoami", json!({})).await;
    let me = &me["structuredContent"];
    assert_eq!(me["agent"]["kind"], "autonomous");
    assert_eq!(
        me["permissions"]["write"],
        json!({"products:write": "propose"})
    );
    assert_eq!(me["writes_cap"], "apply");
    assert_eq!(me["store"]["name"], "Test");
}

#[tokio::test]
async fn an_agent_previews_proposes_and_a_person_approves() {
    let app = app_configured("[server]\nadmin_url = \"http://admin.test\"").await;
    let (agent, key) = agent_key(
        &app,
        json!({"read": ["products:read"], "write": {"products:write": "propose"}}),
    )
    .await;

    let preview = tool(
        &app,
        &key,
        "call",
        json!({"operation": "admin.products.create", "body": mug("mug"), "mode": "preview"}),
    )
    .await;
    assert_eq!(preview["isError"], false, "{preview}");
    let previewed = &preview["structuredContent"]["preview"];
    assert_eq!(previewed["complete"], true);
    assert_eq!(previewed["response"]["status"], 201);
    let (status, products) = send(&app, "GET", "/admin/products", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(products["items"], json!([]), "a preview keeps nothing");

    // Its mode is propose: applying makes a proposal.
    let proposed = tool(
        &app,
        &key,
        "admin__products__update",
        json!({"params": {"id": "prod_00000000000000000000000000000063"}, "body": {"title": "X"}}),
    )
    .await;
    assert_eq!(
        proposed["isError"], true,
        "a failing preview isn't proposed"
    );
    let proposed = tool(
        &app,
        &key,
        "call",
        json!({"operation": "admin.products.create", "body": mug("mug")}),
    )
    .await;
    assert_eq!(proposed["isError"], false, "{proposed}");
    let id = proposed["structuredContent"]["proposed"]
        .as_str()
        .unwrap_or_else(|| panic!("{proposed}"))
        .to_string();
    assert!(id.starts_with("prop_"));
    assert_eq!(
        proposed["structuredContent"]["url"],
        format!("http://admin.test/agents/proposals/{id}")
    );
    let pending = tool(&app, &key, "get_proposal", json!({"proposal": id})).await;
    assert_eq!(
        pending["structuredContent"]["status"], "pending",
        "{pending}"
    );
    let listed = tool(&app, &key, "list_proposals", json!({"status": "pending"})).await;
    assert_eq!(listed["structuredContent"]["items"][0]["id"], json!(id));

    let session = owner_session(&app).await;
    let path = format!("/admin/agent-proposals/{id}/approve");
    let (status, approved) = send_as(&app, &session, "POST", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let applied = tool(&app, &key, "get_proposal", json!({"proposal": id})).await;
    assert_eq!(applied["structuredContent"]["status"], "applied");
    let (_, products) = send(&app, "GET", "/admin/products", None).await;
    assert_eq!(products["items"][0]["handle"], "mug");

    // Hypha's error body comes back as a tool error.
    let duplicate = tool(
        &app,
        &key,
        "call",
        json!({"operation": "admin.products.create", "body": mug("mug"), "mode": "preview"}),
    )
    .await;
    assert_eq!(duplicate["isError"], true);
    assert_eq!(duplicate["_meta"]["hypha/status"], 409);
    assert!(
        duplicate["structuredContent"]["code"].is_string(),
        "{duplicate}"
    );
    assert!(duplicate["structuredContent"]["error"].is_string());

    // Each call is in the agent's log.
    let (_, calls) = send(&app, "GET", &format!("/admin/agents/{agent}/calls"), None).await;
    let modes: Vec<&str> = calls["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["mode"].as_str().unwrap())
        .collect();
    assert!(
        modes.contains(&"preview") && modes.contains(&"propose"),
        "{modes:?}"
    );
}

/// Erasing is only ever proposed; its event's due time moves between the proposal and
/// the approval, and the approval still applies it.
#[tokio::test]
async fn an_erasure_an_agent_proposed_is_applied_when_approved() {
    let app = app().await;
    let (order, _, _) = placed_order_for_three_mugs(&app).await;
    let (_, placed) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    let buyer = placed["customer"].as_str().unwrap().to_string();
    let (_, key) = agent_key(
        &app,
        json!({"read": ["customers:read"], "write": {"customers:erase": "propose"}}),
    )
    .await;
    let proposed = tool(
        &app,
        &key,
        "call",
        json!({"operation": "admin.customers.request_erasure", "params": {"id": buyer}}),
    )
    .await;
    assert_eq!(proposed["isError"], false, "{proposed}");
    let id = proposed["structuredContent"]["proposed"].as_str().unwrap();

    let session = owner_session(&app).await;
    let path = format!("/admin/agent-proposals/{id}/approve");
    let (status, approved) = send_as(&app, &session, "POST", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    assert_eq!(approved["status"], "applied", "{approved}");
    let (_, customer) = send(&app, "GET", &format!("/admin/customers/{buyer}"), None).await;
    assert!(customer["erasure_due_at"].is_string(), "{customer}");
}

#[tokio::test]
async fn an_agent_applies_exactly_what_it_previewed_once() {
    let app = app().await;
    let (_, key) = agent_key(
        &app,
        json!({"read": ["products:read"], "write": {"products:write": "apply"}}),
    )
    .await;
    let preview = tool(
        &app,
        &key,
        "admin__products__update",
        json!({"params": {"id": create_product(&app, "pot").await},
               "body": {"title": "Big pot"}, "mode": "preview"}),
    )
    .await;
    let token = preview["structuredContent"]["preview"]["token"]
        .as_str()
        .unwrap_or_else(|| panic!("{preview}"))
        .to_string();
    let (_, products) = send(&app, "GET", "/admin/products", None).await;
    let id = products["items"][0]["id"].as_str().unwrap().to_string();
    let apply = |key: &'static str| {
        json!({"operation": "admin.products.update", "params": {"id": id},
               "body": {"title": "Big pot"}, "idempotency_key": key})
    };
    let mut first = apply("retry-1");
    first["preview_token"] = json!(token);
    let applied = tool(&app, &key, "call", first).await;
    assert_eq!(applied["isError"], false, "{applied}");
    assert_eq!(applied["structuredContent"]["title"], "Big pot");
    // A preview's token applies once.
    let mut again = apply("retry-2");
    again["preview_token"] = json!(token);
    let again = tool(&app, &key, "call", again).await;
    assert_eq!(again["isError"], true, "{again}");
    assert_eq!(again["structuredContent"]["code"], "preview_used");
}

#[tokio::test]
async fn under_the_cap_off_agents_only_read() {
    let app = app_configured("[agents]\nwrites = \"off\"").await;
    let (_, key) = agent_key(
        &app,
        json!({"read": ["products:read"], "write": {"products:write": "propose"}}),
    )
    .await;
    let tools = tool_names(&app, &key).await;
    assert!(tools.contains(&"admin__products__list".to_string()));
    assert!(!tools.contains(&"admin__products__update".to_string()));
    let refused = tool(
        &app,
        &key,
        "call",
        json!({"operation": "admin.products.create", "body": mug("mug"), "mode": "preview"}),
    )
    .await;
    assert_eq!(refused["isError"], true);
    assert_eq!(refused["structuredContent"]["code"], "agent_writes_off");
}

#[tokio::test]
async fn mcp_takes_only_agents_from_our_origins() {
    let app = app().await;
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
    let json_only = [("content-type", "application/json")];
    let answered = post(&app, &json_only, &list).await;
    assert_eq!(answered.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        answered.challenge.as_deref(),
        Some(
            "Bearer resource_metadata=\"http://localhost/.well-known/oauth-protected-resource/mcp\""
        )
    );
    for token in [app.admin_key.clone(), owner_session(&app).await] {
        let auth = format!("Bearer {token}");
        let headers = [
            ("authorization", auth.as_str()),
            ("content-type", "application/json"),
        ];
        let answered = post(&app, &headers, &list).await;
        assert_eq!(
            answered.status,
            StatusCode::UNAUTHORIZED,
            "{}",
            answered.body
        );
        assert!(
            answered
                .challenge
                .unwrap()
                .contains("error=\"invalid_token\"")
        );
    }

    let (agent, key) = agent_key(&app, json!({"read": ["orders:read"]})).await;
    let auth = format!("Bearer {key}");
    let foreign = [
        ("authorization", auth.as_str()),
        ("content-type", "application/json"),
        ("origin", "https://evil.example"),
    ];
    let answered = post(&app, &foreign, &list).await;
    assert_eq!(answered.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&answered.body), "origin_not_allowed");
    let own = [
        ("authorization", auth.as_str()),
        ("content-type", "application/json"),
        ("origin", "http://localhost"),
        ("mcp-protocol-version", "2025-11-25"),
    ];
    assert_eq!(post(&app, &own, &list).await.status, StatusCode::OK);
    let text = [
        ("authorization", auth.as_str()),
        ("content-type", "text/plain"),
    ];
    assert_eq!(
        post(&app, &text, &list).await.status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    let (status, _) = send_as(&app, &key, "GET", "/mcp", None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    let (status, _) = send_as(&app, &key, "DELETE", "/mcp", None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    // A disabled agent stops at once.
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/admin/agents/{agent}"),
        Some(r#"{"enabled": false}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let answered = post(&app, &own, &list).await;
    assert_eq!(answered.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&answered.body), "agent_disabled");

    for path in [
        "/.well-known/oauth-protected-resource",
        "/.well-known/oauth-protected-resource/mcp",
    ] {
        let (status, metadata) = send_with(&app, &[], "GET", path, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(metadata["resource"], "http://localhost/mcp");
        assert_eq!(
            metadata["authorization_servers"],
            json!(["http://localhost"])
        );
    }
}

#[tokio::test]
async fn with_agents_turned_off_mcp_refuses() {
    let app = app_configured("[agents]\nenabled = false").await;
    let answered = post(
        &app,
        &[("content-type", "application/json")],
        r#"{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}"#,
    )
    .await;
    assert_eq!(answered.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&answered.body), "agents_disabled");
}

/// An MCP client: serves its Client ID Metadata Document at `/client.json`. Its URL.
async fn mcp_client() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let document = json!({
        "client_id": format!("{base}/client.json"),
        "client_name": "Test assistant",
        "client_uri": "https://assistant.example",
        "redirect_uris": ["http://127.0.0.1/callback"],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    })
    .to_string();
    let router = Router::new().route(
        "/client.json",
        get(move || {
            let document = document.clone();
            async move { ([(CONTENT_TYPE, "application/json")], document) }
        }),
    );
    tokio::spawn(async move { axum::serve(listener, router).await });
    format!("{base}/client.json")
}

/// The token endpoint, form-encoded.
async fn token(app: &TestApp, pairs: &[(&str, &str)]) -> (StatusCode, Value) {
    let (status, _, body) = raw(app, "POST", "/oauth/token", Some(encode(pairs))).await;
    (status, body)
}

#[tokio::test]
async fn an_mcp_client_connects_with_oauth_and_acts_as_its_agent() {
    let app = super::oauth::oauth_app().await;
    let client = mcp_client().await;
    // A loopback redirect may take any port (a native client listens where it can).
    let redirect = "http://127.0.0.1:53682/callback";
    let authorize = format!(
        "/oauth/authorize?{}",
        encode(&[
            ("response_type", "code"),
            ("client_id", &client),
            ("redirect_uri", redirect),
            ("state", "s1"),
            ("code_challenge", CHALLENGE),
            ("code_challenge_method", "S256"),
            ("resource", "http://localhost/mcp"),
            ("scope", "orders:read offline_access"),
        ])
    );
    let (status, location, body) = raw(&app, "GET", &authorize, None).await;
    assert_eq!(status, StatusCode::FOUND, "{body}");
    let location = location.unwrap();
    assert!(
        location.starts_with("http://admin.test/agents/connect?request=oareq_"),
        "{location}"
    );
    let request = query_param(&location, "request").unwrap();

    // Another resource is refused: tokens are for /mcp only.
    let elsewhere = authorize.replace("%2Fmcp", "%2Fadmin");
    let (status, _, body) = raw(&app, "GET", &elsewhere, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_target")
    );
    // A redirect the document doesn't list never gets a code.
    let stranger = authorize.replace("127.0.0.1%3A53682", "evil.example");
    let (status, _, body) = raw(&app, "GET", &stranger, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_redirect_uri")
    );

    // The consent page reads the request as the signed-in staff member.
    let session = owner_session(&app).await;
    let path = format!("/admin/agent-connections/{request}");
    let (status, connection) = send_as(&app, &session, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{connection}");
    assert_eq!(connection["status"], "pending");
    assert_eq!(connection["client"]["name"], "Test assistant");
    assert_eq!(connection["client"]["redirect_host"], "127.0.0.1");
    assert_eq!(connection["client"]["loopback_only"], true);
    assert_eq!(connection["requested_scopes"], json!(["orders:read"]));
    assert_eq!(
        connection["suggested_grant"],
        json!({"read": ["orders:read"], "write": {}, "customer_data": [], "operations": null})
    );
    // A key isn't a person: connecting is a staff session's act.
    let (status, _) = send(&app, "GET", &path, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let approve = json!({
        "name": "Claude on my laptop",
        "grant": {"read": ["orders:read", "products:read"]},
    })
    .to_string();
    let (status, answer) = send_as(
        &app,
        &session,
        "POST",
        &format!("{path}/approve"),
        Some(&approve),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["agent"]["kind"], "connected");
    assert_eq!(answer["agent"]["client"]["client_id"], json!(client));
    let agent = answer["agent"]["id"].as_str().unwrap().to_string();
    let to = answer["redirect_to"].as_str().unwrap();
    assert!(to.starts_with(redirect), "{to}");
    assert_eq!(query_param(to, "state").as_deref(), Some("s1"));
    assert_eq!(query_param(to, "iss").as_deref(), Some("http://localhost"));
    let code_ = query_param(to, "code").unwrap();
    let (status, _) = send_as(
        &app,
        &session,
        "POST",
        &format!("{path}/approve"),
        Some(&approve),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "answered once");

    let (status, tokens) = token(
        &app,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code_),
            (
                "code_verifier",
                "wrong-verifier-wrong-verifier-wrong-verifier-x",
            ),
            ("client_id", &client),
            ("redirect_uri", redirect),
            ("resource", "http://localhost/mcp"),
        ],
    )
    .await;
    assert_eq!(
        (status, tokens["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    let (status, tokens) = token(
        &app,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code_),
            ("code_verifier", VERIFIER),
            ("client_id", &client),
            ("redirect_uri", redirect),
            ("resource", "http://localhost/mcp"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tokens}");
    assert!(tokens.get("app_secret").is_none());
    let access = tokens["access_token"].as_str().unwrap().to_string();
    let refresh = tokens["refresh_token"].as_str().unwrap().to_string();

    // At /mcp it's the agent; anywhere else it's refused.
    let me = tool(&app, &access, "whoami", json!({})).await;
    assert_eq!(me["structuredContent"]["agent"]["id"], json!(agent));
    assert_eq!(me["structuredContent"]["agent"]["kind"], "connected");
    assert_eq!(
        me["structuredContent"]["agent"]["acts_for"]["name"],
        "Owner"
    );
    let (status, refused) = send_as(&app, &access, "GET", "/admin/orders", None).await;
    assert_eq!(
        (status, code(&refused)),
        (StatusCode::UNAUTHORIZED, "agent_credentials_mcp_only")
    );

    // Refreshing rotates; the old refresh token used again revokes the agent's tokens.
    let (status, rotated) = token(
        &app,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("resource", "http://localhost/mcp"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    let access = rotated["access_token"].as_str().unwrap().to_string();
    assert!(!tool_names(&app, &access).await.is_empty());
    let (status, reused) = token(
        &app,
        &[("grant_type", "refresh_token"), ("refresh_token", &refresh)],
    )
    .await;
    assert_eq!(
        (status, reused["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    let answered = modern(&app, &access, "tools/list", json!({})).await;
    assert_eq!(answered.status, StatusCode::UNAUTHORIZED);

    // Connect again; revoking the agent stops its token, and /oauth/revoke works.
    let (_, location, _) = raw(&app, "GET", &authorize, None).await;
    let request = query_param(&location.unwrap(), "request").unwrap();
    let (_, answer) = send_as(
        &app,
        &session,
        "POST",
        &format!("/admin/agent-connections/{request}/approve"),
        Some(&approve),
    )
    .await;
    let agent = answer["agent"]["id"].as_str().unwrap().to_string();
    let code_ = query_param(answer["redirect_to"].as_str().unwrap(), "code").unwrap();
    let (_, tokens) = token(
        &app,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code_),
            ("code_verifier", VERIFIER),
            ("client_id", &client),
            ("redirect_uri", redirect),
        ],
    )
    .await;
    let access = tokens["access_token"].as_str().unwrap().to_string();
    assert_eq!(
        modern(&app, &access, "tools/list", json!({})).await.status,
        StatusCode::OK
    );
    let (status, _) = send(&app, "POST", &format!("/admin/agents/{agent}/revoke"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        modern(&app, &access, "tools/list", json!({})).await.status,
        StatusCode::UNAUTHORIZED
    );
    let (status, _) = token(
        &app,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", tokens["refresh_token"].as_str().unwrap()),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "revoked with its agent");

    // A denied connection goes back with access_denied.
    let (_, location, _) = raw(&app, "GET", &authorize, None).await;
    let request = query_param(&location.unwrap(), "request").unwrap();
    let (status, denied) = send_as(
        &app,
        &session,
        "POST",
        &format!("/admin/agent-connections/{request}/deny"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{denied}");
    let to = denied["redirect_to"].as_str().unwrap();
    assert_eq!(query_param(to, "error").as_deref(), Some("access_denied"));
    assert_eq!(query_param(to, "iss").as_deref(), Some("http://localhost"));

    // RFC 7009 revocation of a connected agent's token.
    let (_, location, _) = raw(&app, "GET", &authorize, None).await;
    let request = query_param(&location.unwrap(), "request").unwrap();
    let (_, answer) = send_as(
        &app,
        &session,
        "POST",
        &format!("/admin/agent-connections/{request}/approve"),
        Some(&approve),
    )
    .await;
    let code_ = query_param(answer["redirect_to"].as_str().unwrap(), "code").unwrap();
    let (_, tokens) = token(
        &app,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code_),
            ("code_verifier", VERIFIER),
            ("client_id", &client),
            ("redirect_uri", redirect),
        ],
    )
    .await;
    let access = tokens["access_token"].as_str().unwrap().to_string();
    let (status, _, _) = raw(
        &app,
        "POST",
        "/oauth/revoke",
        Some(encode(&[("token", &access)])),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        modern(&app, &access, "tools/list", json!({})).await.status,
        StatusCode::UNAUTHORIZED
    );

    let (status, metadata) = send_with(
        &app,
        &[],
        "GET",
        "/.well-known/oauth-authorization-server",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(metadata["client_id_metadata_document_supported"], true);
    assert_eq!(
        metadata["authorization_response_iss_parameter_supported"],
        true
    );
}
