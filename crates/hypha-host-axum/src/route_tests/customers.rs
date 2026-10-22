//! Customers: deleting one, and later consent, groups and erasure.

use serde_json::json;

use super::*;

/// Creates a customer as the admin; returns its ID.
async fn customer(app: &TestApp, email: &str) -> String {
    let body = json!({ "email": email }).to_string();
    let (status, created) = send(app, "POST", "/admin/customers", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().unwrap().to_string()
}

/// A customer without orders is deleted (its carts' unlinking is the workflow's own test); one with
/// orders (cancelled or not) is refused and says so on `can_delete`.
#[tokio::test]
async fn a_customer_without_orders_is_deleted_and_one_with_orders_is_not() {
    let app = app().await;
    let ana = customer(&app, "ana@example.hr").await;
    let (_, json) = send(&app, "GET", &format!("/admin/customers/{ana}"), None).await;
    assert_eq!(json["can_delete"], true);
    assert_eq!(json["orders"]["cancelled"], 0);

    let (status, body) = send(&app, "DELETE", &format!("/admin/customers/{ana}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = send(&app, "GET", &format!("/admin/customers/{ana}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, json) = send(&app, "DELETE", &format!("/admin/customers/{ana}"), None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "customer_not_found")
    );

    // A customer with an order, even a cancelled one, is kept.
    let (order, _, _) = placed_order_for_three_mugs(&app).await;
    let (_, placed) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    let buyer = placed["customer"].as_str().unwrap().to_string();
    let (_, json) = send(&app, "GET", &format!("/admin/customers/{buyer}"), None).await;
    assert_eq!(json["can_delete"], false);
    let (status, json) = send(&app, "DELETE", &format!("/admin/customers/{buyer}"), None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "customer_has_orders")
    );
    let cancel = format!("/admin/orders/{order}/cancel");
    send(&app, "POST", &cancel, Some(r#"{"reason":"customer"}"#)).await;
    let (_, json) = send(&app, "GET", &format!("/admin/customers/{buyer}"), None).await;
    assert_eq!(json["orders"]["count"], 0);
    assert_eq!(json["orders"]["cancelled"], 1);
    assert_eq!(json["can_delete"], false);
    let (status, json) = send(&app, "DELETE", &format!("/admin/customers/{buyer}"), None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "customer_has_orders")
    );
}

/// Consent per channel over the API, the newest consent winning, and the cart's marketing
/// box becoming a subscription at checkout.
#[tokio::test]
async fn marketing_consent_is_recorded_newest_first_and_at_checkout() {
    let app = app().await;
    let ana = customer(&app, "ana@example.hr").await;
    let path = format!("/admin/customers/{ana}");
    let (_, json) = send(&app, "GET", &path, None).await;
    assert_eq!(
        json["email_marketing"],
        json!({"state": "not_subscribed", "opt_in_level": "unknown"})
    );

    // Subscribed yesterday, by staff.
    let consent = format!("{path}/marketing/email");
    let body = json!({"state": "subscribed", "opt_in_level": "confirmed",
                      "consented_at": "2026-10-20T10:00:00Z"});
    let (status, json) = send(&app, "PUT", &consent, Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["applied"], true);
    assert_eq!(json["customer"]["email_marketing"]["state"], "subscribed");
    assert_eq!(json["customer"]["email_marketing"]["source"], "admin");
    assert_eq!(
        json["customer"]["email_marketing"]["consented_at"],
        "2026-10-20T10:00:00Z"
    );

    // A replayed, older unsubscribe changes nothing.
    let older = json!({"state": "unsubscribed", "consented_at": "2026-10-19T10:00:00Z"});
    let (status, json) = send(&app, "PUT", &consent, Some(&older.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["applied"], false);
    assert_eq!(json["customer"]["email_marketing"]["state"], "subscribed");

    // What can't be asked for, and SMS without a phone.
    let (status, json) = send(
        &app,
        "PUT",
        &consent,
        Some(&json!({"state": "not_subscribed"}).to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "consent_state_not_settable"
        )
    );
    let sms = format!("{path}/marketing/sms");
    let (status, json) = send(
        &app,
        "PUT",
        &sms,
        Some(&json!({"state": "subscribed"}).to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "consent_needs_phone")
    );
    let (status, _) = send(
        &app,
        "PUT",
        &format!("{path}/marketing/fax"),
        Some(&json!({"state": "subscribed"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A new email resets the consent.
    let (status, json) = send(
        &app,
        "PATCH",
        &path,
        Some(&json!({"email": "ana.horvat@example.hr"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["email_marketing"]["state"], "not_subscribed");

    // The box at checkout: the guest subscribes from checkout.
    let (order, _, _) = placed_order_for_three_mugs_opting_in(&app).await;
    let (_, placed) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    let buyer = placed["customer"].as_str().unwrap();
    let (_, json) = send(&app, "GET", &format!("/admin/customers/{buyer}"), None).await;
    assert_eq!(json["email_marketing"]["state"], "subscribed");
    assert_eq!(json["email_marketing"]["opt_in_level"], "single");
    assert_eq!(json["email_marketing"]["source"], "checkout");
}

/// Groups are created, changed, filled and emptied as sets, filter the customers list, and
/// show on the customer; deleting one leaves its customers.
#[tokio::test]
async fn customer_groups_hold_customers_as_sets() {
    let app = app().await;
    let ana = customer(&app, "ana@example.hr").await;
    let ivo = customer(&app, "ivo@example.hr").await;

    let (status, vip) = send(
        &app,
        "POST",
        "/admin/customer-groups",
        Some(&json!({"name": "VIP", "handle": "vip", "description": "Our best"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{vip}");
    let vip_id = vip["id"].as_str().unwrap().to_string();
    assert!(vip_id.starts_with("cgrp_"));
    assert_eq!(vip["handle"], "vip");

    // Unique handle and name (ignoring case); a bad handle is invalid input.
    let (status, json) = send(
        &app,
        "POST",
        "/admin/customer-groups",
        Some(&json!({"name": "Other", "handle": "vip"}).to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "customer_group_handle_taken")
    );
    let (status, json) = send(
        &app,
        "POST",
        "/admin/customer-groups",
        Some(&json!({"name": "vip", "handle": "other"}).to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "customer_group_name_taken")
    );
    let (status, json) = send(
        &app,
        "POST",
        "/admin/customer-groups",
        Some(&json!({"name": "Bad", "handle": "Not A Handle"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");

    let path = format!("/admin/customer-groups/{vip_id}");
    let (status, json) = send(
        &app,
        "PATCH",
        &path,
        Some(&json!({"name": "Very important", "description": null}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["name"], "Very important");
    assert_eq!(json["description"], Value::Null);

    // Membership as sets: adding twice is once; an unknown customer is refused whole.
    let members = format!("{path}/customers");
    let (status, body) = send(
        &app,
        "POST",
        &members,
        Some(&json!({"customers": [ana, ana, ivo]}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, json) = send(
        &app,
        "POST",
        &members,
        Some(&json!({"customers": ["cus_00000000000000000000000000000063"]}).to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "customer_not_found")
    );
    let (_, page) = send(
        &app,
        "GET",
        &format!("/admin/customers?group={vip_id}"),
        None,
    )
    .await;
    let mut in_group: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    in_group.sort();
    let mut expected = vec![ana.as_str(), ivo.as_str()];
    expected.sort();
    assert_eq!(in_group, expected);
    let (_, json) = send(&app, "GET", &format!("/admin/customers/{ana}"), None).await;
    assert_eq!(json["groups"], json!([vip_id]));

    let (status, _) = send(
        &app,
        "DELETE",
        &members,
        Some(&json!({"customers": [ivo]}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, page) = send(
        &app,
        "GET",
        &format!("/admin/customers?group={vip_id}"),
        None,
    )
    .await;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);

    // The customer's side: set the groups whole.
    let (status, trade) = send(
        &app,
        "POST",
        "/admin/customer-groups",
        Some(&json!({"name": "Trade", "handle": "trade"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{trade}");
    let trade_id = trade["id"].as_str().unwrap().to_string();
    let (status, json) = send(
        &app,
        "PUT",
        &format!("/admin/customers/{ana}/groups"),
        Some(&json!({"groups": [trade_id]}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["groups"], json!([trade_id]));
    let (status, json) = send(
        &app,
        "PUT",
        &format!("/admin/customers/{ana}/groups"),
        Some(&json!({"groups": ["cgrp_00000000000000000000000000000063"]}).to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "customer_group_not_found")
    );

    let (_, list) = send(&app, "GET", "/admin/customer-groups", None).await;
    let names: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Trade", "Very important"]);

    // Deleting a group leaves its customers.
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/admin/customer-groups/{trade_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        "GET",
        &format!("/admin/customer-groups/{trade_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, json) = send(&app, "GET", &format!("/admin/customers/{ana}"), None).await;
    assert_eq!(json["groups"], json!([]));
}

/// An erasure is requested and cancelled over the API, and the export answers a data
/// request.
#[tokio::test]
async fn erasure_is_requested_and_cancelled_and_data_is_exported() {
    let app = app().await;
    let (order, _, _) = placed_order_for_three_mugs(&app).await;
    let (_, placed) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    let buyer = placed["customer"].as_str().unwrap().to_string();
    let path = format!("/admin/customers/{buyer}");

    let erasure = format!("{path}/erasure");
    let (status, json) = send(&app, "POST", &erasure, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["erasure_due_at"].as_str().is_some());
    assert_eq!(json["erased_at"], Value::Null);
    let (status, json) = send(&app, "POST", &erasure, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "erasure_already_requested")
    );
    let (status, json) = send(&app, "DELETE", &erasure, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["erasure_due_at"], Value::Null);
    let (status, json) = send(&app, "DELETE", &erasure, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "erasure_not_requested")
    );

    let (status, json) = send(&app, "GET", &format!("{path}/export"), None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["customer"]["id"], buyer);
    assert_eq!(json["orders"][0]["id"], order);
    assert!(json["exported_at"].as_str().is_some());
    assert!(json["payments"].is_array());
    assert!(json["emails"].is_array());
}
