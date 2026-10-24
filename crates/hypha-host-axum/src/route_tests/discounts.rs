//! Discounts: the admin's discounts and codes, codes on carts, and what orders keep.

use serde_json::json;

use super::*;

fn eur(amount: i64) -> Value {
    json!({"amount": amount, "currency": "EUR"})
}

/// A GET with the admin key, answered as text (a CSV).
async fn send_raw(app: &TestApp, method: &str, uri: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {}", app.admin_key))
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn create(app: &TestApp, body: Value) -> Value {
    let (status, d) = send(app, "POST", "/admin/discounts", Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::CREATED, "{d}");
    d
}

fn ten_percent_off_the_order() -> Value {
    json!({"type": "standard", "rewards": [
        {"target": {"type": "subtotal"}, "value": {"type": "percent", "basis_points": 1000}}
    ]})
}

#[tokio::test]
async fn a_welcome_code_is_one_call() {
    let app = app().await;
    let d = create(
        &app,
        json!({"title": "Welcome", "method": "codes", "kind": ten_percent_off_the_order(),
            "codes": ["welcome10"], "limits": {"once_per_customer": true}}),
    )
    .await;
    assert_eq!(
        (
            &d["method"],
            &d["status"],
            &d["phase"],
            &d["classes"],
            &d["combines_with"],
            &d["summary"],
            &d["code_count"],
            &d["used"],
            &d["limits"],
        ),
        (
            &json!("codes"),
            &json!("active"),
            &json!("active"),
            &json!(["order"]),
            &json!([]),
            &json!("10% off the order · once per customer"),
            &json!(1),
            &json!(0),
            &json!({"total": null, "once_per_customer": true}),
        )
    );
    let url = format!("/admin/discounts/{}", d["id"].as_str().unwrap());
    let (status, fetched) = send(&app, "GET", &url, None).await;
    assert_eq!(
        (status, &fetched["kind"]),
        (StatusCode::OK, &ten_percent_off_the_order())
    );

    let (_, codes) = send(&app, "GET", &format!("{url}/codes"), None).await;
    assert_eq!(
        (&codes["items"][0]["code"], &codes["items"][0]["normalized"]),
        (&json!("welcome10"), &json!("WELCOME10"))
    );
    // Found by the code, however it's typed.
    let (_, found) = send(&app, "GET", "/admin/discounts?code=Welcome10", None).await;
    assert_eq!(ids(&found), vec![d["id"].as_str().unwrap()]);
}

#[tokio::test]
async fn every_broken_rule_is_answered_with_its_path() {
    let app = app().await;
    let body = json!({"title": " ", "method": "automatic", "kind": {"type": "standard",
        "rewards": [{"target": {"type": "subtotal"},
            "value": {"type": "percent", "basis_points": 10001}}]},
        "limits": {"total": 0}});
    let (status, error) = send(&app, "POST", "/admin/discounts", Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_discount")
    );
    let paths: Vec<&str> = error["details"]["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        vec![
            "title",
            "limits.total",
            "kind.rewards[0].value.basis_points"
        ]
    );
}

#[tokio::test]
async fn amounts_are_per_market_in_its_currency() {
    let app = app().await;
    let (market, _) = market_selling_a_mug(&app).await;
    let amount_off = |amount: Value| {
        json!({"title": "5 off", "method": "automatic", "kind": {"type": "standard",
            "rewards": [{"target": {"type": "subtotal"},
                "value": {"type": "amount", "amounts": {market.clone(): amount}}}]}})
    };
    let (status, error) = send(
        &app,
        "POST",
        "/admin/discounts",
        Some(&amount_off(json!({"amount": 500, "currency": "USD"})).to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&error), &error["details"]["market"]),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "discount_currency_mismatch",
            &json!(market)
        )
    );
    let d = create(&app, amount_off(eur(500))).await;
    assert_eq!(d["summary"], json!("5.00 EUR off the order"));
    assert_eq!(
        d["kind"]["rewards"][0]["value"],
        json!({"type": "amount", "amounts": {market: eur(500)}, "allocation": {"type": "across"}})
    );
}

