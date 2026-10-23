//! Previews of admin writes (`Hypha-Preview: 1`): the write runs for real and is rolled
//! back, its effects outside the database recorded or blocked. Each test checks what the
//! preview answers and that nothing of it was kept.

use serde_json::json;

use super::*;

/// Sends `method uri` with the admin key, as a preview.
async fn preview(
    app: &TestApp,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    preview_as(app, &app.admin_key.clone(), method, uri, body).await
}

/// `preview` as `token`.
async fn preview_as(
    app: &TestApp,
    token: &str,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let auth = format!("Bearer {token}");
    send_with(
        app,
        &[(AUTHORIZATION.as_str(), &auth), ("hypha-preview", "1")],
        method,
        uri,
        body,
    )
    .await
}

/// How many rows `table` holds.
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

/// What a write leaves behind, to compare before and after a preview.
async fn traces(app: &TestApp) -> (i64, i64, i64) {
    (
        rows(app, "outbox_events").await,
        rows(app, "idempotency_keys").await,
        rows(app, "notifications").await,
    )
}

/// The types of a preview's events.
fn types(preview: &Value) -> Vec<&str> {
    preview["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn a_previewed_product_is_answered_and_not_kept() {
    let app = app().await;
    let before = traces(&app).await;
    let body =
        json!({"handle": "mug", "title": "Mug", "price": {"amount": 1000, "currency": "EUR"}})
            .to_string();
    let request = Request::builder()
        .method("POST")
        .uri("/admin/products")
        .header(AUTHORIZATION, format!("Bearer {}", app.admin_key))
        .header(CONTENT_TYPE, "application/json")
        .header("hypha-preview", "1")
        .body(Body::from(body.clone()))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let ids = response
        .extensions()
        .get::<hypha_http::preview::PreviewIds>()
        .cloned()
        .expect("the IDs handed out");
    let json: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let preview = &json["preview"];
    assert_eq!(preview["complete"], true, "{json}");
    assert_eq!(preview["response"]["status"], 201);
    let product = &preview["response"]["body"];
    assert_eq!(product["handle"], "mug");
    assert_eq!(types(preview), ["product.created"]);
    assert_eq!(preview["events"][0]["subject"], product["id"]);
    assert_eq!(preview["events"][0]["version"], 1);
    assert_eq!(
        (&preview["emails"], &preview["calls"]),
        (&json!([]), &json!([]))
    );
    assert!(preview.get("blocked_by").is_none());
    let id = product["id"].as_str().unwrap();
    let uuid = id.trim_start_matches("prod_");
    assert!(
        ids.0.iter().any(|u| u.simple().to_string() == uuid),
        "the product's ID is among those handed out"
    );

    // Nothing kept: no product, no event.
    let (status, _) = send(&app, "GET", &format!("/admin/products/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(traces(&app).await, before);
    // The same write, for real.
    let (status, created) = send(&app, "POST", "/admin/products", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
}

#[tokio::test]
async fn a_previewed_variant_change_leaves_the_variant_as_it_was() {
    let app = app().await;
    let product = create_product(&app, "mug").await;
    let (_, json) = send(&app, "GET", &format!("/admin/products/{product}"), None).await;
    let variant = json["variants"][0]["id"].as_str().unwrap().to_string();
    let before = traces(&app).await;

    let path = format!("/admin/products/{product}/variants/{variant}");
    let change = r#"{"price":{"amount":2490,"currency":"EUR"},"sku":"MUG-1"}"#;
    let (status, json) = preview(&app, "PATCH", &path, Some(change)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let preview = &json["preview"];
    assert_eq!(preview["complete"], true);
    assert_eq!(preview["response"]["status"], 200);
    let variants = preview["response"]["body"]["variants"].as_array().unwrap();
    assert_eq!(variants[0]["price"]["amount"], 2490, "{json}");
    assert_eq!(types(preview), ["product.variant_updated"]);
    assert_eq!(preview["events"][0]["subject"], json!(product));

    let (_, json) = send(&app, "GET", &format!("/admin/products/{product}"), None).await;
    assert_eq!(json["variants"][0]["price"]["amount"], 1000);
    assert_eq!(json["variants"][0]["sku"], Value::Null);
    assert_eq!(traces(&app).await, before);
}

#[tokio::test]
async fn a_previewed_stock_adjustment_leaves_the_stock() {
    let app = app().await;
    let product = create_product(&app, "mug").await;
    let (_, json) = send(&app, "GET", &format!("/admin/products/{product}"), None).await;
    let variant = json["variants"][0]["id"].as_str().unwrap().to_string();
    let (_, location) = send(
        &app,
        "POST",
        "/admin/locations",
        Some(r#"{"name":"Warehouse"}"#),
    )
    .await;
    let location = location["id"].as_str().unwrap().to_string();
    let body = json!({ "variant": variant }).to_string();
    let (_, item) = send(&app, "POST", "/admin/inventory-items", Some(&body)).await;
    let item = item["id"].as_str().unwrap().to_string();
    let level = format!("/admin/inventory-items/{item}/levels/{location}");
    let (status, json) = send(&app, "PUT", &level, Some(r#"{"on_hand":10}"#)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let before = traces(&app).await;

    let adjust = format!("{level}/adjustments");
    let (status, json) = preview(&app, "POST", &adjust, Some(r#"{"delta":-3}"#)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let preview = &json["preview"];
    assert_eq!(preview["complete"], true);
    assert_eq!(
        preview["response"]["body"]["levels"][0]["on_hand"], 7,
        "{json}"
    );
    assert_eq!(types(preview), ["inventory_item.changed"]);

    let (_, json) = send(&app, "GET", &format!("/admin/inventory-items/{item}"), None).await;
    assert_eq!(json["levels"][0]["on_hand"], 10);
    assert_eq!(traces(&app).await, before);
}

#[tokio::test]
async fn a_previewed_fulfilment_with_a_key_is_not_kept_and_the_real_one_follows() {
    let app = app().await;
    let (order, line, warehouse) = placed_order_for_three_mugs(&app).await;
    let fulfillments = format!("/admin/orders/{order}/fulfillments");
    let ship =
        json!({"location": warehouse, "lines": [{"line": line, "quantity": 2}], "notify": true})
            .to_string();
    let before = traces(&app).await;
    let auth = format!("Bearer {}", app.admin_key);
    let keyed = |preview: bool| {
        let mut headers = vec![
            (AUTHORIZATION.as_str(), auth.as_str()),
            ("idempotency-key", "ship-1"),
        ];
        if preview {
            headers.push(("hypha-preview", "1"));
        }
        headers
    };

    let (status, json) = send_with(&app, &keyed(true), "POST", &fulfillments, Some(&ship)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let preview = &json["preview"];
    assert_eq!(preview["complete"], true);
    assert_eq!(preview["response"]["status"], 201);
    assert_eq!(preview["response"]["body"]["status"], "shipped");
    let shown = types(preview);
    assert!(shown.contains(&"fulfillment.created"), "{shown:?}");
    assert!(
        shown.contains(&"order.fulfillment_status_changed"),
        "{shown:?}"
    );
    assert_eq!(
        preview["emails"],
        json!([]),
        "the buyer's email goes out later, from the events"
    );

    let (_, json) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    assert_eq!(json["fulfillment_status"], "unfulfilled");
    assert_eq!(traces(&app).await, before, "no event, no idempotency key");

    // The real write with the same key isn't answered with the preview's.
    let (status, json) = send_with(&app, &keyed(false), "POST", &fulfillments, Some(&ship)).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    let (_, order) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    assert_eq!(order["fulfillment_status"], "partially_fulfilled");
    // And a preview with that key now shows what the key already did.
    let (status, again) = send_with(&app, &keyed(true), "POST", &fulfillments, Some(&ship)).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["preview"]["response"]["body"]["id"], json["id"]);
    assert_eq!(again["preview"]["events"], json!([]));
}

#[tokio::test]
async fn a_previewed_cancellation_leaves_the_order_open() {
    let app = app().await;
    let (order, _, _) = placed_order_for_three_mugs(&app).await;
    let before = traces(&app).await;
    let cancel = format!("/admin/orders/{order}/cancel");
    let (status, json) = preview(&app, "POST", &cancel, Some(r#"{"reason":"customer"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let preview = &json["preview"];
    assert_eq!(preview["complete"], true);
    assert_eq!(preview["response"]["status"], 200);
    assert!(
        preview["response"]["body"]["cancelled_at"].is_string(),
        "{json}"
    );
    assert!(types(preview).contains(&"order.cancelled"), "{json}");

    let (_, json) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    assert_eq!(json["cancelled_at"], Value::Null);
    assert_eq!(traces(&app).await, before);
}

#[tokio::test]
async fn a_previewed_invite_lists_its_email_and_sends_nothing() {
    let app = app().await;
    let role = r#"{"name":"support","scopes":["orders:read"]}"#;
    let (status, role) = send(&app, "POST", "/admin/roles", Some(role)).await;
    assert_eq!(status, StatusCode::CREATED, "{role}");
    let support = role["id"].as_str().unwrap().to_string();
    let before = traces(&app).await;
    let invite = json!({ "email": "bob@example.com", "role": support }).to_string();
    let (status, json) = preview(&app, "POST", "/admin/invites", Some(&invite)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let preview = &json["preview"];
    assert_eq!(preview["complete"], true);
    assert_eq!(preview["response"]["status"], 201);
    assert_eq!(preview["emails"].as_array().unwrap().len(), 1, "{json}");
    assert_eq!(preview["emails"][0]["template"], "staff_invite");
    assert_eq!(preview["emails"][0]["to"], "bob@example.com");
    assert!(preview["emails"][0]["subject"].is_string());

    assert!(app.emails.lock().unwrap().is_empty(), "nothing sent");
    let (_, invites) = send(&app, "GET", "/admin/invites", None).await;
    assert_eq!(invites["items"], json!([]));
    assert_eq!(traces(&app).await, before, "not even the email's record");

    // A key without `customer_data:email` sees that there's an email, not to whom.
    let key = key(
        &app,
        json!({"kind": "secret", "name": "Ops", "scopes": ["users:write", "orders:read"]}),
    )
    .await;
    let (status, json) = preview_as(&app, &key, "POST", "/admin/invites", Some(&invite)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let email = &json["preview"]["emails"][0];
    assert_eq!(email.get("to"), None, "{json}");
    assert_eq!(email["redacted"], json!(["to"]));
}

#[tokio::test]
async fn a_previewed_capture_stops_at_the_payment_app() {
    let app = app().await;
    let (base, _, saw) = fake_app(&[
        ("/start", 200, card_widget()),
        (
            "/capture",
            200,
            json!({"status": "accepted", "reference": "ch_1"}),
        ),
    ])
    .await;
    let (pk, cart_path) = cart_paying_by_card(&app, &base).await;
    let (status, started) = send_as(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/payment"),
        Some(r#"{"return_url":"https://shop.example/back"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let payment = started["payment"].as_str().unwrap().to_string();
    let (status, json) = report(&app, &payment, "evt_1", json!({"type": "authorized"})).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let calls = saw.lock().unwrap().len();
    let before = traces(&app).await;

    let capture = format!("/admin/payments/{payment}/capture");
    let (status, json) = preview(&app, "POST", &capture, Some("{}")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let preview = &json["preview"];
    assert_eq!(preview["complete"], false);
    assert_eq!(
        preview["blocked_by"],
        json!({"kind": "extension_call", "point": "payments.capture", "app": "app_card"})
    );
    assert_eq!(preview["response"], Value::Null);
    assert!(preview.get("token").is_none(), "nothing to apply");
    assert_eq!(saw.lock().unwrap().len(), calls, "the app wasn't called");

    let (_, json) = send(&app, "GET", &format!("/admin/payments/{payment}"), None).await;
    assert_eq!(json["status"], "authorized");
    assert_eq!(traces(&app).await, before, "not even the call's record");
}

#[tokio::test]
async fn a_failing_write_previews_as_its_failure() {
    let app = app().await;
    create_product(&app, "mug").await;
    let body =
        json!({"handle": "mug", "title": "Mug", "price": {"amount": 1000, "currency": "EUR"}})
            .to_string();
    let real = send(&app, "POST", "/admin/products", Some(&body)).await;
    let previewed = preview(&app, "POST", "/admin/products", Some(&body)).await;
    assert_eq!(previewed, real);
    assert_eq!(code(&previewed.1), "handle_taken");

    let bad = preview(&app, "POST", "/admin/products", Some("{")).await;
    assert_eq!(bad.0, StatusCode::BAD_REQUEST, "{}", bad.1);
}

#[tokio::test]
async fn reads_ignore_the_header_and_it_is_one_or_zero() {
    let app = app().await;
    create_product(&app, "mug").await;
    let (status, json) = preview(&app, "GET", "/admin/products", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["items"].as_array().unwrap().len(), 1, "{json}");

    let auth = format!("Bearer {}", app.admin_key);
    let body =
        json!({"handle": "cup", "title": "Cup", "price": {"amount": 1000, "currency": "EUR"}})
            .to_string();
    let (status, json) = send_with(
        &app,
        &[(AUTHORIZATION.as_str(), &auth), ("hypha-preview", "yes")],
        "POST",
        "/admin/products",
        Some(&body),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::BAD_REQUEST, "invalid_preview_header")
    );
    let (status, _) = send_with(
        &app,
        &[(AUTHORIZATION.as_str(), &auth), ("hypha-preview", "0")],
        "POST",
        "/admin/products",
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "0 runs the write");
}

/// Sends `method uri` as `token` with `Hypha-Preview-Token: preview_token`.
async fn apply_as(
    app: &TestApp,
    token: &str,
    preview_token: &str,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let auth = format!("Bearer {token}");
    send_with(
        app,
        &[
            (AUTHORIZATION.as_str(), &auth),
            ("hypha-preview-token", preview_token),
        ],
        method,
        uri,
        body,
    )
    .await
}

const MUG: &str =
    r#"{"handle": "mug", "title": "Mug", "price": {"amount": 1000, "currency": "EUR"}}"#;

#[tokio::test]
async fn a_token_applies_exactly_the_preview_once() {
    let app = app().await;
    let (status, json) = preview(&app, "POST", "/admin/products", Some(MUG)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let preview = &json["preview"];
    let token = preview["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("hypha_pv_"), "{token}");
    assert!(preview["expires_at"].is_string());
    let previewed = preview["response"]["body"]["id"].clone();
    let events = rows(&app, "outbox_events").await;

    // Re-serialized: the same request.
    let same =
        json!({"price": {"currency": "EUR", "amount": 1000}, "title": "Mug", "handle": "mug"})
            .to_string();
    let key = app.admin_key.clone();
    let (status, created) =
        apply_as(&app, &key, &token, "POST", "/admin/products", Some(&same)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["id"], previewed, "the product the preview showed");
    assert_eq!(rows(&app, "outbox_events").await, events + 1);

    let (status, json) = apply_as(&app, &key, &token, "POST", "/admin/products", Some(&same)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "preview_used")
    );
    assert_eq!(rows(&app, "products").await, 1);
}

#[tokio::test]
async fn a_token_applies_nothing_once_the_write_would_differ() {
    let app = app().await;
    let product = create_product(&app, "mug").await;
    let (_, json) = send(&app, "GET", &format!("/admin/products/{product}"), None).await;
    let variant = json["variants"][0]["id"].as_str().unwrap().to_string();
    let path = format!("/admin/products/{product}/variants/{variant}");
    let change = r#"{"price":{"amount":2490,"currency":"EUR"}}"#;
    let (status, json) = preview(&app, "PATCH", &path, Some(change)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let token = json["preview"]["token"].as_str().unwrap().to_string();

    // Meanwhile, someone else sets that price: the write would now change nothing.
    let (status, _) = send(&app, "PATCH", &path, Some(change)).await;
    assert_eq!(status, StatusCode::OK);
    let events = rows(&app, "outbox_events").await;

    let key = app.admin_key.clone();
    let (status, json) = apply_as(&app, &key, &token, "PATCH", &path, Some(change)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "preview_stale"),
        "{json}"
    );
    let fresh = &json["details"]["preview"];
    assert_eq!(fresh["complete"], true);
    assert_eq!(fresh["events"], json!([]), "nothing left to change");
    assert_eq!(fresh["response"]["status"], 200);
    let again = fresh["token"].as_str().unwrap().to_string();
    assert_ne!(again, token);
    assert_eq!(rows(&app, "outbox_events").await, events, "nothing written");

    // The fresh preview's token applies it as it is now.
    let (status, json) = apply_as(&app, &key, &again, "PATCH", &path, Some(change)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["variants"][0]["price"]["amount"], 2490);
    assert_eq!(rows(&app, "outbox_events").await, events);
}

#[tokio::test]
async fn a_token_is_its_callers_and_its_requests_only() {
    let app = app().await;
    let (_, json) = preview(&app, "POST", "/admin/products", Some(MUG)).await;
    let token = json["preview"]["token"].as_str().unwrap().to_string();
    let session = owner_session(&app).await;
    let (status, json) =
        apply_as(&app, &session, &token, "POST", "/admin/products", Some(MUG)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "preview_token_invalid"),
        "another caller"
    );
    let key = app.admin_key.clone();
    let cup = MUG.replace("mug", "cup");
    let (status, json) = apply_as(&app, &key, &token, "POST", "/admin/products", Some(&cup)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "preview_token_invalid"),
        "another body"
    );
    let unknown = format!("hypha_pv_{}", "0".repeat(64));
    for bad in [unknown.as_str(), "nonsense"] {
        let (status, json) = apply_as(&app, &key, bad, "POST", "/admin/products", Some(MUG)).await;
        assert_eq!(
            (status, code(&json)),
            (StatusCode::UNPROCESSABLE_ENTITY, "preview_token_invalid"),
            "{bad}"
        );
    }
    assert_eq!(rows(&app, "products").await, 0, "nothing applied");
    // Expired.
    sql(
        &app,
        "UPDATE preview_tokens SET expires_at = now() - interval '1 second'",
    )
    .await;
    let (status, json) = apply_as(&app, &key, &token, "POST", "/admin/products", Some(MUG)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "preview_token_invalid"),
        "expired"
    );
    // Both headers: one previews, the other applies.
    let auth = format!("Bearer {key}");
    let (status, json) = send_with(
        &app,
        &[
            (AUTHORIZATION.as_str(), &auth),
            ("hypha-preview", "1"),
            ("hypha-preview-token", &token),
        ],
        "POST",
        "/admin/products",
        Some(MUG),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::BAD_REQUEST, "invalid_preview_header")
    );
}

#[tokio::test]
async fn an_apply_sends_its_emails_only_once_it_commits() {
    let app = app().await;
    let role = r#"{"name":"support","scopes":["orders:read"]}"#;
    let (_, role) = send(&app, "POST", "/admin/roles", Some(role)).await;
    let support = role["id"].as_str().unwrap().to_string();
    let invite = |email: &str| json!({ "email": email, "role": support }).to_string();
    let sent = |app: &TestApp| app.emails.lock().unwrap().len();
    let key = app.admin_key.clone();

    // Applied as previewed: the invite's email goes out, after the commit.
    let bob = invite("bob@example.com");
    let (_, json) = preview(&app, "POST", "/admin/invites", Some(&bob)).await;
    let token = json["preview"]["token"].as_str().unwrap().to_string();
    let (status, json) = apply_as(&app, &key, &token, "POST", "/admin/invites", Some(&bob)).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(sent(&app), 1);

    // Stale: someone invited Ana meanwhile, so the write would renew that invite rather
    // than make the one previewed. Nothing is written, and nothing is sent.
    let ana = invite("ana@example.com");
    let (_, json) = preview(&app, "POST", "/admin/invites", Some(&ana)).await;
    let token = json["preview"]["token"].as_str().unwrap().to_string();
    let (status, _) = send(&app, "POST", "/admin/invites", Some(&ana)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(sent(&app), 2);
    let (status, json) = apply_as(&app, &key, &token, "POST", "/admin/invites", Some(&ana)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "preview_stale"),
        "{json}"
    );
    assert_eq!(sent(&app), 2, "the stale apply's email wasn't sent");
    let held = &json["details"]["preview"]["emails"];
    assert_eq!(held[0]["template"], "staff_invite", "{json}");
}
