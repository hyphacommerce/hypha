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