#[tokio::test]
async fn a_discount_is_changed_and_deleted() {
    let app = app().await;
    let d = create(
        &app,
        json!({"title": "Sale", "method": "codes", "kind": ten_percent_off_the_order(),
            "codes": ["SALE"]}),
    )
    .await;
    let url = format!("/admin/discounts/{}", d["id"].as_str().unwrap());
    let patch = json!({"status": "draft", "ends_at": "2999-01-01T00:00:00Z",
        "combines_with": ["product", "shipping"]});
    let (status, changed) = send(&app, "PATCH", &url, Some(&patch.to_string())).await;
    assert_eq!(
        (status, &changed["phase"], &changed["combines_with"]),
        (
            StatusCode::OK,
            &json!("draft"),
            &json!(["product", "shipping"])
        )
    );
    let (status, error) = send(&app, "PATCH", &url, Some(r#"{"method":"automatic"}"#)).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body")
    );
    let (status, _) = send(&app, "DELETE", &url, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, error) = send(&app, "GET", &url, None).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::NOT_FOUND, "discount_not_found")
    );
    // Its code is free for another discount.
    create(
        &app,
        json!({"title": "Again", "method": "codes", "kind": ten_percent_off_the_order(),
            "codes": ["sale"]}),
    )
    .await;
}

#[tokio::test]
async fn codes_are_added_generated_exported_and_removed() {
    let app = app().await;
    let d = create(
        &app,
        json!({"title": "Newsletter", "method": "codes", "kind": ten_percent_off_the_order(),
            "codes": ["TAKEN"]}),
    )
    .await;
    let url = format!("/admin/discounts/{}", d["id"].as_str().unwrap());

    let body = json!({"codes": [{"code": "NEW"}, {"code": "taken"}]}).to_string();
    let (status, error) = send(&app, "POST", &format!("{url}/codes"), Some(&body)).await;
    assert_eq!(
        (status, code(&error), &error["details"]["code"]),
        (StatusCode::CONFLICT, "discount_code_taken", &json!("taken"))
    );

    let body = json!({"count": 5, "prefix": "nl-", "length": 10, "batch": "October"}).to_string();
    let (status, generated) =
        send(&app, "POST", &format!("{url}/codes/generate"), Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{generated}");
    let codes = generated["codes"].as_array().unwrap();
    assert_eq!(codes.len(), 5);
    for c in codes {
        let text = c["code"].as_str().unwrap();
        assert!(text.starts_with("NL-") && text.len() == 10, "{text}");
        assert!(!text[3..].contains(['0', 'O', '1', 'I', 'L']), "{text}");
        assert_eq!(c["usage_limit"], json!(1));
    }

    let (status, csv) = send_raw(&app, "GET", &format!("{url}/codes/export?batch=October")).await;
    assert_eq!(status, StatusCode::OK);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        (lines[0], lines.len()),
        (
            "code,normalized,usage_limit,used,active,batch,created_at",
            6
        )
    );

    let body = json!({"batch": "October"}).to_string();
    let (status, removed) = send(&app, "DELETE", &format!("{url}/codes"), Some(&body)).await;
    assert_eq!((status, &removed["removed"]), (StatusCode::OK, &json!(5)));
    let (_, d) = send(&app, "GET", &url, None).await;
    assert_eq!(d["code_count"], json!(1));

    let (_, usage) = send(&app, "GET", &format!("{url}/usage"), None).await;
    assert_eq!(usage["items"], json!([]));
}

#[tokio::test]
async fn an_automatic_discount_takes_no_codes() {
    let app = app().await;
    let body = json!({"title": "Auto", "method": "automatic", "kind": ten_percent_off_the_order(),
        "codes": ["X"]});
    let (status, error) = send(&app, "POST", "/admin/discounts", Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::CONFLICT, "discount_automatic")
    );
}

