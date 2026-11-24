//! Carts, orders and customers in a language: what a cart keeps, what its responses are
//! answered in, and what an order copies in it beside the store's own text.

use axum::http::header::{AUTHORIZATION, CONTENT_LANGUAGE, CONTENT_TYPE};
use serde_json::{Value, json};

use super::*;

/// German published beside English.
async fn german_published(app: &TestApp) {
    let body = json!({"languages": [{"language": "de", "published": true}]}).to_string();
    let (status, store) = send(app, "PATCH", "/admin/store", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{store}");
}

/// German published beside English, offered by the first market after English.
async fn german_beside_english(app: &TestApp) {
    german_published(app).await;
    let (_, markets) = send(app, "GET", "/admin/markets", None).await;
    let market = markets["items"][0]["id"].as_str().unwrap();
    let body = json!({"languages": ["en", "de"]}).to_string();
    let path = format!("/admin/markets/{market}");
    let (status, json) = send(app, "PATCH", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

/// Translates `{kind}/{id}` into German: `[(field key, value)]`.
async fn in_german(app: &TestApp, kind: &str, id: &str, fields: &[(&str, &str)]) {
    let fields: serde_json::Map<String, Value> = fields
        .iter()
        .map(|(key, value)| (key.to_string(), json!({ "value": value })))
        .collect();
    let path = format!("/admin/translations/{kind}/{id}/de");
    let body = json!({ "fields": fields }).to_string();
    let (status, json) = send(app, "PUT", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{kind}: {json}");
}

/// A storefront request: the status, `Content-Language` and the body.
async fn storefront(
    app: &TestApp,
    key: &str,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Option<String>, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {key}"));
    if body.is_some() {
        request = request.header(CONTENT_TYPE, "application/json");
    }
    let request = request
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let response = app.router().oneshot(request).await.unwrap();
    let status = response.status();
    let language = response
        .headers()
        .get(CONTENT_LANGUAGE)
        .map(|l| l.to_str().unwrap().to_string());
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        language,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A warning rule every cart with an email meets, with a German message.
async fn rule_with_a_german_message(app: &TestApp) -> String {
    let body = json!({
        "name": "Example addresses",
        "conditions": [{"type": "field_contains", "field": "email", "values": ["example.hr"]}],
        "outcome": {
            "severity": "warning",
            "message": "Check your email address.",
            "code": "example_email"
        }
    });
    let (status, rule) = send(
        app,
        "POST",
        "/admin/checkout-rules",
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{rule}");
    let rule = rule["id"].as_str().unwrap().to_string();
    in_german(
        app,
        "checkout_rule",
        &rule,
        &[("message", "Prüfen Sie Ihre E-Mail-Adresse.")],
    )
    .await;
    rule
}

async fn mug_in_german(app: &TestApp) {
    let (_, mugs) = send(app, "GET", "/admin/products?limit=1", None).await;
    let mug = mugs["items"][0]["id"].as_str().unwrap().to_string();
    in_german(app, "product", &mug, &[("title", "Tasse")]).await;
}

#[tokio::test]
async fn a_cart_keeps_its_language_held_to_its_market_and_is_answered_in_it() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    german_beside_english(&app).await;
    mug_in_german(&app).await;
    rule_with_a_german_message(&app).await;

    // Nothing chosen: the market's default, English, and the store's own text.
    let (status, language, cart) = storefront(&app, &pk, "GET", &cart_path, None).await;
    assert_eq!(status, StatusCode::OK, "{cart}");
    assert_eq!(language.as_deref(), Some("en"));
    assert_eq!(cart["language"], Value::Null);
    assert_eq!(cart["lines"][0]["product_title"], "Mug");

    let path = format!("{cart_path}/language");
    let (status, language, cart) =
        storefront(&app, &pk, "PUT", &path, Some(r#"{"language":"de"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{cart}");
    assert_eq!(language.as_deref(), Some("de"));
    assert_eq!(cart["language"], "de");
    assert_eq!(cart["lines"][0]["product_title"], "Tasse");
    let message = &cart["messages"][0];
    assert_eq!(
        (&message["code"], &message["message"]),
        (
            &json!("example_email"),
            &json!("Prüfen Sie Ihre E-Mail-Adresse.")
        )
    );
    let (_, language, again) = storefront(&app, &pk, "GET", &cart_path, None).await;
    assert_eq!(language.as_deref(), Some("de"));
    assert_eq!(again["lines"][0]["product_title"], "Tasse");

    // A regional tag the market doesn't offer is kept as its language.
    let (_, _, cart) = storefront(&app, &pk, "PUT", &path, Some(r#"{"language":"de-AT"}"#)).await;
    assert_eq!(cart["language"], "de");
    // One it doesn't offer at all, or that isn't a tag, is kept as the market's default.
    for asked in ["fr", "not a tag"] {
        let body = json!({ "language": asked }).to_string();
        let (status, language, cart) = storefront(&app, &pk, "PUT", &path, Some(&body)).await;
        assert_eq!(status, StatusCode::OK, "{cart}");
        assert_eq!(
            (language.as_deref(), &cart["language"]),
            (Some("en"), &json!("en"))
        );
    }
    // `null` follows the market's default again.
    let (_, _, cart) = storefront(&app, &pk, "PUT", &path, Some(r#"{"language":null}"#)).await;
    assert_eq!(cart["language"], Value::Null);
    // The field is required, `null` included.
    let (status, _, json) = storefront(&app, &pk, "PUT", &path, Some("{}")).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body")
    );
}

#[tokio::test]
async fn a_cart_made_in_a_language_and_one_the_market_stopped_offering() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    german_beside_english(&app).await;
    mug_in_german(&app).await;
    let (status, language, cart) = storefront(
        &app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR","language":"de"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{cart}");
    assert_eq!(
        (language.as_deref(), &cart["language"]),
        (Some("de"), &json!("de"))
    );

    // The market drops German: the cart, which chose it, reads in English now.
    let path = format!("{cart_path}/language");
    storefront(&app, &pk, "PUT", &path, Some(r#"{"language":"de"}"#)).await;
    let (_, markets) = send(&app, "GET", "/admin/markets", None).await;
    let market = markets["items"][0]["id"].as_str().unwrap();
    let body = json!({"languages": ["en"]}).to_string();
    send(
        &app,
        "PATCH",
        &format!("/admin/markets/{market}"),
        Some(&body),
    )
    .await;
    let (status, language, cart) = storefront(&app, &pk, "GET", &cart_path, None).await;
    assert_eq!(status, StatusCode::OK, "{cart}");
    assert_eq!(language.as_deref(), Some("en"));
    assert_eq!(cart["lines"][0]["product_title"], "Mug");
}

#[tokio::test]
async fn an_order_is_read_in_its_language_by_the_buyer_and_in_the_stores_by_staff() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    german_beside_english(&app).await;
    mug_in_german(&app).await;
    let method = cash_on_delivery(&app).await;
    in_german(
        &app,
        "payment_method",
        &method,
        &[
            ("name", "Nachnahme"),
            ("instructions", "Zahlen Sie beim Boten."),
        ],
    )
    .await;
    let path = format!("{cart_path}/language");
    storefront(&app, &pk, "PUT", &path, Some(r#"{"language":"de"}"#)).await;
    let (_, _, methods) = storefront(
        &app,
        &pk,
        "GET",
        &format!("{cart_path}/payment-methods"),
        None,
    )
    .await;
    assert_eq!(methods["items"][0]["name"], "Nachnahme");
    let body = json!({ "payment_method": method }).to_string();
    storefront(
        &app,
        &pk,
        "PUT",
        &format!("{cart_path}/payment-method"),
        Some(&body),
    )
    .await;

    let (status, language, placed) = storefront(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/checkout"),
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{placed}");
    assert_eq!(language.as_deref(), Some("de"));
    assert_eq!(placed["language"], "de");
    assert_eq!(placed["lines"][0]["product_title"], "Tasse");
    assert_eq!(placed["payment"]["method_name"], "Nachnahme");
    assert_eq!(
        placed["next_action"]["instructions"],
        "Zahlen Sie beim Boten."
    );

    let order = placed["id"].as_str().unwrap();
    let token = placed["access_token"].as_str().unwrap();
    let (status, language, read) = storefront(
        &app,
        &pk,
        "GET",
        &format!("/storefront/orders/{order}?token={token}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(language.as_deref(), Some("de"));
    assert_eq!(read["lines"][0]["product_title"], "Tasse");
    assert_eq!(read["lines"][0].get("localized"), None);

    // Staff read the store's own text, with the buyer's beside it.
    let (status, admin) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    assert_eq!(status, StatusCode::OK, "{admin}");
    assert_eq!(admin["language"], "de");
    let line = &admin["lines"][0];
    assert_eq!(
        (&line["product_title"], &line["localized"]["product_title"]),
        (&json!("Mug"), &json!("Tasse"))
    );
    // Its payment too: the method's own words, and what the buyer was told.
    let payment = &admin["payments"][0];
    assert_eq!(
        (&payment["method_name"], &payment["instructions"]),
        (&json!("Cash on delivery"), &json!("Pay the courier."))
    );
    assert_eq!(
        payment["localized"],
        json!({"method_name": "Nachnahme", "instructions": "Zahlen Sie beim Boten."})
    );
    let path = format!("/admin/payments/{}", payment["id"].as_str().unwrap());
    let (status, alone) = send(&app, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{alone}");
    assert_eq!(alone["localized"], payment["localized"]);

    // The buyer reads German now.
    let customer = admin["customer"].as_str().unwrap();
    let (_, buyer) = send(&app, "GET", &format!("/admin/customers/{customer}"), None).await;
    assert_eq!(buyer["language"], "de");

    // The store stops publishing German: the checked-out cart is still its order's
    // German copies, and says so.
    let (_, markets) = send(&app, "GET", "/admin/markets", None).await;
    let market = markets["items"][0]["id"].as_str().unwrap();
    let body = json!({"languages": ["en"]}).to_string();
    let path = format!("/admin/markets/{market}");
    let (status, json) = send(&app, "PATCH", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let body = json!({"languages": [{"language": "de", "published": false}]}).to_string();
    let (status, store) = send(&app, "PATCH", "/admin/store", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{store}");
    let (status, language, cart) = storefront(&app, &pk, "GET", &cart_path, None).await;
    assert_eq!(status, StatusCode::OK, "{cart}");
    assert_eq!(language.as_deref(), Some("de"));
    assert_eq!(cart["lines"][0]["product_title"], "Tasse");
    // Its payment methods answer in the language the cart does.
    let path = format!("{cart_path}/payment-methods");
    let (status, language, json) = storefront(&app, &pk, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(language.as_deref(), Some("de"), "as the cart reads");
}

#[tokio::test]
async fn an_order_from_before_languages_reads_as_the_stores_default() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    pay_on_delivery(&app, &pk, &cart_path).await;
    let (status, placed) = send_as(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/checkout"),
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{placed}");
    assert_eq!(placed["language"], "en");
    let order = placed["id"].as_str().unwrap();
    // An order in the store's own language has no buyer's copy of its payment.
    let (status, admin) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    assert_eq!(status, StatusCode::OK, "{admin}");
    let payment = &admin["payments"][0];
    assert_eq!(
        (&payment["instructions"], &payment["localized"]),
        (&json!("Pay the courier."), &Value::Null)
    );
    // As a row written before the columns existed.
    sql(
        &app,
        "UPDATE orders SET language = NULL; UPDATE order_lines SET localized = NULL;
         UPDATE carts SET language = NULL; UPDATE customers SET language = NULL;
         UPDATE payments SET localized = NULL",
    )
    .await;
    let (status, admin) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    assert_eq!(status, StatusCode::OK, "{admin}");
    assert_eq!(
        (&admin["language"], &admin["lines"][0]["localized"]),
        (&Value::Null, &Value::Null)
    );
    let token = placed["access_token"].as_str().unwrap();
    let (status, language, read) = storefront(
        &app,
        &pk,
        "GET",
        &format!("/storefront/orders/{order}?token={token}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(language.as_deref(), Some("en"));
    assert_eq!(read["lines"][0]["product_title"], "Mug");
}

#[tokio::test]
async fn staff_set_a_customers_language_held_to_the_stores() {
    let app = app().await;
    german_published(&app).await;
    let (status, customer) = send(
        &app,
        "POST",
        "/admin/customers",
        Some(r#"{"email":"ivo@example.hr"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{customer}");
    assert_eq!(customer["language"], Value::Null);
    let path = format!("/admin/customers/{}", customer["id"].as_str().unwrap());
    let (status, updated) = send(&app, "PATCH", &path, Some(r#"{"language":"de-AT"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["language"], "de");
    let (_, updated) = send(&app, "PATCH", &path, Some(r#"{"language":null}"#)).await;
    assert_eq!(updated["language"], Value::Null);
}

#[tokio::test]
async fn a_refused_checkout_names_the_rules_in_the_carts_language() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    german_beside_english(&app).await;
    let body = json!({
        "name": "No example addresses",
        "conditions": [{"type": "field_contains", "field": "email", "values": ["example.hr"]}],
        "outcome": {"severity": "error", "message": "Use a real address.", "code": "example_email"}
    });
    let (status, rule) = send(
        &app,
        "POST",
        "/admin/checkout-rules",
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{rule}");
    let rule = rule["id"].as_str().unwrap();
    in_german(
        &app,
        "checkout_rule",
        rule,
        &[("message", "Nutzen Sie eine echte Adresse.")],
    )
    .await;
    pay_on_delivery(&app, &pk, &cart_path).await;
    let path = format!("{cart_path}/language");
    storefront(&app, &pk, "PUT", &path, Some(r#"{"language":"de"}"#)).await;
    let (status, _, refused) = storefront(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/checkout"),
        Some("{}"),
    )
    .await;
    assert_eq!(
        (status, code(&refused)),
        (StatusCode::CONFLICT, "checkout_blocked")
    );
    assert_eq!(
        refused["details"]["messages"][0]["message"],
        "Nutzen Sie eine echte Adresse."
    );
}
