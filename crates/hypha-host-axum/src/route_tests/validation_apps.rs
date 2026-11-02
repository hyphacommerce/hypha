//! Validation apps: every app answering `checkout.validate` asked as the shopper goes,
//! its answer kept with the cart, an error blocking checkout, a failure blocking only
//! when the merchant says so, and at most five such apps.

use serde_json::{Value, json};

use super::*;

fn zagreb(line1: &str, postal_code: &str) -> Value {
    json!({"last_name": "Horvat", "line1": line1, "city": "Zagreb",
        "postal_code": postal_code, "country": "HR"})
}

fn restricted() -> Value {
    json!({"messages": [{"severity": "error", "code": "alcohol_restricted",
        "path": "$.shipping_address.postal_code",
        "message": "We can't ship alcohol to this postal code."}]})
}

/// `app_rules`, made by hand, answering `checkout.validate` with `answer`.
async fn validating(app: &TestApp, answer: Value) -> (AppAnswers, AppSaw) {
    let (base, answers, saw) = fake_app(&[("/validate", 200, answer)]).await;
    app_answering(
        app,
        "app_rules",
        &base,
        &[("checkout.validate", "/validate")],
    )
    .await;
    (answers, saw)
}

async fn ship_to(app: &TestApp, pk: &str, cart_path: &str, address: Value) -> Value {
    let body = json!({ "address": address }).to_string();
    let path = format!("{cart_path}/shipping-address");
    let (status, cart) = send_as(app, pk, "PUT", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{cart}");
    cart
}

async fn check_out(app: &TestApp, pk: &str, cart_path: &str) -> (StatusCode, Value) {
    let path = format!("{cart_path}/checkout");
    send_as(app, pk, "POST", &path, Some("{}")).await
}

#[tokio::test]
async fn the_settings_list_each_app_answering_with_its_defaults() {
    let app = app().await;
    validating(&app, json!({"messages": []})).await;
    let (status, json) = send(&app, "GET", "/admin/checkout-settings", None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["validation_apps"],
        json!([{"app": "app_rules", "enabled": true, "block_on_failure": false}])
    );

    let patch = json!({"validation_apps": [
        {"app": "app_rules", "enabled": true, "block_on_failure": true}]});
    let (status, json) = send(
        &app,
        "PATCH",
        "/admin/checkout-settings",
        Some(&patch.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["validation_apps"][0]["block_on_failure"], true);
}

#[tokio::test]
async fn only_an_app_answering_the_point_is_set_once() {
    let app = app().await;
    validating(&app, json!({"messages": []})).await;
    let setting = |key: &str| json!({"app": key, "enabled": true, "block_on_failure": false});
    let patch = json!({"validation_apps": [setting("app_nobody")]}).to_string();
    let (status, json) = send(&app, "PATCH", "/admin/checkout-settings", Some(&patch)).await;
    assert_eq!(
        (status, code(&json)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_app_unavailable"
        )
    );
    assert_eq!(json["details"]["app"], "app_nobody");
    let patch =
        json!({"validation_apps": [setting("app_rules"), setting("app_rules")]}).to_string();
    let (status, json) = send(&app, "PATCH", "/admin/checkout-settings", Some(&patch)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "duplicate_validation_app")
    );
}

#[tokio::test]
async fn an_apps_messages_are_on_the_cart_and_its_answer_is_kept() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    let (_, saw) = validating(&app, restricted()).await;
    let cart = ship_to(&app, &pk, &cart_path, zagreb("Ilica 1", "10000")).await;
    let message = &cart["messages"][0];
    assert_eq!(message["code"], "alcohol_restricted", "{cart}");
    assert_eq!(message["severity"], "error");
    assert_eq!(message["path"], "$.shipping_address.postal_code");
    assert_eq!(
        message["source"],
        json!({"type": "app", "app": "app_rules"})
    );
    {
        let saw = saw.lock().unwrap();
        assert_eq!(saw.len(), 1);
        assert_eq!(saw[0].1, "checkout.validate");
        assert!(saw[0].2.starts_with("validate_address_"), "{}", saw[0].2);
        let sent = &saw[0].3;
        assert_eq!(sent["step"], "address");
        assert_eq!(sent["cart"]["lines"][0]["quantity"], 3);
        // An app made by hand has no grant: no personal data.
        let address = &sent["cart"]["shipping_address"];
        assert_eq!(address.get("line1"), None, "{address}");
        assert_eq!(address["country"], "HR");
    }
    // The same address again: the same question, kept.
    let cart = ship_to(&app, &pk, &cart_path, zagreb("Ilica 1", "10000")).await;
    assert_eq!(cart["messages"][0]["code"], "alcohol_restricted");
    let (_, fetched) = send_as(&app, &pk, "GET", &cart_path, None).await;
    assert_eq!(fetched["messages"][0]["code"], "alcohol_restricted");
    assert_eq!(saw.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_apps_error_blocks_checkout_until_the_cart_changes() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    let (answers, saw) = validating(&app, restricted()).await;
    pay_on_delivery(&app, &pk, &cart_path).await;
    ship_to(&app, &pk, &cart_path, zagreb("Ilica 1", "10000")).await;
    let (status, json) = check_out(&app, &pk, &cart_path).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "checkout_blocked")
    );
    assert_eq!(json["details"]["messages"][0]["code"], "alcohol_restricted");
    assert_eq!(saw.lock().unwrap().len(), 1, "the kept answer decided");

    answers
        .lock()
        .unwrap()
        .insert("/validate".to_string(), (200, json!({"messages": []})));
    // An app without the address's scope doesn't see the street or postal code: another
    // one is the same question.
    ship_to(&app, &pk, &cart_path, zagreb("Ilica 2", "10020")).await;
    assert_eq!(saw.lock().unwrap().len(), 1);
    // Changing a line asks nobody, but the answer is stale: checkout asks again.
    let (_, cart) = send_as(&app, &pk, "GET", &cart_path, None).await;
    let mug = cart["lines"][0]["variant"].as_str().unwrap().to_string();
    let line = format!("{cart_path}/lines/{mug}");
    let (status, cart) = send_as(&app, &pk, "PUT", &line, Some(r#"{"quantity":2}"#)).await;
    assert_eq!(status, StatusCode::OK, "{cart}");
    assert_eq!(saw.lock().unwrap().len(), 1);
    assert_eq!(
        cart["messages"][0]["code"], "alcohol_restricted",
        "stale, still shown"
    );
    let (status, json) = check_out(&app, &pk, &cart_path).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    let saw = saw.lock().unwrap();
    assert_eq!(saw.len(), 2);
    assert_eq!(saw[1].3["step"], "checkout");
}

#[tokio::test]
async fn a_failing_app_is_skipped_unless_its_failure_blocks() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    let (answers, _) = validating(&app, json!({})).await;
    answers
        .lock()
        .unwrap()
        .insert("/validate".to_string(), (500, json!({})));
    pay_on_delivery(&app, &pk, &cart_path).await;
    let cart = ship_to(&app, &pk, &cart_path, zagreb("Ilica 1", "10000")).await;
    assert_eq!(cart["messages"], json!([]));

    let patch = json!({"validation_apps": [
        {"app": "app_rules", "enabled": true, "block_on_failure": true}]})
    .to_string();
    let (status, json) = send(&app, "PATCH", "/admin/checkout-settings", Some(&patch)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (status, json) = check_out(&app, &pk, &cart_path).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "checkout_blocked")
    );
    assert_eq!(
        json["details"]["messages"][0],
        json!({"severity": "error", "code": "validation_unavailable", "path": "$",
            "message": "We can't check your order right now. Try again in a moment.",
            "source": {"type": "app", "app": "app_rules"}})
    );
}

