//! Price lists: names, kinds, windows, compare-at prices, and what shoppers see.

use serde_json::json;

use super::*;

fn eur(amount: i64) -> Value {
    json!({"amount": amount, "currency": "EUR"})
}

async fn create_list(app: &TestApp, body: Value) -> Value {
    let (status, list) = send(app, "POST", "/admin/price-lists", Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::CREATED, "{list}");
    list
}

/// The product behind `market_selling_a_mug`'s variant.
async fn mug_product(app: &TestApp) -> String {
    let (_, mugs) = send(app, "GET", "/admin/products?limit=1", None).await;
    mugs["items"][0]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_price_list_has_a_name_a_kind_a_status_and_a_window() {
    let app = app().await;
    let list = create_list(
        &app,
        json!({"name": "Black Friday", "description": "Everything off", "currency": "EUR",
            "kind": "sale", "status": "draft", "starts_at": "2026-11-27T00:00:00Z",
            "ends_at": "2026-11-30T00:00:00Z", "compare_at": "hidden",
            "adjustment": {"percent_bps": -2000}}),
    )
    .await;
    assert_eq!(
        (
            &list["name"],
            &list["kind"],
            &list["status"],
            &list["phase"],
            &list["starts_at"],
            &list["compare_at"],
            &list["adjustment"],
        ),
        (
            &json!("Black Friday"),
            &json!("sale"),
            &json!("draft"),
            &json!("draft"),
            &json!("2026-11-27T00:00:00Z"),
            &json!("hidden"),
            &json!({"percent_bps": -2000}),
        )
    );

    // The defaults: an active override list without a window or an adjustment.
    let plain = create_list(&app, json!({"name": "Retail", "currency": "EUR"})).await;
    assert_eq!(
        (&plain["kind"], &plain["phase"], &plain["adjustment"]),
        (&json!("override"), &json!("active"), &Value::Null)
    );

    let (status, error) = send(
        &app,
        "POST",
        "/admin/price-lists",
        Some(r#"{"name":"black FRIDAY","currency":"EUR"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&error), &error["details"]["price_list"]),
        (StatusCode::CONFLICT, "price_list_name_taken", &list["id"])
    );
    let body = r#"{"name":"Never","currency":"EUR","starts_at":"2026-01-02T00:00:00Z","ends_at":"2026-01-01T00:00:00Z"}"#;
    let (status, error) = send(&app, "POST", "/admin/price-lists", Some(body)).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "ends_not_after_start")
    );
    let (status, error) = send(
        &app,
        "POST",
        "/admin/price-lists",
        Some(r#"{"currency":"EUR"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body")
    );

    // Activated before its window opens, it's scheduled; renaming onto another list's name
    // is refused.
    let url = format!("/admin/price-lists/{}", list["id"].as_str().unwrap());
    let patch = r#"{"status":"active","starts_at":"2999-01-01T00:00:00Z","ends_at":null,"description":null}"#;
    let (status, list) = send(&app, "PATCH", &url, Some(patch)).await;
    assert_eq!(
        (
            status,
            &list["phase"],
            &list["ends_at"],
            &list["description"]
        ),
        (
            StatusCode::OK,
            &json!("scheduled"),
            &Value::Null,
            &Value::Null
        )
    );
    let (status, error) = send(&app, "PATCH", &url, Some(r#"{"name":"retail"}"#)).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::CONFLICT, "price_list_name_taken")
    );
}

#[tokio::test]
async fn fixed_prices_keep_compare_at_prices_and_report_every_problem() {
    let app = app().await;
    let (_, mug) = market_selling_a_mug(&app).await;
    let list = create_list(&app, json!({"name": "Sale", "currency": "EUR"})).await;
    let url = format!(
        "/admin/price-lists/{}/fixed-prices",
        list["id"].as_str().unwrap()
    );

    let body =
        json!({"fixed_prices": [{"variant": mug, "price": eur(800), "compare_at": eur(1200)}]});
    let (status, json) = send(&app, "PUT", &url, Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");
    let (_, prices) = send(&app, "GET", &url, None).await;
    assert_eq!(
        (
            &prices["items"][0]["price"],
            &prices["items"][0]["compare_at"]
        ),
        (&eur(800), &eur(1200))
    );

    let body = json!({"fixed_prices": [
        {"variant": mug, "price": eur(800)},
        {"variant": "var_00000000000000000000000000000001", "price": eur(800), "compare_at": eur(800)},
        {"variant": "var_00000000000000000000000000000002", "price": {"amount": 800, "currency": "USD"}},
    ]});
    let (status, error) = send(&app, "PUT", &url, Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "fixed_prices_invalid")
    );
    let entries: Vec<(&Value, &Value)> = error["details"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (&e["index"], &e["code"]))
        .collect();
    assert_eq!(
        entries,
        [
            (&json!(1), &json!("compare_at_not_above_price")),
            (&json!(2), &json!("fixed_price_currency_mismatch")),
        ]
    );
    let (_, prices) = send(&app, "GET", &url, None).await;
    assert_eq!(
        prices["items"][0]["compare_at"],
        eur(1200),
        "nothing was written"
    );

    let too_many: Vec<Value> = (1..=1001)
        .map(|n| json!({"variant": format!("var_{n:032x}"), "price": eur(100)}))
        .collect();
    let body = json!({ "fixed_prices": too_many });
    let (status, error) = send(&app, "PUT", &url, Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&error), &error["details"]["max"]),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "too_many_prices",
            &json!(1000)
        )
    );
}

