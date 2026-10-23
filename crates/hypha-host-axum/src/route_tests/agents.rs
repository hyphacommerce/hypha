//! Agents: the staff routes that manage them and their proposals, and agents' calls of
//! operations, which come only in process (`hypha_http::agents::acting` and `call_as`, what `/mcp`
//! sends): their credentials, kill switches, cap, allowlist, modes, the call log,
//! proposals approved exactly as previewed, and redaction.

use hypha_core::AgentId;
use hypha_http::agents::{Answer, Mode, OperationCall};
use serde_json::{Map, json};

use super::*;

/// Creates an autonomous agent as the owner (a session: `apply` takes one) with `grant`;
/// its ID.
async fn agent_with(app: &TestApp, grant: Value) -> String {
    let session = owner_session(app).await;
    let body = json!({"name": "Restocker", "grant": grant}).to_string();
    let (status, json) = send_as(app, &session, "POST", "/admin/agents", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    json["id"].as_str().unwrap().to_string()
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        other => panic!("not an object: {other}"),
    }
}

/// `operation` with `params`, `query` and `body`, in `mode`.
fn op(operation: &str, params: Value, body: Option<Value>, mode: Mode) -> OperationCall {
    OperationCall {
        operation: hypha_core::OperationId::new(operation).unwrap(),
        params: object(params),
        query: Map::new(),
        body,
        mode,
        idempotency_key: None,
        preview_token: None,
    }
}

/// `agent` calls `call`, as `/mcp` would.
async fn call(app: &TestApp, agent: &str, call: OperationCall) -> Answer {
    let agent: AgentId = agent.parse().unwrap();
    use axum::response::IntoResponse;
    use hypha_http::agents::{NoteUse, acting, answer_of, call_as};
    match acting(&app.state, agent, NoteUse::Yes).await {
        Ok(acting) => call_as(&app.state, &acting, call).await,
        Err(e) => answer_of(e.into_response()).await,
    }
}

fn mug() -> Value {
    json!({"handle": "mug", "title": "Mug", "price": {"amount": 1000, "currency": "EUR"}})
}

fn create_mug(mode: Mode) -> OperationCall {
    op("admin.products.create", json!({}), Some(mug()), mode)
}

fn answer_code(answer: &Answer) -> (u16, &str) {
    (answer.status, answer.code().unwrap_or_default())
}