#[tokio::test]
async fn a_disabled_app_isnt_asked() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    let (_, saw) = validating(&app, restricted()).await;
    let patch = json!({"validation_apps": [
        {"app": "app_rules", "enabled": false, "block_on_failure": true}]})
    .to_string();
    let (status, json) = send(&app, "PATCH", "/admin/checkout-settings", Some(&patch)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let cart = ship_to(&app, &pk, &cart_path, zagreb("Ilica 1", "10000")).await;
    assert_eq!(cart["messages"], json!([]));
    assert!(saw.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_sixth_validation_app_is_refused() {
    let app = app().await;
    let (base, _, _) = fake_app(&[("/validate", 200, json!({"messages": []}))]).await;
    for n in 1..=5 {
        app_answering(
            &app,
            &format!("app_rules_{n}"),
            &base,
            &[("checkout.validate", "/validate")],
        )
        .await;
    }
    create_app(&app, json!({"key": "app_rules_6", "name": "Six"})).await;
    let body = json!({"extension_points": [{"point": "checkout.validate", "version": 1,
        "url": format!("{base}/validate")}]})
    .to_string();
    let path = "/admin/apps/app_rules_6/extension-points";
    let (status, json) = send(&app, "PUT", path, Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "too_many_validation_apps")
    );
    assert_eq!(json["details"]["max"], 5);
    // One already answering keeps its place.
    let path = "/admin/apps/app_rules_1/extension-points";
    let (status, json) = send(&app, "PUT", path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

/// Five hand-made apps answering `checkout.validate` at `base`.
async fn five_validation_apps(app: &TestApp, base: &str) {
    for n in 1..=5 {
        app_answering(
            app,
            &format!("app_rules_{n}"),
            base,
            &[("checkout.validate", "/validate")],
        )
        .await;
    }
}

/// A local app whose manifest registers `checkout.validate` and asks for `scopes`; its
/// base URL.
async fn validating_manifest(scopes: &[&str]) -> String {
    use axum::{Router, routing::get};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let manifest = json!({
        "key": "app_sixth",
        "name": "Sixth",
        "version": "1.0.0",
        "publisher": "Rules Ltd",
        "app_url": base,
        "redirect_uris": ["/callback"],
        "scopes": scopes,
        "privacy_url": "/privacy",
        "extension_points": [{"point": "checkout.validate", "version": 1, "url": "/validate"}]
    })
    .to_string();
    let router = Router::new().route(
        "/hypha-app.json",
        get(move || {
            let manifest = manifest.clone();
            async move { ([(CONTENT_TYPE, "application/json")], manifest) }
        }),
    );
    tokio::spawn(async move { axum::serve(listener, router).await });
    base
}

/// Asks to install the app at `base` with `scope`: the consent request's ID.
async fn consent_request(app: &TestApp, base: &str, scope: &str) -> String {
    use super::oauth::{encode, query_param, raw};
    let client = format!("{base}/hypha-app.json");
    let redirect = format!("{base}/callback");
    let query = encode(&[
        ("response_type", "code"),
        ("client_id", &client),
        ("redirect_uri", &redirect),
        ("state", "st4te"),
        (
            "code_challenge",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        ),
        ("code_challenge_method", "S256"),
        ("scope", scope),
    ]);
    let (status, location, body) =
        raw(app, "GET", &format!("/oauth/authorize?{query}"), None).await;
    assert_eq!(status, StatusCode::FOUND, "{body}");
    query_param(&location.unwrap(), "request").unwrap()
}

#[tokio::test]
async fn approving_a_sixth_validation_app_is_refused() {
    let app = super::oauth::oauth_app().await;
    let (base, _, _) = fake_app(&[("/validate", 200, json!({"messages": []}))]).await;
    five_validation_apps(&app, &base).await;
    let sixth = validating_manifest(&["carts:read"]).await;
    let request = consent_request(&app, &sixth, "carts:read").await;
    let owner = owner_session(&app).await;
    let path = format!("/admin/oauth/requests/{request}/approve");
    let (status, json) = send_as(&app, &owner, "POST", &path, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "too_many_validation_apps")
    );
}

#[tokio::test]
async fn a_sixth_validation_app_asking_for_carts_write_is_refused_too() {
    let app = super::oauth::oauth_app().await;
    let (base, _, _) = fake_app(&[("/validate", 200, json!({"messages": []}))]).await;
    five_validation_apps(&app, &base).await;
    let sixth = validating_manifest(&["carts:write"]).await;
    let request = consent_request(&app, &sixth, "carts:write").await;
    let owner = owner_session(&app).await;
    let path = format!("/admin/oauth/requests/{request}/approve");
    let (status, json) = send_as(&app, &owner, "POST", &path, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "too_many_validation_apps")
    );
}

#[tokio::test]
async fn enabling_a_sixth_validation_app_is_refused() {
    let app = app().await;
    let (base, _, _) = fake_app(&[("/validate", 200, json!({"messages": []}))]).await;
    five_validation_apps(&app, &base).await;
    create_app(&app, json!({"key": "app_rules_6", "name": "Six"})).await;
    let disable = json!({"enabled": false}).to_string();
    let (status, json) = send(&app, "PATCH", "/admin/apps/app_rules_6", Some(&disable)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let body = json!({"extension_points": [{"point": "checkout.validate", "version": 1,
        "url": format!("{base}/validate")}]})
    .to_string();
    let path = "/admin/apps/app_rules_6/extension-points";
    let (status, json) = send(&app, "PUT", path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let enable = json!({"enabled": true}).to_string();
    let (status, json) = send(&app, "PATCH", "/admin/apps/app_rules_6", Some(&enable)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "too_many_validation_apps")
    );
}
