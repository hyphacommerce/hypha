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
            &json!({"percent_bps": -2000, "applies_to": {"type": "all"}}),
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
    let body = json!({ "price_lists": [sale["id"]] }).to_string();
    let path = format!("/admin/catalogs/{catalog}/price-lists");
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

/// A market, its catalog and the mug, with lists made from `bodies` on the catalog.
async fn lists_on_the_catalog(app: &TestApp, bodies: Vec<Value>) -> (String, String, Vec<String>) {
    let (market, mug) = market_selling_a_mug(app).await;
    let (_, read) = send(app, "GET", &format!("/admin/markets/{market}"), None).await;
    let catalog = read["catalog"].as_str().unwrap().to_string();
    let mut ids = Vec::new();
    for body in bodies {
        ids.push(
            create_list(app, body).await["id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    let body = json!({ "price_lists": ids }).to_string();
    let (status, json) = send(
        app,
        "PUT",
        &format!("/admin/catalogs/{catalog}/price-lists"),
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    (market, mug, ids)
}

#[tokio::test]
async fn a_catalogs_lists_compete_and_the_lowest_price_wins() {
    let app = app().await;
    let (market, mug, lists) = lists_on_the_catalog(
        &app,
        vec![
            json!({"name": "Retail", "currency": "EUR", "adjustment": {"percent_bps": -1000}}),
            json!({"name": "Sale", "currency": "EUR", "kind": "sale",
                "adjustment": {"percent_bps": -3000}}),
        ],
    )
    .await;
    let product = mug_product(&app).await;
    let pk = publishable(&app).await;
    let (_, page) = send_as(
        &app,
        &pk,
        "GET",
        &format!("/storefront/products/{product}?country=HR"),
        None,
    )
    .await;
    // The sale wins at 700, measured against the retail list's 900.
    assert_eq!(
        (
            &page["variants"][0]["price"],
            &page["variants"][0]["compare_at_price"]
        ),
        (&eur(700), &eur(900))
    );

    let (status, prices) = send(
        &app,
        "GET",
        &format!("/admin/products/{product}/prices?market={market}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{prices}");
    let variant = &prices["markets"][0]["variants"][0];
    assert_eq!(
        (
            &variant["variant"],
            &variant["price"],
            &variant["price_list"]
        ),
        (&json!(mug), &eur(700), &json!(lists[1]))
    );
    let results: Vec<(&Value, &Value, &Value)> = variant["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (&c["name"], &c["price"], &c["result"]))
        .collect();
    assert_eq!(
        results,
        [
            (&json!("Retail"), &eur(900), &json!("higher")),
            (&json!("Sale"), &eur(700), &json!("won")),
        ]
    );
    let (status, error) = send(
        &app,
        "GET",
        &format!("/admin/products/{product}/prices?quantity=0"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_quantity")
    );

    // The market and the catalog name their lists; one taken off stops competing.
    let (_, read) = send(&app, "GET", &format!("/admin/markets/{market}"), None).await;
    assert_eq!(read["price_lists"], json!(lists));
    let catalog = read["catalog"].as_str().unwrap();
    let path = format!("/admin/catalogs/{catalog}/price-lists/{}", lists[1]);
    let (status, json) = send(&app, "DELETE", &path, None).await;
    assert_eq!(
        (status, &json["price_lists"]),
        (StatusCode::OK, &json!([lists[0]]))
    );
    let (status, error) = send(&app, "DELETE", &path, None).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::NOT_FOUND, "price_list_not_in_catalog")
    );
    let add = format!("/admin/catalogs/{catalog}/price-lists");
    let body = json!({ "price_list": lists[1] }).to_string();
    let (status, json) = send(&app, "POST", &add, Some(&body)).await;
    assert_eq!(
        (status, json["price_lists"].as_array().unwrap().len()),
        (StatusCode::OK, 2)
    );
    let rsd = create_list(&app, json!({"name": "Dinars", "currency": "RSD"})).await;
    let body = json!({ "price_list": rsd["id"] }).to_string();
    let (status, error) = send(&app, "POST", &add, Some(&body)).await;
    assert_eq!(
        (status, code(&error)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "price_list_currency_mismatch"
        )
    );
}

#[tokio::test]
async fn an_adjustment_reaches_only_what_it_applies_to() {
    let app = app().await;
    let (_, category) = send(
        &app,
        "POST",
        "/admin/categories",
        Some(r#"{"name":"Kitchen","handle":"kitchen","visible":true}"#),
    )
    .await;
    let (_, below) = send(
        &app,
        "POST",
        "/admin/categories",
        Some(
            &json!({"name": "Mugs", "handle": "mugs", "parent": category["id"], "visible": true})
                .to_string(),
        ),
    )
    .await;
    let (_, _, _) = lists_on_the_catalog(
        &app,
        vec![
            json!({"name": "Kitchen sale", "currency": "EUR", "kind": "sale",
            "adjustment": {"percent_bps": -2000,
                "applies_to": {"type": "categories", "categories": [category["id"]]}}}),
        ],
    )
    .await;
    let product = mug_product(&app).await;
    let pk = publishable(&app).await;
    let read = format!("/storefront/products/{product}?country=HR");
    let (_, page) = send_as(&app, &pk, "GET", &read, None).await;
    assert_eq!(
        page["variants"][0]["price"],
        eur(1000),
        "not in the category"
    );

    // In a category below it: reached.
    let body = json!({ "categories": [below["id"]] }).to_string();
    let (status, json) = send(
        &app,
        "PUT",
        &format!("/admin/products/{product}/categories"),
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (_, page) = send_as(&app, &pk, "GET", &read, None).await;
    assert_eq!(page["variants"][0]["price"], eur(800));
}

#[tokio::test]
async fn price_lists_are_listed_filtered_and_deleted_when_free() {
    let app = app().await;
    let (market, mug, lists) =
        lists_on_the_catalog(&app, vec![json!({"name": "Retail", "currency": "EUR"})]).await;
    create_list(
        &app,
        json!({"name": "a sale", "currency": "EUR", "kind": "sale"}),
    )
    .await;
    create_list(
        &app,
        json!({"name": "Later", "currency": "EUR", "starts_at": "2999-01-01T00:00:00Z"}),
    )
    .await;
    let names = async |query: &str| {
        let (status, page) = send(&app, "GET", &format!("/admin/price-lists{query}"), None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(names("").await, ["Later", "a sale", "Retail"]);
    assert_eq!(names("?sort=name_asc").await, ["a sale", "Later", "Retail"]);
    assert_eq!(names("?kind=sale").await, ["a sale"]);
    assert_eq!(names("?status=scheduled").await, ["Later"]);
    assert_eq!(names(&format!("?market={market}")).await, ["Retail"]);
    let (_, page) = send(
        &app,
        "GET",
        "/admin/price-lists?sort=name_asc&limit=1",
        None,
    )
    .await;
    let cursor = page["next_cursor"].as_str().unwrap();
    assert_eq!(
        names(&format!("?sort=name_asc&limit=1&cursor={cursor}")).await,
        ["Later"]
    );
    let (status, error) = send(
        &app,
        "GET",
        &format!("/admin/price-lists?sort=name_desc&cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::BAD_REQUEST, "invalid_cursor")
    );

    // The list names its markets and counts its prices.
    let url = format!("/admin/price-lists/{}", lists[0]);
    let body = json!({"fixed_prices": [{"variant": mug, "price": eur(950)}]}).to_string();
    send(&app, "PUT", &format!("{url}/fixed-prices"), Some(&body)).await;
    let (_, list) = send(&app, "GET", &url, None).await;
    assert_eq!(
        (&list["markets"], &list["fixed_price_count"]),
        (&json!([market]), &json!(1))
    );
    let product = mug_product(&app).await;
    let other = "var_00000000000000000000000000000063";
    for (query, count) in [
        (format!("?product={product}"), 1),
        ("?q=MUG".to_string(), 1),
        ("?q=cup".to_string(), 0),
        (format!("?variants={mug},{other}"), 1),
        (format!("?variants={other}"), 0),
    ] {
        let (_, prices) = send(&app, "GET", &format!("{url}/fixed-prices{query}"), None).await;
        assert_eq!(prices["items"].as_array().unwrap().len(), count, "{query}");
    }

    let many: Vec<String> = (0..251).map(|n| format!("var_{:032x}", n + 1)).collect();
    let (status, error) = send(
        &app,
        "GET",
        &format!("{url}/fixed-prices?variants={}", many.join(",")),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&error), &error["details"]),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "too_many_filter_ids",
            &json!({"field": "variants", "max": 250})
        )
    );

    let (status, error) = send(&app, "DELETE", &url, None).await;
    assert_eq!(
        (
            status,
            code(&error),
            error["details"]["catalogs"].as_array().map(Vec::len)
        ),
        (StatusCode::CONFLICT, "price_list_in_use", Some(1))
    );
    let (_, read) = send(&app, "GET", &format!("/admin/markets/{market}"), None).await;
    let catalog = read["catalog"].as_str().unwrap();
    send(
        &app,
        "PUT",
        &format!("/admin/catalogs/{catalog}/price-lists"),
        Some(r#"{"price_lists":[]}"#),
    )
    .await;
    let (status, _) = send(&app, "DELETE", &url, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, error) = send(&app, "GET", &url, None).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::NOT_FOUND, "price_list_not_found")
    );
    let (status, error) = send(&app, "DELETE", &url, None).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::NOT_FOUND, "price_list_not_found")
    );
}

#[tokio::test]
async fn quantity_prices_show_on_the_product_and_price_the_cart_line() {
    let app = app().await;
    let (_, mug, lists) =
        lists_on_the_catalog(&app, vec![json!({"name": "Wholesale", "currency": "EUR"})]).await;
    let url = format!("/admin/price-lists/{}/fixed-prices", lists[0]);
    let body = json!({"fixed_prices": [{"variant": mug, "price": eur(1000),
        "breaks": [{"min_quantity": 5, "price": eur(900)}, {"min_quantity": 10, "price": eur(800)}]}]});
    let (status, json) = send(&app, "PUT", &url, Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");
    let (_, prices) = send(&app, "GET", &url, None).await;
    assert_eq!(
        prices["items"][0]["breaks"][1],
        json!({"min_quantity": 10, "price": eur(800)})
    );

    let bad = json!({"fixed_prices": [{"variant": mug, "price": eur(1000),
        "breaks": [{"min_quantity": 5, "price": eur(1000)}]}]});
    let (status, error) = send(&app, "PUT", &url, Some(&bad.to_string())).await;
    assert_eq!(
        (status, &error["details"]["entries"][0]["code"]),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            &json!("price_breaks_not_descending")
        )
    );
    // Past what storage keeps (a Postgres integer): refused, not a storage error.
    let huge = json!({"fixed_prices": [{"variant": mug, "price": eur(1000),
        "breaks": [{"min_quantity": 3_000_000_000u64, "price": eur(900)}]}]});
    let (status, error) = send(&app, "PUT", &url, Some(&huge.to_string())).await;
    assert_eq!(
        (status, &error["details"]["entries"][0]["code"]),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            &json!("price_break_quantity_invalid")
        )
    );

    let product = mug_product(&app).await;
    let pk = publishable(&app).await;
    let (_, page) = send_as(
        &app,
        &pk,
        "GET",
        &format!("/storefront/products/{product}?country=HR"),
        None,
    )
    .await;
    let variant = &page["variants"][0];
    assert_eq!(
        (
            &variant["price"],
            variant["price_breaks"].as_array().map(Vec::len)
        ),
        (&eur(1000), Some(2))
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
    let line = json!({"variant": mug, "quantity": 10}).to_string();
    let (_, cart) = send_as(&app, &pk, "POST", &lines, Some(&line)).await;
    assert_eq!(
        (&cart["lines"][0]["unit_price"], &cart["lines"][0]["total"]),
        (&eur(800), &eur(8000))
    );
    let (_, prices) = send(
        &app,
        "GET",
        &format!("/admin/products/{product}/prices?quantity=5"),
        None,
    )
    .await;
    assert_eq!(prices["markets"][0]["variants"][0]["price"], eur(900));
}

#[tokio::test]
async fn a_sales_start_is_announced_once_and_search_follows() {
    let app = app().await;
    let (_, mug, lists) = lists_on_the_catalog(
        &app,
        vec![json!({"name": "Weekend", "currency": "EUR", "kind": "sale",
            "starts_at": "2099-01-01T00:00:00Z", "adjustment": {"percent_bps": -2000}})],
    )
    .await;
    settle_search(&app).await;
    let pk = publishable(&app).await;
    let on_sale = "/storefront/search?country=HR&filter=on_sale:true";
    let (status, found) = send_as(&app, &pk, "GET", on_sale, None).await;
    assert_eq!(
        (status, &found["total"]),
        (StatusCode::OK, &json!(0)),
        "{found}"
    );

    // Time passes: the window opened an hour ago, after the job last looked; the list was
    // last written after that (a rename's event says nothing of prices), and the start is
    // still announced.
    let list: hypha_core::PriceListId = lists[0].parse().unwrap();
    sql(
        &app,
        &format!(
            "UPDATE price_lists SET starts_at = now() - interval '1 hour',
                 updated_at = now() - interval '30 minutes'
             WHERE id = '{}';
             INSERT INTO price_list_schedule (ran_until) VALUES (now() - interval '90 minutes');",
            list.as_uuid()
        ),
    )
    .await;
    let ran = hypha_http::jobs::price_list_schedule(&app.state)
        .await
        .unwrap();
    assert!(!ran.idle);
    let again = hypha_http::jobs::price_list_schedule(&app.state)
        .await
        .unwrap();
    assert!(again.idle, "announced once");

    settle_search(&app).await;
    let (_, found) = send_as(&app, &pk, "GET", on_sale, None).await;
    let hit = &found["items"][0];
    assert_eq!(
        (
            &found["total"],
            &hit["variant"],
            &hit["price"],
            &hit["compare_at"],
            &hit["on_sale"]
        ),
        (&json!(1), &json!(mug), &eur(800), &eur(1000), &json!(true)),
        "{found}"
    );
}

#[tokio::test]
async fn a_list_for_a_group_prices_only_for_its_signed_in_members() {
    use super::accounts::{send_signed_in, signed_in};
    let app = app().await;
    let (status, group) = send(
        &app,
        "POST",
        "/admin/customer-groups",
        Some(r#"{"name":"Wholesale","handle":"wholesale"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{group}");
    let group = group["id"].as_str().unwrap().to_string();
    let unknown = "cgrp_00000000000000000000000000000063";

    let body = json!({"name": "Nobody's", "currency": "EUR", "audience": {"groups": [unknown]}});
    let (status, error) = send(&app, "POST", "/admin/price-lists", Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "customer_group_not_found")
    );

    let (market, mug, lists) = lists_on_the_catalog(
        &app,
        vec![json!({"name": "Trade", "currency": "EUR",
            "adjustment": {"percent_bps": -2000}, "audience": {"groups": [group]}})],
    )
    .await;
    let trade = &lists[0];
    let (_, listed) = send(
        &app,
        "GET",
        &format!("/admin/price-lists?group={group}"),
        None,
    )
    .await;
    assert_eq!(
        (&listed["items"][0]["id"], &listed["items"][0]["audience"]),
        (&json!(trade), &json!({"groups": [group]}))
    );
    let (_, listed) = send(
        &app,
        "GET",
        &format!("/admin/price-lists?group={unknown}"),
        None,
    )
    .await;
    assert_eq!(listed["items"], json!([]));

    let product = mug_product(&app).await;
    let read = format!("/storefront/products/{product}?country=HR");
    let price_seen = |page: &Value| page["variants"][0]["price"].clone();
    let pk = publishable(&app).await;
    let (_, page) = send_as(&app, &pk, "GET", &read, None).await;
    assert_eq!(price_seen(&page), eur(1000), "anyone: the public price");

    let (pk, session) = signed_in(&app, "ana@example.hr").await;
    let (_, page) = send_signed_in(&app, &pk, &session, "GET", &read, None).await;
    assert_eq!(price_seen(&page), eur(1000), "signed in, in no group");
    let (_, account) =
        send_signed_in(&app, &pk, &session, "GET", "/storefront/account", None).await;
    let members = json!({"customers": [account["id"]]}).to_string();
    let path = format!("/admin/customer-groups/{group}/customers");
    let (status, json) = send(&app, "POST", &path, Some(&members)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");
    let (_, page) = send_signed_in(&app, &pk, &session, "GET", &read, None).await;
    assert_eq!(price_seen(&page), eur(800), "in the group");

    // Their cart is priced for them; a stranger's isn't.
    let line = json!({"variant": mug, "quantity": 1}).to_string();
    let (_, cart) = send_signed_in(
        &app,
        &pk,
        &session,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let lines = format!("/storefront/carts/{}/lines", cart["id"].as_str().unwrap());
    let (_, cart) = send_signed_in(&app, &pk, &session, "POST", &lines, Some(&line)).await;
    assert_eq!(cart["lines"][0]["unit_price"], eur(800), "{cart}");
    let (_, cart) = send_as(
        &app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let lines = format!("/storefront/carts/{}/lines", cart["id"].as_str().unwrap());
    let (_, cart) = send_as(&app, &pk, "POST", &lines, Some(&line)).await;
    assert_eq!(cart["lines"][0]["unit_price"], eur(1000), "{cart}");

    // The admin explains it for anyone, or for the group.
    let prices = format!("/admin/products/{product}/prices?market={market}");
    let (_, explained) = send(&app, "GET", &prices, None).await;
    let variant = &explained["markets"][0]["variants"][0];
    assert_eq!(
        (&variant["price"], &variant["candidates"][0]["result"]),
        (&eur(1000), &json!("not_in_audience"))
    );
    let (_, explained) = send(&app, "GET", &format!("{prices}&group={group}"), None).await;
    let variant = &explained["markets"][0]["variants"][0];
    assert_eq!(
        (&variant["price"], &variant["candidates"][0]["result"]),
        (&eur(800), &json!("won"))
    );
    let (status, error) = send(&app, "GET", &format!("{prices}&group={unknown}"), None).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::NOT_FOUND, "customer_group_not_found")
    );

    // A group an audience names stays until the list lets it go.
    let group_path = format!("/admin/customer-groups/{group}");
    let (status, error) = send(&app, "DELETE", &group_path, None).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::CONFLICT, "customer_group_in_use")
    );
    let (status, list) = send(
        &app,
        "PATCH",
        &format!("/admin/price-lists/{trade}"),
        Some(r#"{"audience":{"groups":[]}}"#),
    )
    .await;
    assert_eq!(
        (status, &list["audience"]),
        (StatusCode::OK, &json!({"groups": []}))
    );
    let (_, page) = send_as(&app, &pk, "GET", &read, None).await;
    assert_eq!(price_seen(&page), eur(800), "everyone's now");
    let (status, _) = send(&app, "DELETE", &group_path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn an_order_line_keeps_its_compare_at_price_and_price_list() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    let sale = create_list(
        &app,
        json!({"name": "Sale", "currency": "EUR", "kind": "sale",
            "adjustment": {"percent_bps": -2000}}),
    )
    .await;
    let (_, markets) = send(&app, "GET", "/admin/markets", None).await;
    let market = markets["items"][0]["id"].as_str().unwrap();
    let (_, market) = send(&app, "GET", &format!("/admin/markets/{market}"), None).await;
    let catalog = market["catalog"].as_str().unwrap();
    let body = json!({ "price_lists": [sale["id"]] }).to_string();
    let path = format!("/admin/catalogs/{catalog}/price-lists");
    let (status, json) = send(&app, "PUT", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    pay_on_delivery(&app, &pk, &cart_path).await;
    let (status, order) = send_as(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/checkout"),
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{order}");
    let line = &order["lines"][0];
    // Which list priced it is the merchant's business.
    assert_eq!(
        (
            &line["unit_price"],
            &line["compare_at_price"],
            line.get("price_list")
        ),
        (&eur(800), &eur(1000), None),
        "the storefront's"
    );
    let token = order["access_token"].as_str().unwrap();
    let (_, placed) = send_as(
        &app,
        &pk,
        "GET",
        &format!(
            "/storefront/orders/{}?token={token}",
            order["id"].as_str().unwrap()
        ),
        None,
    )
    .await;
    assert_eq!(placed["lines"][0].get("price_list"), None, "{placed}");
    let expected = (&eur(800), &eur(1000), &sale["id"]);
    let id = order["id"].as_str().unwrap();
    let (_, admin) = send(&app, "GET", &format!("/admin/orders/{id}"), None).await;
    let line = &admin["lines"][0];
    assert_eq!(
        (
            &line["unit_price"],
            &line["compare_at_price"],
            &line["price_list"]
        ),
        expected,
        "the admin's"
    );
}
