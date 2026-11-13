//! Checkout rules: their routes, their messages on carts, and checkout refusing an error.

use serde_json::{Value, json};

use super::addresses::dutch_cart;
use super::*;

fn no_po_boxes(name: &str) -> Value {
    json!({
        "name": name,
        "conditions": [{"type": "address_is", "flag": "po_box"}],
        "outcome": {
            "severity": "error",
            "message": "We can't ship to PO boxes.",
            "code": "po_box_not_allowed"
        }
    })
}

async fn create_rule(app: &TestApp, body: &Value) -> String {
    let (status, created) = send(
        app,
        "POST",
        "/admin/checkout-rules",
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().unwrap().to_string()
}

async fn rule_order(app: &TestApp) -> Vec<String> {
    let (status, list) = send(app, "GET", "/admin/checkout-rules", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn rules_are_kept_in_order_moved_and_deleted() {
    let app = app().await;
    let first = create_rule(&app, &no_po_boxes("First")).await;
    assert!(first.starts_with("crule_"));
    let second = create_rule(&app, &no_po_boxes("Second")).await;
    assert_eq!(rule_order(&app).await, ["First", "Second"]);

    let (status, fetched) =
        send(&app, "GET", &format!("/admin/checkout-rules/{first}"), None).await;
    assert_eq!(status, StatusCode::OK, "{fetched}");
    assert_eq!(
        (
            &fetched["enabled"],
            &fetched["position"],
            &fetched["applies_to"]
        ),
        (
            &json!(true),
            &json!(0),
            &json!({"countries": {"mode": "all"}, "markets": [], "channels": [], "shipping_options": []})
        )
    );
    assert_eq!(fetched["outcome"]["path"], json!(null));

    let (status, moved) = send(
        &app,
        "PATCH",
        &format!("/admin/checkout-rules/{second}"),
        Some(r#"{"position": 0, "enabled": false}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(
        (&moved["position"], &moved["enabled"]),
        (&json!(0), &json!(false))
    );
    assert_eq!(rule_order(&app).await, ["Second", "First"]);

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/admin/checkout-rules/{second}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(
        &app,
        "GET",
        &format!("/admin/checkout-rules/{second}"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "checkout_rule_not_found")
    );
    assert_eq!(rule_order(&app).await, ["First"]);
}

#[tokio::test]
async fn every_broken_rule_comes_back_with_its_path() {
    let app = app().await;
    let body = json!({
        "name": "Broken",
        "conditions": [{"type": "field_matches", "field": "line1", "pattern": "(unclosed"}],
        "outcome": {"severity": "error", "message": " ", "path": "$.lines[0]"}
    })
    .to_string();
    let (status, json) = send(&app, "POST", "/admin/checkout-rules", Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_checkout_rule")
    );
    let found: Vec<(&str, &str)> = json["details"]["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["path"].as_str().unwrap(), p["code"].as_str().unwrap()))
        .collect();
    assert_eq!(
        found,
        [
            ("conditions[0].pattern", "invalid_pattern"),
            ("outcome.message", "empty"),
            ("outcome.path", "path_not_allowed"),
        ]
    );
}

#[tokio::test]
async fn reading_rules_doesnt_change_them() {
    let app = app().await;
    let reader = key(
        &app,
        json!({"kind": "secret", "name": "rules", "scopes": ["checkout_rules:read"]}),
    )
    .await;
    let (status, _) = send_as(&app, &reader, "GET", "/admin/checkout-rules", None).await;
    assert_eq!(status, StatusCode::OK);
    let body = no_po_boxes("Nope").to_string();
    let (status, json) = send_as(&app, &reader, "POST", "/admin/checkout-rules", Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
}

#[tokio::test]
async fn a_rules_error_blocks_checkout_until_the_address_changes() {
    let app = app().await;
    let rule = create_rule(&app, &no_po_boxes("No PO boxes")).await;
    let (pk, cart) = dutch_cart(&app).await;
    let po_box = json!({"address": {"last_name": "de Vries", "line1": "Postbus 263",
        "city": "Amsterdam", "postal_code": "1016 GV", "country": "NL"}})
    .to_string();
    let path = format!("{cart}/shipping-address");
    let (status, json) = send_as(&app, &pk, "PUT", &path, Some(&po_box)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let error = json!({
        "severity": "error",
        "code": "po_box_not_allowed",
        "path": "$.shipping_address.line1",
        "message": "We can't ship to PO boxes.",
        "source": {"type": "rule", "rule": rule},
        "params": {},
    });
    assert_eq!(json["messages"], json!([error]));

    let checkout = format!("{cart}/checkout");
    let (status, json) = send_as(&app, &pk, "POST", &checkout, Some("{}")).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "checkout_blocked"),
        "{json}"
    );
    assert_eq!(json["details"]["messages"], json!([error]));

    let street = json!({"address": {"last_name": "de Vries", "line1": "Prinsengracht 263",
        "city": "Amsterdam", "postal_code": "1016 GV", "country": "NL"}})
    .to_string();
    let (status, json) = send_as(&app, &pk, "PUT", &path, Some(&street)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["messages"], json!([]));
    let (status, json) = send_as(&app, &pk, "POST", &checkout, Some("{}")).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
}

#[tokio::test]
async fn a_rule_for_other_countries_says_nothing() {
    let app = app().await;
    let mut rule = no_po_boxes("Not here");
    rule["applies_to"] = json!({"countries": {"mode": "include", "codes": ["DE"]}});
    create_rule(&app, &rule).await;
    let (pk, cart) = dutch_cart(&app).await;
    let po_box = json!({"address": {"last_name": "de Vries", "line1": "Postbus 263",
        "city": "Amsterdam", "postal_code": "1016 GV", "country": "NL"}})
    .to_string();
    let (status, json) = send_as(
        &app,
        &pk,
        "PUT",
        &format!("{cart}/shipping-address"),
        Some(&po_box),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["messages"], json!([]));
}

#[tokio::test]
async fn trying_an_address_shows_what_the_cart_would() {
    let app = app().await;
    let saved = create_rule(&app, &no_po_boxes("No PO boxes")).await;
    let draft = json!({
        "name": "Phone for couriers",
        "enabled": false,
        "conditions": [{"type": "field_missing", "field": "phone"}],
        "outcome": {"severity": "warning", "message": "Add a phone number."}
    });
    let body = json!({
        "address": {"last_name": "de Vries", "line1": "Postbus", "city": "Amsterdam",
            "province": null, "postal_code": "1016gv", "country": "NL"},
        "rule": draft
    })
    .to_string();
    let (status, json) = send(&app, "POST", "/admin/checkout-rules/test", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["address"]["postal_code"], json!("1016GV"));
    assert_eq!(json["flags"]["po_box"], json!(true));
    let said: Vec<(&str, &str, bool)> = json["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["code"].as_str().unwrap(),
                m["path"].as_str().unwrap(),
                m["draft"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        said,
        // The format's house number warning steps aside for the rule's error on line 1,
        // as on a cart.
        [
            ("po_box_not_allowed", "$.shipping_address.line1", false),
            ("rule", "$.shipping_address.phone", true),
        ]
    );
    assert_eq!(json["messages"][0]["source"]["rule"], json!(saved));

    // A format problem stops it there, as a cart would refuse the address.
    let wrong = json!({"address": {"last_name": "de Vries", "line1": "Prinsengracht 263",
        "city": "Amsterdam", "postal_code": "1", "country": "NL"}})
    .to_string();
    let (status, json) = send(&app, "POST", "/admin/checkout-rules/test", Some(&wrong)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["problems"][0]["field"], json!("postal_code"));
    assert_eq!(json["messages"], json!([]));
}