#[tokio::test]
async fn discounts_list_by_status_method_and_class() {
    let app = app().await;
    let auto = create(
        &app,
        json!({"title": "Free shipping", "method": "automatic", "kind": {"type": "standard",
            "rewards": [{"target": {"type": "shipping"},
                "value": {"type": "percent", "basis_points": 10000}}]}}),
    )
    .await;
    let draft = create(
        &app,
        json!({"title": "Later", "method": "codes", "status": "draft",
            "kind": ten_percent_off_the_order()}),
    )
    .await;
    let list = |q: &'static str| {
        let app = &app;
        async move {
            let (status, page) = send(app, "GET", &format!("/admin/discounts{q}"), None).await;
            assert_eq!(status, StatusCode::OK, "{page}");
            ids(&page)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        }
    };
    let (auto, draft) = (auto["id"].as_str().unwrap(), draft["id"].as_str().unwrap());
    assert_eq!(list("").await, vec![draft, auto]);
    assert_eq!(list("?status=draft").await, vec![draft]);
    assert_eq!(list("?method=automatic").await, vec![auto]);
    assert_eq!(list("?class=shipping").await, vec![auto]);
    assert_eq!(list("?q=SHIPPING").await, vec![auto]);
}

#[tokio::test]
async fn discounts_need_their_scopes() {
    let app = app().await;
    let reader = key(
        &app,
        json!({"kind": "secret", "name": "Reader", "scopes": ["discounts:read"]}),
    )
    .await;
    let (status, _) = send_as(&app, &reader, "GET", "/admin/discounts", None).await;
    assert_eq!(status, StatusCode::OK);
    let body = json!({"title": "No", "method": "codes", "kind": ten_percent_off_the_order()});
    let (status, error) = send_as(
        &app,
        &reader,
        "POST",
        "/admin/discounts",
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
}

/// A storefront cart for `quantity` mugs (10.00 each) in Croatia: the key and the cart's
/// path.
async fn cart_of_mugs(app: &TestApp, mug: &str, quantity: u32) -> (String, String) {
    let pk = publishable(app).await;
    let (_, cart) = send_as(
        app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let path = format!("/storefront/carts/{}", cart["id"].as_str().unwrap());
    let line = json!({"variant": mug, "quantity": quantity}).to_string();
    let (status, body) = send_as(app, &pk, "POST", &format!("{path}/lines"), Some(&line)).await;
    assert!(status.is_success(), "{body}");
    (pk, path)
}

#[tokio::test]
async fn a_code_on_a_cart_takes_its_discount_off_the_lines() {
    let app = app().await;
    create(
        &app,
        json!({"title": "Welcome", "method": "codes", "kind": ten_percent_off_the_order(),
            "codes": ["WELCOME10"]}),
    )
    .await;
    let (_, mug) = market_selling_a_mug(&app).await;
    let (pk, cart) = cart_of_mugs(&app, &mug, 3).await;
    let codes = format!("{cart}/discount-codes");

    let (status, error) = send_as(&app, &pk, "POST", &codes, Some(r#"{"code":"NOPE"}"#)).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "discount_code_invalid")
    );

    let (status, priced) =
        send_as(&app, &pk, "POST", &codes, Some(r#"{"code":" welcome10"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{priced}");
    assert_eq!(
        (
            &priced["discount_total"],
            &priced["subtotal"],
            &priced["total"],
            &priced["discount_codes"],
        ),
        (
            &eur(300),
            &eur(2700),
            &eur(2700),
            &json!([{"code": "WELCOME10", "state": "applied"}]),
        )
    );
    assert_eq!(
        (
            &priced["discounts"][0]["title"],
            &priced["discounts"][0]["code"],
            &priced["discounts"][0]["class"],
            &priced["discounts"][0]["summary"],
        ),
        (
            &json!("Welcome"),
            &json!("WELCOME10"),
            &json!("order"),
            &json!("10% off the order")
        )
    );
    let line = &priced["lines"][0];
    assert_eq!(
        (&line["subtotal"], &line["total"], &line["discounts"]),
        (
            &eur(3000),
            &eur(2700),
            &json!([{"application": 0, "amount": eur(300)}])
        )
    );

    let (status, priced) = send_as(&app, &pk, "DELETE", &format!("{codes}/welcome10"), None).await;
    assert_eq!(
        (status, &priced["total"], &priced["discount_codes"]),
        (StatusCode::OK, &eur(3000), &json!([]))
    );
    let (status, error) = send_as(&app, &pk, "DELETE", &format!("{codes}/WELCOME10"), None).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::NOT_FOUND, "discount_code_not_on_cart")
    );
}

#[tokio::test]
async fn a_code_short_of_its_minimum_is_kept_with_what_is_missing() {
    let app = app().await;
    let (market, mug) = market_selling_a_mug(&app).await;
    create(
        &app,
        json!({"title": "Big order", "method": "codes", "kind": ten_percent_off_the_order(),
            "codes": ["BIG"], "conditions": [{"type": "min_subtotal",
                "amounts": {market: eur(5000)}}]}),
    )
    .await;
    let (pk, cart) = cart_of_mugs(&app, &mug, 2).await;
    let (_, priced) = send_as(
        &app,
        &pk,
        "POST",
        &format!("{cart}/discount-codes"),
        Some(r#"{"code":"big"}"#),
    )
    .await;
    assert_eq!(
        priced["discount_codes"],
        json!([{"code": "BIG", "state": "minimum_not_met", "needed": {"subtotal": eur(3000)}}])
    );
    assert_eq!(priced["total"], eur(2000));
}

#[tokio::test]
async fn an_order_keeps_what_its_code_took_off_and_counts_the_use() {
    let app = app().await;
    let d = create(
        &app,
        json!({"title": "Welcome", "method": "codes", "kind": ten_percent_off_the_order(),
            "codes": ["WELCOME10"], "limits": {"total": 1}}),
    )
    .await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    let (_, priced) = send_as(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/discount-codes"),
        Some(r#"{"code":"welcome10"}"#),
    )
    .await;
    assert_eq!(priced["total"], eur(2700));
    pay_on_delivery(&app, &pk, &cart_path).await;
    let body = json!({"expected_total": eur(2700)}).to_string();
    let (status, placed) = send_as(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/checkout"),
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{placed}");
    let order = &placed;
    assert_eq!(
        (
            &order["discount_total"],
            &order["total"],
            &order["subtotal"]
        ),
        (&eur(300), &eur(2700), &eur(2700))
    );
    assert_eq!(
        (
            &order["discounts"][0]["code"],
            &order["discounts"][0]["value"],
            &order["lines"][0]["subtotal"],
            &order["lines"][0]["discounts"],
        ),
        (
            &json!("WELCOME10"),
            &json!({"type": "percent", "basis_points": 1000}),
            &eur(3000),
            &json!([{"application": 0, "amount": eur(300)}])
        )
    );

    let url = format!("/admin/discounts/{}", d["id"].as_str().unwrap());
    let (_, usage) = send(&app, "GET", &format!("{url}/usage"), None).await;
    assert_eq!(
        (&usage["items"][0]["code"], &usage["items"][0]["amount"]),
        (&json!("WELCOME10"), &eur(300))
    );
    let (_, d) = send(&app, "GET", &url, None).await;
    assert_eq!(d["used"], json!(1));

    // Its one use is taken: another cart keeps the code, not applied.
    let (_, cart) = send_as(
        &app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let other = format!("/storefront/carts/{}", cart["id"].as_str().unwrap());
    let (_, again) = send_as(
        &app,
        &pk,
        "POST",
        &format!("{other}/discount-codes"),
        Some(r#"{"code":"WELCOME10"}"#),
    )
    .await;
    assert_eq!(
        again["discount_codes"],
        json!([{"code": "WELCOME10", "state": "limit_reached"}])
    );
}

#[tokio::test]
async fn buy_two_get_one_free_takes_the_third_mug_off() {
    let app = app().await;
    let d = create(
        &app,
        json!({"title": "3 for 2", "method": "automatic", "kind": {"type": "buy_x_get_y",
            "buys": {"items": {"type": "all"}}, "buy": {"type": "quantity", "quantity": 2},
            "gets": {"items": {"type": "all"}}, "get_quantity": 1,
            "value": {"type": "percent", "basis_points": 10000}, "uses_per_order": 1}}),
    )
    .await;
    assert_eq!(
        (&d["classes"], &d["summary"]),
        (
            &json!(["product"]),
            &json!("Buy 2 × all items, get 1 × all items free (up to 1 time per order)")
        )
    );
    let (_, mug) = market_selling_a_mug(&app).await;
    let (pk, cart) = cart_of_mugs(&app, &mug, 3).await;
    let (_, priced) = send_as(&app, &pk, "GET", &cart, None).await;
    assert_eq!(
        (
            &priced["discount_total"],
            &priced["total"],
            &priced["lines"][0]["discounts"]
        ),
        (
            &eur(1000),
            &eur(2000),
            &json!([{"application": 0, "amount": eur(1000), "units": 1}])
        )
    );
}

#[tokio::test]
async fn volume_tiers_apply_the_highest_reached() {
    let app = app().await;
    let tiers = json!({"title": "Volume", "method": "automatic", "kind": {"type": "tiered",
        "items": {"items": {"type": "all"}}, "measure": "quantity", "tiers": [
            {"threshold": {"type": "quantity", "quantity": 2},
             "value": {"type": "percent", "basis_points": 1000}},
            {"threshold": {"type": "quantity", "quantity": 3},
             "value": {"type": "percent", "basis_points": 1500}}]}});
    create(&app, tiers).await;
    let (_, mug) = market_selling_a_mug(&app).await;
    let (pk, cart) = cart_of_mugs(&app, &mug, 3).await;
    let (_, priced) = send_as(&app, &pk, "GET", &cart, None).await;
    assert_eq!(
        (&priced["discount_total"], &priced["discounts"][0]["value"]),
        (&eur(450), &json!({"type": "percent", "basis_points": 1500}))
    );

    let falling = json!({"title": "Wrong", "method": "automatic", "kind": {"type": "tiered",
        "items": {"items": {"type": "all"}}, "measure": "quantity", "tiers": [
            {"threshold": {"type": "quantity", "quantity": 3},
             "value": {"type": "percent", "basis_points": 1000}},
            {"threshold": {"type": "quantity", "quantity": 2},
             "value": {"type": "percent", "basis_points": 1500}}]}});
    let (status, error) = send(&app, "POST", "/admin/discounts", Some(&falling.to_string())).await;
    assert_eq!(
        (
            status,
            code(&error),
            &error["details"]["problems"][0]["path"]
        ),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_discount",
            &json!("kind.tiers[1].threshold")
        )
    );
}

#[tokio::test]
async fn a_product_shows_the_promotions_that_may_reach_it() {
    let app = app().await;
    create(
        &app,
        json!({"title": "3 for 2", "method": "automatic", "kind": {"type": "buy_x_get_y",
            "buys": {"items": {"type": "all"}}, "buy": {"type": "quantity", "quantity": 2},
            "gets": {"items": {"type": "all"}}, "get_quantity": 1,
            "value": {"type": "percent", "basis_points": 10000}}}),
    )
    .await;
    // Only for signed-in customers: never a badge every shopper sees.
    create(
        &app,
        json!({"title": "Members", "method": "automatic", "conditions": [{"type": "signed_in"}],
            "kind": ten_percent_off_the_order()}),
    )
    .await;
    // An order discount reaches no product.
    create(
        &app,
        json!({"title": "Order", "method": "automatic", "kind": ten_percent_off_the_order()}),
    )
    .await;
    market_selling_a_mug(&app).await;
    let pk = publishable(&app).await;
    let (status, product) = send_as(
        &app,
        &pk,
        "GET",
        "/storefront/products/by-handle/mug?country=HR",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{product}");
    let titles: Vec<&str> = product["promotions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["3 for 2"]);
    let (_, without_country) =
        send_as(&app, &pk, "GET", "/storefront/products/by-handle/mug", None).await;
    assert_eq!(without_country["promotions"], json!([]));
}
