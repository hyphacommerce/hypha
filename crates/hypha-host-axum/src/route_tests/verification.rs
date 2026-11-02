//! Address verification: the store's verification app, asked once per address, its answer
//! on the cart and the order, and staff reviewing, marking valid and changing an order's
//! address.

use serde_json::{Value, json};

use super::*;

fn zagreb(line1: &str) -> Value {
    json!({"last_name": "Horvat", "line1": line1, "city": "Zagreb",
        "postal_code": "10000", "country": "HR"})
}

/// A suggestion for any address: Ilica 1 with its full postal code.
fn corrected() -> Value {
    json!({"verdict": "deliverable_with_fixes", "precision": "premise",
        "fields": {"postal_code": {"status": "corrected"}},
        "suggested": {"line1": "Ilica 1", "city": "Zagreb", "postal_code": "10000",
                      "country": "HR"},
        "flags": {"residential": true}, "reference": "lookup-1"})
}

/// The verification app answering `answer`, named in the checkout settings.
async fn verifying(app: &TestApp, answer: Value) -> (AppAnswers, AppSaw) {
    let (base, answers, saw) = fake_app(&[("/verify", 200, answer)]).await;
    let patch = r#"{"verification_app":"app_verify"}"#;
    let (status, json) = send(app, "PATCH", "/admin/checkout-settings", Some(patch)).await;
    assert_eq!(
        (status, code(&json)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "verification_app_unavailable"
        ),
        "not an app answering address.verify yet"
    );
    app_answering(app, "app_verify", &base, &[("address.verify", "/verify")]).await;
    let (status, json) = send(app, "PATCH", "/admin/checkout-settings", Some(patch)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["verification_app"], "app_verify");
    (answers, saw)
}