#[tokio::test]
async fn a_variants_compare_at_price_stays_above_its_price() {
    let app = app().await;
    let (_, mug) = market_selling_a_mug(&app).await;
    let product = mug_product(&app).await;
    let url = format!("/admin/products/{product}/variants/{mug}");

    let body = json!({ "compare_at_price": eur(1500) }).to_string();
    let (status, json) = send(&app, "PATCH", &url, Some(&body)).await;
    assert_eq!(
        (status, &json["variants"][0]["compare_at_price"]),
        (StatusCode::OK, &eur(1500))
    );
    let body = json!({ "compare_at_price": eur(900) }).to_string();
    let (status, error) = send(&app, "PATCH", &url, Some(&body)).await;
    assert_eq!(
        (status, code(&error), &error["details"]["variant"]),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "compare_at_not_above_price",
            &json!(mug)
        )
    );
    // Raising the price past it is refused too; both together work.
    let body = json!({ "price": eur(1600) }).to_string();
    let (status, error) = send(&app, "PATCH", &url, Some(&body)).await;
    assert_eq!(
        (status, code(&error)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "compare_at_not_above_price"
        )
    );
    let body = json!({ "price": eur(1600), "compare_at_price": null }).to_string();
    let (status, json) = send(&app, "PATCH", &url, Some(&body)).await;
    assert_eq!(
        (status, &json["variants"][0]["compare_at_price"]),
        (StatusCode::OK, &Value::Null)
    );
}

#[tokio::test]
async fn shoppers_see_compare_at_prices_on_products_and_cart_lines() {
    let app = app().await;
    let (market, mug) = market_selling_a_mug(&app).await;
    let product = mug_product(&app).await;
    let url = format!("/admin/products/{product}/variants/{mug}");
    let body = json!({ "compare_at_price": eur(1500) }).to_string();
    send(&app, "PATCH", &url, Some(&body)).await;
    let pk = publishable(&app).await;
    let read = format!("/storefront/products/{product}?country=HR");

    // On the base price: the variant's own compare-at.
    let (_, page) = send_as(&app, &pk, "GET", &read, None).await;
    let variant = &page["variants"][0];
    assert_eq!(
        (
            &variant["price"],
            &variant["compare_at_price"],
            &variant["on_sale"]
        ),
        (&eur(1000), &eur(1500), &json!(true))
    );

    // A sale list is measured against the regular price.
    let sale = create_list(
        &app,
        json!({"name": "Sale", "currency": "EUR", "kind": "sale",
            "adjustment": {"percent_bps": -2000}}),
    )
    .await;
    let (_, market) = send(&app, "GET", &format!("/admin/markets/{market}"), None).await;
    let catalog = market["catalog"].as_str().unwrap();
    let body = json!({ "price_list": sale["id"] }).to_string();
    let path = format!("/admin/catalogs/{catalog}/price-list");
    let (status, json) = send(&app, "PUT", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (_, page) = send_as(&app, &pk, "GET", &read, None).await;
    let variant = &page["variants"][0];
    assert_eq!(
        (&variant["price"], &variant["compare_at_price"]),
        (&eur(800), &eur(1000))
    );

    let (_, cart) = send_as(
        &app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let lines = format!("/storefront/carts/{}/lines", cart["id"].as_str().unwrap());
    let line = json!({"variant": mug, "quantity": 2}).to_string();
    let (_, cart) = send_as(&app, &pk, "POST", &lines, Some(&line)).await;
    assert_eq!(
        (
            &cart["lines"][0]["unit_price"],
            &cart["lines"][0]["compare_at_price"],
            &cart["lines"][0]["total"]
        ),
        (&eur(800), &eur(1000), &eur(1600))
    );

    // A draft prices nothing: back to the base price.
    let url = format!("/admin/price-lists/{}", sale["id"].as_str().unwrap());
    send(&app, "PATCH", &url, Some(r#"{"status":"draft"}"#)).await;
    let (_, page) = send_as(&app, &pk, "GET", &read, None).await;
    assert_eq!(page["variants"][0]["price"], eur(1000));
}