/// The agent's call log, newest first: `(operation, mode, status)`.
async fn calls_of(app: &TestApp, agent: &str) -> Vec<(String, String, u64)> {
    let (status, json) = send(app, "GET", &format!("/admin/agents/{agent}/calls"), None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    json["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            // An operation of this build has a summary; one that isn't, none.
            let known = c["operation"] != "admin.nothing.here";
            assert_eq!(c["summary"].is_string(), known, "{c}");
            (
                c["operation"].as_str().unwrap().to_string(),
                c["mode"].as_str().unwrap().to_string(),
                c["status"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn agents_are_created_listed_changed_and_revoked() {
    let app = app().await;
    let session = owner_session(&app).await;
    let body = json!({
        "name": "Restocker",
        "grant": {
            "read": ["orders:read"],
            "write": {"inventory:write": "apply", "products:write": "propose"},
            "operations": ["admin.orders.list", "admin.inventory.adjust_level"]
        }
    })
    .to_string();
    let (status, created) = send_as(&app, &session, "POST", "/admin/agents", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert!(created["key"].as_str().unwrap().starts_with("hypha_ak_"));
    assert_eq!(created["kind"], "autonomous");
    assert_eq!(created["writes_cap"], "apply");
    assert_eq!(
        created["grant"]["write"],
        json!({"inventory:write": "apply", "products:write": "propose"})
    );
    let id = created["id"].as_str().unwrap().to_string();

    let (status, listed) = send(&app, "GET", "/admin/agents", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["items"][0]["id"], json!(id));
    assert!(
        listed["items"][0].get("key").is_none(),
        "the key is shown once"
    );

    let path = format!("/admin/agents/{id}");
    let lift = r#"{"name": "Night shift", "operations": null, "enabled": false}"#;
    // Lifting the allowlist hands out the whole grant, which applies inventory writes: a
    // key can't, a staff session can.
    let (status, json) = send(&app, "PATCH", &path, Some(lift)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "not_allowed_to_grant_apply")
    );
    let (status, changed) = send_as(&app, &session, "PATCH", &path, Some(lift)).await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(
        (
            &changed["name"],
            &changed["enabled"],
            &changed["grant"]["operations"]
        ),
        (&json!("Night shift"), &json!(false), &Value::Null)
    );
    let (status, fetched) = send(&app, "GET", &path, None).await;
    assert_eq!(
        (status, &fetched["name"]),
        (StatusCode::OK, &json!("Night shift"))
    );

    let (status, revoked) = send(&app, "POST", &format!("{path}/revoke"), None).await;
    assert_eq!(status, StatusCode::OK, "{revoked}");
    assert!(revoked["revoked_at"].is_string());
    let (status, json) = send(&app, "POST", &format!("{path}/revoke"), None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "agent_revoked")
    );
    let (_, listed) = send(&app, "GET", "/admin/agents", None).await;
    assert_eq!(listed["items"], json!([]), "revoked agents are left out");
    let (_, listed) = send(&app, "GET", "/admin/agents?include_revoked=true", None).await;
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn nobody_grants_an_agent_what_they_may_not() {
    let app = app().await;
    let grant = |g: Value| json!({"name": "A", "grant": g}).to_string();
    // A key holds every scope but isn't a person: it can't let an agent write unchecked.
    let (status, json) = send(
        &app,
        "POST",
        "/admin/agents",
        Some(&grant(json!({"write": {"products:write": "apply"}}))),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "not_allowed_to_grant_apply")
    );
    assert_eq!(
        json["details"],
        json!({"field": "write", "scope": "products:write"})
    );
    let (status, json) = send(
        &app,
        "POST",
        "/admin/agents",
        Some(&grant(json!({"write": {"products:write": "propose"}}))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    let (status, json) = send(
        &app,
        "POST",
        "/admin/agents",
        Some(&grant(json!({"write": {"agents:write": "propose"}}))),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "agent_scope_not_grantable"
        )
    );
    assert_eq!(
        json["details"],
        json!({"field": "write", "scope": "agents:write"})
    );
    for (g, details) in [
        (
            json!({"read": ["orders:reed"]}),
            json!({"field": "read", "scope": "orders:reed"}),
        ),
        (
            json!({"write": {"orders:write": "always"}}),
            json!({"field": "write", "scope": "orders:write"}),
        ),
        (
            json!({"customer_data": ["orders:read"]}),
            json!({"field": "customer_data", "scope": "orders:read"}),
        ),
        (
            json!({"operations": ["Not an ID"]}),
            json!({"field": "operations", "operation": "Not an ID"}),
        ),
    ] {
        let (status, json) = send(&app, "POST", "/admin/agents", Some(&grant(g))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
        assert_eq!(json["details"], details, "{json}");
    }
    let reader = key(
        &app,
        json!({"kind": "secret", "name": "R", "scopes": ["agents:read"]}),
    )
    .await;
    let (status, json) = send_as(
        &app,
        &reader,
        "POST",
        "/admin/agents",
        Some(&grant(json!({}))),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
}

#[tokio::test]
async fn an_agents_key_is_refused_outside_mcp() {
    let app = app().await;
    let session = owner_session(&app).await;
    let body = json!({"name": "A", "grant": {"read": ["products:read"]}}).to_string();
    let (_, created) = send_as(&app, &session, "POST", "/admin/agents", Some(&body)).await;
    let key = created["key"].as_str().unwrap();
    for (method, path) in [
        ("GET", "/admin/products"),
        ("POST", "/admin/products"),
        ("GET", "/admin/me"),
        ("GET", "/storefront/store"),
    ] {
        let (status, json) = send_as(&app, key, method, path, Some("{}")).await;
        assert_eq!(
            (status, code(&json)),
            (StatusCode::UNAUTHORIZED, "agent_credentials_mcp_only"),
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn an_agent_reads_and_writes_as_its_grant_allows_and_each_call_is_logged() {
    let app = app().await;
    let agent = agent_with(
        &app,
        json!({"read": ["orders:read"], "write": {"products:write": "apply"}}),
    )
    .await;
    let created = call(&app, &agent, create_mug(Mode::Apply)).await;
    assert_eq!(created.status, 201, "{:?}", created.body);
    let product = created.body["id"].as_str().unwrap().to_string();
    let events = outbox_events(&app).await;
    let last = events.last().unwrap();
    assert_eq!(last.event_type, "product.created");
    assert_eq!(last.actor, hypha_core::Actor::Agent(agent.parse().unwrap()));

    let read = call(
        &app,
        &agent,
        op(
            "admin.products.fetch",
            json!({"id": product}),
            None,
            Mode::Apply,
        ),
    )
    .await;
    assert_eq!((read.status, &read.body["id"]), (200, &json!(product)));
    let refused = call(
        &app,
        &agent,
        op("admin.customers.list", json!({}), None, Mode::Apply),
    )
    .await;
    assert_eq!(answer_code(&refused), (403, "missing_scope"));
    let unknown = call(
        &app,
        &agent,
        op("admin.nothing.here", json!({}), None, Mode::Apply),
    )
    .await;
    assert_eq!(answer_code(&unknown), (404, "operation_not_found"));
    let bad = call(
        &app,
        &agent,
        op("admin.products.fetch", json!({}), None, Mode::Apply),
    )
    .await;
    assert_eq!(answer_code(&bad), (422, "invalid_operation_call"));

    assert_eq!(
        calls_of(&app, &agent).await,
        [
            ("admin.products.fetch".to_string(), "read".to_string(), 422),
            ("admin.nothing.here".to_string(), "apply".to_string(), 404),
            ("admin.customers.list".to_string(), "read".to_string(), 403),
            ("admin.products.fetch".to_string(), "read".to_string(), 200),
            (
                "admin.products.create".to_string(),
                "apply".to_string(),
                201
            ),
        ]
    );
    let (_, fetched) = send(&app, "GET", &format!("/admin/agents/{agent}"), None).await;
    assert!(fetched["last_used_at"].is_string(), "{fetched}");
}

#[tokio::test]
async fn a_disabled_or_revoked_agent_does_nothing() {
    let app = app().await;
    let agent = agent_with(&app, json!({"read": ["products:read"]})).await;
    let list = || op("admin.products.list", json!({}), None, Mode::Apply);
    assert_eq!(call(&app, &agent, list()).await.status, 200);
    let path = format!("/admin/agents/{agent}");
    send(&app, "PATCH", &path, Some(r#"{"enabled": false}"#)).await;
    assert_eq!(
        answer_code(&call(&app, &agent, list()).await),
        (403, "agent_disabled")
    );
    send(&app, "PATCH", &path, Some(r#"{"enabled": true}"#)).await;
    assert_eq!(call(&app, &agent, list()).await.status, 200);
    send(&app, "POST", &format!("{path}/revoke"), None).await;
    assert_eq!(
        answer_code(&call(&app, &agent, list()).await),
        (403, "agent_disabled")
    );
}

#[tokio::test]
async fn the_operator_turns_agents_off_or_caps_their_writes() {
    let off = app_configured("[agents]\nenabled = false").await;
    let agent = agent_with(&off, json!({"read": ["products:read"]})).await;
    let list = op("admin.products.list", json!({}), None, Mode::Apply);
    assert_eq!(
        answer_code(&call(&off, &agent, list).await),
        (403, "agents_disabled")
    );

    let read_only = app_configured("[agents]\nwrites = \"off\"").await;
    let agent = agent_with(&read_only, json!({"write": {"products:write": "apply"}})).await;
    let list = op("admin.products.list", json!({}), None, Mode::Apply);
    assert_eq!(call(&read_only, &agent, list).await.status, 200);
    for mode in [Mode::Apply, Mode::Preview, Mode::Propose] {
        let answer = call(&read_only, &agent, create_mug(mode)).await;
        assert_eq!(answer_code(&answer), (403, "agent_writes_off"), "{mode:?}");
    }

    let proposing = app_configured("[agents]\nwrites = \"propose\"").await;
    let agent = agent_with(&proposing, json!({"write": {"products:write": "apply"}})).await;
    let answer = call(&proposing, &agent, create_mug(Mode::Apply)).await;
    assert_eq!(
        answer.status, 202,
        "applied is proposed under the cap: {:?}",
        answer.body
    );
    let (_, fetched) = send(&proposing, "GET", &format!("/admin/agents/{agent}"), None).await;
    assert_eq!(fetched["writes_cap"], "propose");
    assert_eq!(
        fetched["grant"]["write"]["products:write"], "apply",
        "the grant is kept"
    );
}

#[tokio::test]
async fn an_allowlist_is_every_operation_an_agent_has() {
    let app = app().await;
    let agent = agent_with(
        &app,
        json!({"read": ["products:read", "orders:read"], "operations": ["admin.products.list"]}),
    )
    .await;
    let products = op("admin.products.list", json!({}), None, Mode::Apply);
    assert_eq!(call(&app, &agent, products).await.status, 200);
    let orders = op("admin.orders.list", json!({}), None, Mode::Apply);
    assert_eq!(
        answer_code(&call(&app, &agent, orders).await),
        (403, "operation_not_allowed")
    );
}

/// Bob, a staff member whose role ("Packer") has `scopes`: his session and his role's ID.
async fn staff_member(app: &TestApp, scopes: Value) -> (String, String, String) {
    let body = json!({"name": "Packer", "scopes": scopes}).to_string();
    let (_, role) = send(app, "POST", "/admin/roles", Some(&body)).await;
    let invite = json!({ "email": "bob@example.com", "role": role["id"] }).to_string();
    send(app, "POST", "/admin/invites", Some(&invite)).await;
    let accept = json!({
        "token": super::last_invite_token(app),
        "name": "Bob",
        "password": "bob password",
    })
    .to_string();
    let (status, accepted) =
        send_with(app, &[], "POST", "/admin/invites/accept", Some(&accept)).await;
    assert_eq!(status, StatusCode::CREATED, "{accepted}");
    let session = accepted["session"]["token"].as_str().unwrap().to_string();
    let (_, me) = send_as(app, &session, "GET", "/admin/me", None).await;
    let user = me["user"]["id"].as_str().unwrap().to_string();
    (session, role["id"].as_str().unwrap().to_string(), user)
}

#[tokio::test]
async fn a_connected_agent_is_capped_by_its_staff_members_role_now() {
    let app = app().await;
    let (_, role, user) = staff_member(&app, json!(["products:read", "orders:read"])).await;
    // Bob connects a client (the OAuth flow's step, done here directly).
    let ports = app.state.public_ports().await.unwrap();
    let bob = hypha_core::Ctx::new(
        hypha_core::Actor::User(user.parse().unwrap()),
        vec![
            hypha_core::Scope::ProductsRead,
            hypha_core::Scope::OrdersRead,
        ],
    );
    let input = hypha_agents::ConnectAgent {
        name: "Claude".to_string(),
        client: None,
        grant: hypha_agents::GrantInput {
            read: vec!["products:read".to_string(), "orders:read".to_string()],
            ..Default::default()
        },
    };
    let agent = hypha_agents::create_connected_agent(&bob, &ports, &app.state.ids, input)
        .await
        .unwrap()
        .id()
        .to_string();
    drop(ports);
    let orders = || op("admin.orders.list", json!({}), None, Mode::Apply);
    assert_eq!(call(&app, &agent, orders()).await.status, 200);

    // Bob's role loses orders: so does his agent, at its next call.
    let narrow = json!({"scopes": ["products:read"]}).to_string();
    let (status, json) = send(
        &app,
        "PATCH",
        &format!("/admin/roles/{role}"),
        Some(&narrow),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        answer_code(&call(&app, &agent, orders()).await),
        (403, "missing_scope")
    );
    let products = op("admin.products.list", json!({}), None, Mode::Apply);
    assert_eq!(call(&app, &agent, products).await.status, 200);

    // Bob is disabled: his agent stops.
    let (status, json) = send(
        &app,
        "PATCH",
        &format!("/admin/users/{user}"),
        Some(r#"{"status": "disabled"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let products = op("admin.products.list", json!({}), None, Mode::Apply);
    assert_eq!(
        answer_code(&call(&app, &agent, products).await),
        (403, "agent_disabled")
    );
}

#[tokio::test]
async fn an_agent_sees_personal_data_only_with_its_scopes() {
    let app = app().await;
    let (status, created) = send(
        &app,
        "POST",
        "/admin/customers",
        Some(r#"{"email": "ana@example.hr", "first_name": "Ana"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let customer = created["id"].as_str().unwrap();
    let fetch = || {
        op(
            "admin.customers.fetch",
            json!({"id": customer}),
            None,
            Mode::Apply,
        )
    };

    let plain = agent_with(&app, json!({"read": ["customers:read"]})).await;
    let seen = call(&app, &plain, fetch()).await;
    assert_eq!(seen.status, 200, "{:?}", seen.body);
    assert!(seen.body.get("email").is_none(), "{:?}", seen.body);
    assert!(seen.body.get("first_name").is_none());
    let redacted = seen.body["redacted"].as_array().unwrap();
    assert!(redacted.contains(&json!("email")) && redacted.contains(&json!("first_name")));

    let trusted = agent_with(
        &app,
        json!({"read": ["customers:read"], "customer_data": ["customer_data:email"]}),
    )
    .await;
    let seen = call(&app, &trusted, fetch()).await;
    assert_eq!(seen.body["email"], "ana@example.hr");
    assert!(seen.body.get("first_name").is_none());
}

/// A proposal holds what its write sends and answers: a caller other than a staff session
/// sees that only with every scope the write takes and every `customer_data:*` scope, the
/// agent that proposed it included.
#[tokio::test]
async fn a_proposals_write_is_shown_only_to_who_may_see_all_of_it() {
    let app = app().await;
    let (status, created) = send(
        &app,
        "POST",
        "/admin/customers",
        Some(r#"{"email": "pii@example.hr", "first_name": "Piiana"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let customer = created["id"].as_str().unwrap();
    let agent = agent_with(
        &app,
        json!({"read": ["customers:read"], "write": {"customers:write": "propose"}}),
    )
    .await;
    let proposed = call(
        &app,
        &agent,
        op(
            "admin.customers.update",
            json!({"id": customer}),
            Some(json!({"phone": "+385 PII 9"})),
            Mode::Apply,
        ),
    )
    .await;
    assert_eq!(proposed.status, 202, "{:?}", proposed.body);
    let hidden = json!(["request", "preview", "response"]);
    let proposal = &proposed.body["proposal"];
    assert!(!proposal.to_string().contains("PII"), "{proposal}");
    assert_eq!(proposal["redacted"], hidden);
    let id = proposal["id"].as_str().unwrap();

    let plain = key(
        &app,
        json!({"kind": "secret", "name": "inbox", "scopes": ["agents:read", "customers:write"]}),
    )
    .await;
    let trusted = key(
        &app,
        json!({"kind": "secret", "name": "trusted", "scopes": ["agents:read", "customers:write",
            "customer_data:name", "customer_data:email", "customer_data:phone",
            "customer_data:address"]}),
    )
    .await;
    for path in [
        "/admin/agent-proposals".to_string(),
        format!("/admin/agent-proposals/{id}"),
    ] {
        let (status, json) = send_as(&app, &plain, "GET", &path, None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert!(!json.to_string().contains("PII"), "{path}: {json}");
        let (status, json) = send_as(&app, &trusted, "GET", &path, None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert!(json.to_string().contains("PII"), "{path}: {json}");
    }
    let (_, one) = send_as(
        &app,
        &plain,
        "GET",
        &format!("/admin/agent-proposals/{id}"),
        None,
    )
    .await;
    assert_eq!(one["redacted"], hidden);
    let session = owner_session(&app).await;
    let path = format!("/admin/agent-proposals/{id}");
    let (_, one) = send_as(&app, &session, "GET", &path, None).await;
    assert_eq!(
        one["request"]["body"]["phone"], "+385 PII 9",
        "staff see it"
    );
    assert!(one.get("redacted").is_none());
}

#[tokio::test]
async fn an_agents_preview_answers_a_token_it_can_apply() {
    let app = app().await;
    let agent = agent_with(&app, json!({"write": {"products:write": "apply"}})).await;
    let previewed = call(&app, &agent, create_mug(Mode::Preview)).await;
    assert_eq!(previewed.status, 200, "{:?}", previewed.body);
    let preview = &previewed.body["preview"];
    let token = preview["token"].as_str().unwrap().to_string();
    assert_eq!(rows(&app, "products").await, 0);
    let mut apply = create_mug(Mode::Apply);
    apply.preview_token = Some(token.clone());
    let applied = call(&app, &agent, apply.clone()).await;
    assert_eq!(applied.status, 201, "{:?}", applied.body);
    assert_eq!(applied.body["id"], preview["response"]["body"]["id"]);
    assert_eq!(
        answer_code(&call(&app, &agent, apply).await),
        (409, "preview_used")
    );
    // Another caller can't use an agent's token.
    let (status, json) = super::send_with(
        &app,
        &[
            (AUTHORIZATION.as_str(), &format!("Bearer {}", app.admin_key)),
            ("hypha-preview-token", &token),
        ],
        "POST",
        "/admin/products",
        Some(&mug().to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "preview_token_invalid"),
        "the agent's, not the key's"
    );
    let modes: Vec<String> = calls_of(&app, &agent)
        .await
        .into_iter()
        .map(|c| c.1)
        .collect();
    assert_eq!(modes, ["apply", "apply", "preview"]);
}

async fn rows(app: &TestApp, table: &str) -> i64 {
    use hypha_store_postgres::PgConnection;
    let ports = app.state.public_ports().await.unwrap();
    let mut conn = ports.conn().await;
    conn.client()
        .query_one(&format!("SELECT count(*) FROM {table}"), &[])
        .await
        .unwrap()
        .get(0)
}

/// An agent that may only propose product writes.
async fn proposer(app: &TestApp) -> String {
    agent_with(
        app,
        json!({"read": ["products:read"], "write": {"products:write": "propose"}}),
    )
    .await
}

#[tokio::test]
async fn a_proposal_is_approved_and_applied_as_previewed() {
    let app = app().await;
    let agent = proposer(&app).await;
    let mut create = create_mug(Mode::Apply);
    create.idempotency_key = Some("mug-1".to_string());
    let proposed = call(&app, &agent, create.clone()).await;
    assert_eq!(proposed.status, 202, "{:?}", proposed.body);
    let proposal = &proposed.body["proposal"];
    assert_eq!(proposal["status"], "pending");
    assert_eq!(proposal["operation"], "admin.products.create");
    assert!(proposal["summary"].is_string(), "{proposal}");
    assert_eq!(proposal["scopes"], json!(["products:write"]));
    // The agent holds no `customer_data:*` scope: its proposal's write isn't shown to it.
    assert_eq!(
        proposal["redacted"],
        json!(["request", "preview", "response"])
    );
    assert_eq!(rows(&app, "products").await, 0, "nothing written yet");
    // The same key again: the same proposal.
    let again = call(&app, &agent, create).await;
    assert_eq!(again.body["proposal"]["id"], proposal["id"]);
    let id = proposal["id"].as_str().unwrap().to_string();

    let (status, inbox) = send(&app, "GET", "/admin/agent-proposals?status=pending", None).await;
    assert_eq!(status, StatusCode::OK, "{inbox}");
    assert_eq!(inbox["items"][0]["id"], json!(id));
    let (status, one) = send(&app, "GET", &format!("/admin/agent-proposals/{id}"), None).await;
    assert_eq!(
        (status, &one["preview"]["complete"]),
        (StatusCode::OK, &json!(true))
    );
    assert!(one["preview"].get("token").is_none());
    assert!(one["preview"].get("ids").is_none());
    let previewed = one["preview"]["response"]["body"]["id"].clone();

    let session = owner_session(&app).await;
    let approve = format!("/admin/agent-proposals/{id}/approve");
    let (status, decided) = send_as(&app, &session, "POST", &approve, None).await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    assert_eq!(decided["status"], "applied");
    assert_eq!(decided["response"]["status"], 201);
    assert_eq!(
        decided["response"]["body"]["id"], previewed,
        "the preview's product"
    );
    let (status, _) = send(
        &app,
        "GET",
        &format!("/admin/products/{}", previewed.as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let created = outbox_events(&app)
        .await
        .into_iter()
        .find(|e| e.event_type == "product.created")
        .unwrap();
    assert_eq!(
        created.actor,
        hypha_core::Actor::Agent(agent.parse().unwrap())
    );
    let (status, json) = send_as(&app, &session, "POST", &approve, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "proposal_decided")
    );
    let modes: Vec<String> = calls_of(&app, &agent)
        .await
        .into_iter()
        .map(|c| c.1)
        .collect();
    // The approval is a person's: the proposal records it, the agent's log doesn't.
    assert_eq!(modes, ["propose", "propose"]);
}

/// A proposal `agent` made directly in storage, as if previewed: creating a mug.
async fn stored_proposal(app: &TestApp, agent: &str) -> String {
    let ports = app.state.public_ports().await.unwrap();
    let ctx = hypha_core::Ctx::new(
        hypha_core::Actor::Agent(agent.parse().unwrap()),
        vec![hypha_core::Scope::ProductsWrite],
    );
    let input = hypha_agents::Propose {
        operation: "admin.products.create".to_string(),
        request: hypha_agents::ProposalRequest {
            body: Some(mug()),
            ..Default::default()
        },
        idempotency_key: None,
        preview: json!({"complete": false}),
        digest: "sha256:0".to_string(),
        scopes: vec![hypha_core::Scope::ProductsWrite],
    };
    let settings = &app.state.config.agents.settings;
    let made = hypha_agents::create_proposal(
        &ctx,
        &ports,
        &app.state.ids,
        &app.state.clock,
        settings,
        input,
    )
    .await
    .unwrap();
    made.value.id().to_string()
}

#[tokio::test]
async fn an_approval_holds_its_proposal_and_names_what_it_saw() {
    let app = app().await;
    let agent = proposer(&app).await;
    let id = stored_proposal(&app, &agent).await;
    let session = owner_session(&app).await;
    let approve = format!("/admin/agent-proposals/{id}/approve");

    // Shown before it was previewed again: nothing applied.
    let body = json!({"refreshes": 1}).to_string();
    let (status, json) = send_as(&app, &session, "POST", &approve, Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "proposal_changed"),
        "{json}"
    );
    assert_eq!(json["details"]["proposal"]["status"], "pending");
    assert_eq!(json["details"]["proposal"]["refreshes"], 0);
    assert_eq!(rows(&app, "products").await, 0);

    // Another approval is applying it: nobody else decides it meanwhile.
    let ports = app.state.public_ports().await.unwrap();
    let staff = hypha_core::Ctx::new(
        hypha_core::Actor::User(hypha_core::UserId::from_uuid(uuid::Uuid::from_u128(1))),
        hypha_core::Scope::ALL.to_vec(),
    );
    let claimed =
        hypha_agents::approve_proposal(&staff, &ports, &app.state.clock, id.parse().unwrap(), None)
            .await;
    assert_eq!(
        claimed.map(|p| p.status()),
        Ok(hypha_agents::ProposalStatus::Applying)
    );
    drop(ports);
    let body = json!({"refreshes": 0}).to_string();
    let (status, json) = send_as(&app, &session, "POST", &approve, Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "proposal_applying"),
        "{json}"
    );
    let reject = format!("/admin/agent-proposals/{id}/reject");
    let (status, json) = send_as(&app, &session, "POST", &reject, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "proposal_applying")
    );
    let (_, one) = send(&app, "GET", &format!("/admin/agent-proposals/{id}"), None).await;
    assert_eq!(one["status"], "applying");
}

#[tokio::test]
async fn approving_while_agents_only_read_keeps_the_proposal_pending() {
    let app = app_configured("[agents]\nwrites = \"off\"").await;
    let agent = proposer(&app).await;
    let id = stored_proposal(&app, &agent).await;
    let session = owner_session(&app).await;
    let approve = format!("/admin/agent-proposals/{id}/approve");
    let (status, json) = send_as(&app, &session, "POST", &approve, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "agent_writes_off"),
        "{json}"
    );
    let (_, one) = send(&app, "GET", &format!("/admin/agent-proposals/{id}"), None).await;
    assert_eq!(one["status"], "pending", "{one}");
    assert_eq!(rows(&app, "products").await, 0);
}

#[tokio::test]
async fn a_stale_proposal_is_previewed_again_for_a_second_look() {
    let app = app().await;
    let product = create_product(&app, "mug").await;
    let (_, json) = send(&app, "GET", &format!("/admin/products/{product}"), None).await;
    let variant = json["variants"][0]["id"].as_str().unwrap().to_string();
    let agent = proposer(&app).await;
    let price = json!({"price": {"amount": 2490, "currency": "EUR"}});
    let change = op(
        "admin.products.update_variant",
        json!({"id": product, "variant_id": variant}),
        Some(price.clone()),
        Mode::Propose,
    );
    let proposed = call(&app, &agent, change).await;
    assert_eq!(proposed.status, 202, "{:?}", proposed.body);
    let id = proposed.body["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    // Meanwhile staff set that price: the proposal would change nothing now.
    let path = format!("/admin/products/{product}/variants/{variant}");
    send(&app, "PATCH", &path, Some(&price.to_string())).await;
    let events = outbox_events(&app).await.len();

    let session = owner_session(&app).await;
    let approve = format!("/admin/agent-proposals/{id}/approve");
    let (status, json) = send_as(&app, &session, "POST", &approve, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "preview_stale"),
        "{json}"
    );
    assert_eq!(json["details"]["preview"]["events"], json!([]));
    assert!(
        json["details"]["preview"].get("token").is_none(),
        "the agent's"
    );
    let refreshed = &json["details"]["proposal"];
    assert_eq!(refreshed["status"], "pending", "a person looks again");
    assert_eq!(refreshed["refreshes"], 1);
    assert_eq!(refreshed["preview"]["events"], json!([]));
    assert!(refreshed["response"].is_null());
    let changes = outbox_events(&app).await;
    assert_eq!(
        changes[events..]
            .iter()
            .map(|e| e.event_type.as_str())
            .collect::<Vec<_>>(),
        ["proposal.updated"],
        "nothing written but the proposal"
    );
    // The new version is what's approved now.
    let (status, json) = send_as(&app, &session, "POST", &approve, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        (json["status"].as_str(), json["refreshes"].as_u64()),
        (Some("applied"), Some(1))
    );
}

#[tokio::test]
async fn a_stale_proposal_whose_write_now_fails_is_failed() {
    let app = app().await;
    let product = create_product(&app, "mug").await;
    let agent = proposer(&app).await;
    let change = op(
        "admin.products.update",
        json!({"id": product}),
        Some(json!({"title": "Big mug"})),
        Mode::Propose,
    );
    let proposed = call(&app, &agent, change).await;
    assert_eq!(proposed.status, 202, "{:?}", proposed.body);
    let id = proposed.body["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    // Meanwhile the product is gone: the write would fail, not just differ.
    send(&app, "DELETE", &format!("/admin/products/{product}"), None).await;
    let session = owner_session(&app).await;
    let approve = format!("/admin/agent-proposals/{id}/approve");
    let (status, json) = send_as(&app, &session, "POST", &approve, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "failed");
    assert_eq!(json["response"]["status"], 404);
}

#[tokio::test]
async fn only_staff_holding_its_scopes_decide_a_proposal() {
    let app = app().await;
    let agent = proposer(&app).await;
    let proposed = call(&app, &agent, create_mug(Mode::Propose)).await;
    let id = proposed.body["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let approve = format!("/admin/agent-proposals/{id}/approve");
    let (status, json) = send(&app, "POST", &approve, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "session_required")
    );
    let (bob, _, _) = staff_member(&app, json!(["orders:read", "agents:read"])).await;
    let (status, json) = send_as(&app, &bob, "POST", &approve, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
    let (_, mine) = send_as(
        &app,
        &bob,
        "GET",
        "/admin/agent-proposals?approvable=true",
        None,
    )
    .await;
    assert_eq!(mine["items"], json!([]), "none Bob could approve");

    let session = owner_session(&app).await;
    let reject = format!("/admin/agent-proposals/{id}/reject");
    let (status, json) = send_as(&app, &bob, "POST", &reject, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
    let (status, rejected) = send_as(
        &app,
        &session,
        "POST",
        &reject,
        Some(r#"{"note": "Not this week"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rejected}");
    assert_eq!(
        (&rejected["status"], &rejected["note"]),
        (&json!("rejected"), &json!("Not this week"))
    );
    assert_eq!(rows(&app, "products").await, 0);
}

#[tokio::test]
async fn approving_is_not_previewed() {
    let app = app().await;
    let agent = proposer(&app).await;
    let proposed = call(&app, &agent, create_mug(Mode::Propose)).await;
    let id = proposed.body["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let session = owner_session(&app).await;
    let auth = format!("Bearer {session}");
    let (status, json) = send_with(
        &app,
        &[(AUTHORIZATION.as_str(), &auth), ("hypha-preview", "1")],
        "POST",
        &format!("/admin/agent-proposals/{id}/approve"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["preview"]["complete"], false);
    assert_eq!(json["preview"]["blocked_by"], json!({"kind": "proposal"}));
    let (_, one) = send(&app, "GET", &format!("/admin/agent-proposals/{id}"), None).await;
    assert_eq!(one["status"], "pending");
    assert_eq!(rows(&app, "products").await, 0);
}

#[tokio::test]
async fn a_failing_preview_proposes_nothing() {
    let app = app().await;
    let agent = proposer(&app).await;
    let bad = op(
        "admin.products.create",
        json!({}),
        Some(json!({"handle": "Not a handle!", "title": "x"})),
        Mode::Propose,
    );
    let answer = call(&app, &agent, bad).await;
    assert_eq!(answer.status, 422, "{:?}", answer.body);
    assert_eq!(rows(&app, "agent_proposals").await, 0);
}

#[tokio::test]
async fn the_prune_job_expires_proposals_and_forgets_old_calls() {
    let app = app().await;
    let agent = proposer(&app).await;
    let proposed = call(&app, &agent, create_mug(Mode::Propose)).await;
    let id = proposed.body["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    super::sql(
        &app,
        "UPDATE agent_proposals SET expires_at = now() - interval '1 second'",
    )
    .await;
    super::sql(
        &app,
        "UPDATE agent_calls SET created_at = now() - interval '31 days'",
    )
    .await;
    super::sql(
        &app,
        "UPDATE preview_tokens SET expires_at = now() - interval '1 second'",
    )
    .await;
    hypha_http::jobs::prune(&app.state).await.unwrap();
    let (_, one) = send(&app, "GET", &format!("/admin/agent-proposals/{id}"), None).await;
    assert_eq!(one["status"], "expired");
    assert_eq!(calls_of(&app, &agent).await, []);
    assert_eq!(rows(&app, "preview_tokens").await, 0);
}

#[tokio::test]
async fn the_deployments_agent_settings_are_read_only_here() {
    let app = app_configured("[agents]\nwrites = \"propose\"\nproposal_lifetime = \"2d\"").await;
    let (status, json) = send(&app, "GET", "/admin/agent-settings", None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json,
        json!({
            "enabled": true,
            "writes": "propose",
            "proposal_lifetime_seconds": 172_800,
            "preview_token_lifetime_seconds": 900,
        })
    );
    let stranger = key(
        &app,
        json!({"kind": "secret", "name": "S", "scopes": ["orders:read"]}),
    )
    .await;
    let (status, json) = send_as(&app, &stranger, "GET", "/admin/agent-settings", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
}

/// A cart linked to a customer, by staff: its ID.
async fn cart_with_customer(app: &TestApp) -> String {
    let (_, cart_path, _) = cart_for_three_mugs(app).await;
    let cart = cart_path.rsplit('/').next().unwrap().to_string();
    let (status, customer) = send(
        app,
        "POST",
        "/admin/customers",
        Some(r#"{"email":"ana@example.hr"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{customer}");
    let body = json!({"customer": customer["id"]}).to_string();
    let (status, json) = send(
        app,
        "PUT",
        &format!("/admin/carts/{cart}/customer"),
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    cart
}

fn unlink_customer(cart: &str) -> OperationCall {
    op(
        "admin.carts.set_customer",
        json!({"id": cart}),
        Some(json!({"customer": null})),
        Mode::Apply,
    )
}

#[tokio::test]
async fn a_write_the_agent_only_proposes_never_commits_without_a_person() {
    let app = app().await;
    let cart = cart_with_customer(&app).await;
    // `admin.carts.set_customer` names customers:read too, which this agent lacks; the
    // command asks for it only when a customer is given.
    let agent = agent_with(&app, json!({"write": {"carts:write": "propose"}})).await;
    let answer = call(&app, &agent, unlink_customer(&cart)).await;
    assert_ne!(answer.status, 200, "applied: {:?}", answer.body);
    assert_eq!(
        rows(&app, "carts WHERE customer_id IS NOT NULL").await,
        1,
        "the cart keeps its customer"
    );
    assert_eq!(calls_of(&app, &agent).await[0].1, "propose");

    // The operator's cap, likewise: a grant to apply proposes under it.
    let capped = app_configured("[agents]\nwrites = \"propose\"").await;
    let cart = cart_with_customer(&capped).await;
    let agent = agent_with(&capped, json!({"write": {"carts:write": "apply"}})).await;
    let answer = call(&capped, &agent, unlink_customer(&cart)).await;
    assert_ne!(answer.status, 200, "applied: {:?}", answer.body);
    assert_eq!(
        rows(&capped, "carts WHERE customer_id IS NOT NULL").await,
        1,
        "the cart keeps its customer under the cap"
    );
}

#[tokio::test]
async fn approving_spends_none_of_the_agents_own_limits() {
    let app = app_configured("[rate_limits]\nagent_write = { burst = 1, per = \"1h\" }").await;
    let agent = proposer(&app).await;
    let proposed = call(&app, &agent, create_mug(Mode::Propose)).await;
    assert_eq!(proposed.status, 202, "{:?}", proposed.body);
    let id = proposed.body["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let session = owner_session(&app).await;
    let approve = format!("/admin/agent-proposals/{id}/approve");
    let (status, decided) = send_as(&app, &session, "POST", &approve, None).await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    assert_eq!(decided["status"], "applied", "{decided}");
    assert_eq!(
        calls_of(&app, &agent)
            .await
            .into_iter()
            .map(|c| c.1)
            .collect::<Vec<_>>(),
        ["propose"],
        "a person's approval isn't the agent's call"
    );
}

#[tokio::test]
async fn an_approval_whose_write_was_applied_already_is_recorded_applied() {
    let app = app().await;
    let (order, line, warehouse) = placed_order_for_three_mugs(&app).await;
    let agent = agent_with(&app, json!({"write": {"fulfillments:write": "apply"}})).await;
    let ship = |mode| {
        let mut call = op(
            "admin.fulfillments.create",
            json!({"id": order}),
            Some(json!({"location": warehouse, "lines": [{"line": line, "quantity": 1}]})),
            mode,
        );
        call.idempotency_key = Some("ship-1".to_string());
        call
    };
    let proposed = call(&app, &agent, ship(Mode::Propose)).await;
    assert_eq!(proposed.status, 202, "{:?}", proposed.body);
    let id = proposed.body["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    // The write went through under the proposal's key (an approval whose answer was lost
    // before it was recorded, or another person's at the same time).
    let applied = call(&app, &agent, ship(Mode::Apply)).await;
    assert_eq!(applied.status, 201, "{:?}", applied.body);

    let session = owner_session(&app).await;
    let approve = format!("/admin/agent-proposals/{id}/approve");
    let (status, decided) = send_as(&app, &session, "POST", &approve, None).await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    assert_eq!(decided["status"], "applied", "{decided}");
    assert_eq!(decided["response"]["status"], 201);
    assert_eq!(decided["response"]["body"]["id"], applied.body["id"]);
    assert_eq!(decided["refreshes"], 0);
    assert_eq!(rows(&app, "fulfillments").await, 1, "shipped once");
}

#[tokio::test]
async fn a_proposal_the_store_moved_past_is_stale_not_applied() {
    let app = app().await;
    let product = create_product(&app, "mug").await;
    let agent = proposer(&app).await;
    let restock = op(
        "admin.products.update",
        json!({"id": product}),
        Some(json!({"title": "Mug (restocked)"})),
        Mode::Apply,
    );
    let proposed = call(&app, &agent, restock).await;
    assert_eq!(proposed.status, 202, "{:?}", proposed.body);
    let id = proposed.body["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    // The owner changes the same product by hand meanwhile.
    let path = format!("/admin/products/{product}");
    let (status, _) = send(&app, "PATCH", &path, Some(r#"{"title": "Mug, blue"}"#)).await;
    assert_eq!(status, StatusCode::OK);

    let session = owner_session(&app).await;
    let approve = format!("/admin/agent-proposals/{id}/approve");
    let (status, json) = send_as(&app, &session, "POST", &approve, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "preview_stale"),
        "{json}"
    );
    assert_eq!(json["details"]["proposal"]["status"], "pending");
    assert_eq!(json["details"]["proposal"]["refreshes"], 1);
    let (_, now) = send(&app, "GET", &path, None).await;
    assert_eq!(now["title"], "Mug, blue", "the owner's change stands");

    // Looked at again, the new version is approved.
    let (status, json) = send_as(&app, &session, "POST", &approve, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "applied");
    let (_, now) = send(&app, "GET", &path, None).await;
    assert_eq!(now["title"], "Mug (restocked)");
}

#[tokio::test]
async fn an_agents_token_applies_nothing_after_a_hand_edit() {
    let app = app().await;
    let product = create_product(&app, "mug").await;
    let agent = agent_with(&app, json!({"write": {"products:write": "apply"}})).await;
    let restock = |mode| {
        op(
            "admin.products.update",
            json!({"id": product}),
            Some(json!({"title": "Mug (restocked)"})),
            mode,
        )
    };
    let previewed = call(&app, &agent, restock(Mode::Preview)).await;
    assert_eq!(previewed.status, 200, "{:?}", previewed.body);
    let token = previewed.body["preview"]["token"]
        .as_str()
        .unwrap()
        .to_string();
    let path = format!("/admin/products/{product}");
    let (status, _) = send(&app, "PATCH", &path, Some(r#"{"title": "Mug, blue"}"#)).await;
    assert_eq!(status, StatusCode::OK);

    let mut apply = restock(Mode::Apply);
    apply.preview_token = Some(token);
    let answer = call(&app, &agent, apply).await;
    assert_eq!(
        answer_code(&answer),
        (409, "preview_stale"),
        "{:?}",
        answer.body
    );
    let (_, now) = send(&app, "GET", &path, None).await;
    assert_eq!(now["title"], "Mug, blue");
}