async fn ship_to(app: &TestApp, pk: &str, cart_path: &str, address: Value) -> Value {
    let body = json!({ "address": address }).to_string();
    let path = format!("{cart_path}/shipping-address");
    let (status, cart) = send_as(app, pk, "PUT", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{cart}");
    cart
}

fn codes(cart: &Value) -> Vec<String> {
    cart["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["code"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn checkout_settings_start_empty() {
    let app = app().await;
    let (status, json) = send(&app, "GET", "/admin/checkout-settings", None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json,
        json!({"verification_app": null, "autocomplete_app": null, "validation_apps": []})
    );
}

#[tokio::test]
async fn an_address_is_verified_once_and_its_suggestion_costs_no_second_lookup() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    let (_, saw) = verifying(&app, corrected()).await;

    let cart = ship_to(&app, &pk, &cart_path, zagreb("Ilica 1a")).await;
    let check = &cart["shipping_address_check"];
    assert_eq!(check["verdict"], "deliverable_with_fixes");
    assert_eq!(check["suggested"]["line1"], "Ilica 1");
    assert_eq!(check["flags"], json!({"residential": true}));
    assert_eq!(codes(&cart), ["address_corrections_suggested"]);
    assert_eq!(cart["messages"][0]["severity"], "warning");
    assert_eq!(cart["messages"][0]["source"]["type"], "verification");
    {
        let saw = saw.lock().unwrap();
        assert_eq!(saw.len(), 1);
        assert_eq!(saw[0].1, "address.verify");
        assert_eq!(saw[0].3["context"], "cart");
        assert_eq!(saw[0].3["address"]["line1"], "Ilica 1a");
        assert_eq!(saw[0].3["address"].get("last_name"), None, "no names");
    }

    // The same address again, then the suggestion: nobody is asked.
    ship_to(&app, &pk, &cart_path, zagreb("Ilica 1a")).await;
    let cart = ship_to(&app, &pk, &cart_path, zagreb("Ilica 1")).await;
    assert_eq!(cart["shipping_address_check"]["verdict"], "deliverable");
    assert_eq!(cart["shipping_address_check"]["suggested"], Value::Null);
    assert_eq!(codes(&cart), Vec::<String>::new());
    assert_eq!(saw.lock().unwrap().len(), 1);
    let (status, calls) = send(&app, "GET", "/admin/apps/app_verify/extension-calls", None).await;
    assert_eq!(status, StatusCode::OK, "{calls}");
    assert_eq!(calls["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_failing_verification_app_leaves_the_address_unknown_and_quiet() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    let (answers, _) = verifying(&app, corrected()).await;
    answers
        .lock()
        .unwrap()
        .insert("/verify".to_string(), (500, json!({})));
    let cart = ship_to(&app, &pk, &cart_path, zagreb("Ilica 1a")).await;
    assert_eq!(cart["shipping_address_check"]["verdict"], "unknown");
    assert_eq!(codes(&cart), Vec::<String>::new());
}

#[tokio::test]
async fn a_rule_makes_an_undeliverable_address_block_and_the_order_needs_review() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    let (_, saw) = verifying(&app, json!({"verdict": "undeliverable"})).await;
    pay_on_delivery(&app, &pk, &cart_path).await;
    let cart = ship_to(&app, &pk, &cart_path, zagreb("Nowhere 1")).await;
    assert_eq!(codes(&cart), ["address_undeliverable"]);

    let rule = json!({"name": "Undeliverable",
        "conditions": [{"type": "verification_is", "verdicts": ["undeliverable"]}],
        "outcome": {"severity": "error", "message": "We can't deliver there."}});
    let (status, rule) = send(
        &app,
        "POST",
        "/admin/checkout-rules",
        Some(&rule.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{rule}");
    let checkout = format!("{cart_path}/checkout");
    let (status, json) = send_as(&app, &pk, "POST", &checkout, Some("{}")).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "checkout_blocked")
    );
    assert_eq!(json["details"]["messages"][0]["path"], "$.shipping_address");

    let path = format!("/admin/checkout-rules/{}", rule["id"].as_str().unwrap());
    send(&app, "DELETE", &path, None).await;
    let (status, placed) = send_as(&app, &pk, "POST", &checkout, Some("{}")).await;
    assert_eq!(status, StatusCode::CREATED, "{placed}");
    let order = placed["id"].as_str().unwrap();
    let order_path = format!("/admin/orders/{order}");
    let (_, json) = send(&app, "GET", &order_path, None).await;
    assert_eq!(json["address_review"], true);
    let check = &json["address_check"];
    assert_eq!(check["verification"]["verdict"], "undeliverable");
    assert_eq!(check["verification"]["app"], "app_verify");
    assert_eq!(check["warnings"][0]["code"], "address_undeliverable");
    assert_eq!(check["marked_valid"], Value::Null);
    let (_, listed) = send(&app, "GET", "/admin/orders?address=needs_review", None).await;
    assert_eq!(listed["items"][0]["id"], order);
    assert_eq!(listed["items"][0]["address_review"], true);

    let mark = format!("{order_path}/address-check/mark-valid");
    let (status, json) = send(&app, "POST", &mark, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["address_review"], false);
    assert!(
        json["address_check"]["marked_valid"]["by"]
            .as_str()
            .unwrap()
            .starts_with("key_"),
        "{json}"
    );
    let (_, listed) = send(&app, "GET", "/admin/orders?address=needs_review", None).await;
    assert_eq!(listed["items"], json!([]));

    // Changing the address: its format, its country, then verified again as an order's.
    let change = format!("{order_path}/shipping-address");
    let mut no_postal_code = zagreb("Ilica 2");
    no_postal_code["postal_code"] = Value::Null;
    let body = json!({ "address": no_postal_code }).to_string();
    let (status, json) = send(&app, "PUT", &change, Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_address")
    );
    assert_eq!(json["details"]["problems"][0]["field"], "postal_code");
    let slovenia = json!({"address": {"last_name": "Novak", "line1": "Slovenska 1",
        "city": "Ljubljana", "postal_code": "1000", "country": "SI"}})
    .to_string();
    let (status, json) = send(&app, "PUT", &change, Some(&slovenia)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "address_country_mismatch")
    );
    let body = json!({ "address": zagreb("Ilica 2") }).to_string();
    let (status, json) = send(&app, "PUT", &change, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["shipping_address"]["line1"], "Ilica 2");
    assert_eq!(json["address_check"]["marked_valid"], Value::Null);
    assert_eq!(json["address_review"], true, "undeliverable again");
    let saw = saw.lock().unwrap();
    assert_eq!(saw.last().unwrap().3["context"], "order");
}

#[tokio::test]
async fn a_rule_reads_a_verification_in_the_test_panel_only_with_checkout_rules_write() {
    let app = app().await;
    verifying(&app, corrected()).await;
    let body = json!({"address": zagreb("Ilica 1a"), "verify": true}).to_string();
    let reader = key(
        &app,
        json!({"kind": "secret", "name": "Reader", "scopes": ["checkout_rules:read"]}),
    )
    .await;
    let path = "/admin/checkout-rules/test";
    let (status, json) = send_as(&app, &reader, "POST", path, Some(&body)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    let (status, json) = send(&app, "POST", path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["verification"]["verdict"], "deliverable_with_fixes");
    assert_eq!(json["messages"][0]["code"], "address_corrections_suggested");
}
