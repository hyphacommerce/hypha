//! Customer accounts: a code emailed, a session beside the publishable key, signing out.

use serde_json::json;

use super::*;

/// The six digits in the last email sent.
fn last_code(app: &TestApp) -> String {
    let email = app.emails.lock().unwrap().last().cloned().unwrap();
    email
        .text
        .split(|c: char| !c.is_ascii_digit())
        .find(|w| w.len() == 6)
        .unwrap_or_else(|| panic!("no code in {}", email.text))
        .to_string()
}

fn emails_sent(app: &TestApp) -> usize {
    app.emails.lock().unwrap().len()
}

/// Asks for a code for `email` with the key `pk`: 202 `{sent: true}`.
async fn ask(app: &TestApp, pk: &str, email: &str) {
    let body = json!({ "email": email }).to_string();
    let (status, json) = send_as(app, pk, "POST", "/storefront/auth/code", Some(&body)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{json}");
    assert_eq!(json, json!({ "sent": true }));
}

/// Signs in with `code`; the answer as is.
async fn login(
    app: &TestApp,
    pk: &str,
    email: &str,
    code: &str,
    cart: Option<&str>,
) -> (StatusCode, Value) {
    let body = json!({ "email": email, "code": code }).to_string();
    let mut headers = vec![("x-publishable-key", pk)];
    if let Some(cart) = cart {
        headers.push(("x-hypha-cart", cart));
    }
    send_with(app, &headers, "POST", "/storefront/auth/login", Some(&body)).await
}

/// A request with the key and the customer's session.
pub(super) async fn send_signed_in(
    app: &TestApp,
    pk: &str,
    session: &str,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let auth = format!("Bearer {session}");
    send_with(
        app,
        &[("x-publishable-key", pk), ("authorization", &auth)],
        method,
        uri,
        body,
    )
    .await
}

#[tokio::test]
async fn a_customer_signs_in_with_an_emailed_code_and_signs_out() {
    let app = app().await;
    let pk = publishable(&app).await;
    ask(&app, &pk, "Nina@Example.com").await;
    let email = app.emails.lock().unwrap().last().cloned().unwrap();
    assert_eq!(email.to.as_str(), "nina@example.com");
    let digits = last_code(&app);
    // The subject is kept in the message record, so the code stays out of it.
    assert!(!email.subject.contains(&digits), "{}", email.subject);
    assert!(email.text.contains(&digits), "{}", email.text);

    let wrong = if digits == "000000" {
        "000001"
    } else {
        "000000"
    };
    let (status, json) = login(&app, &pk, "nina@example.com", wrong, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "invalid_code")
    );
    let (status, json) = login(&app, &pk, "nina@example.com", &digits, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let token = json["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("hypha_cs_"), "{token}");
    assert_eq!(json["customer"]["email"], "nina@example.com");
    assert!(json["expires_at"].is_string());
    let customer = json["customer"]["id"].as_str().unwrap().to_string();
    // The customer was created at the first request, and the admin sees them.
    let (status, admin) = send(&app, "GET", &format!("/admin/customers/{customer}"), None).await;
    assert_eq!(status, StatusCode::OK, "{admin}");
    assert_eq!(admin["email"], "nina@example.com");

    // A code works once.
    let (status, json) = login(&app, &pk, "nina@example.com", &digits, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "invalid_code")
    );

    // Signing out needs the session; afterwards it's gone.
    let (status, json) = send_as(&app, &pk, "POST", "/storefront/account/logout", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "account_required")
    );
    let (status, _) = send_signed_in(
        &app,
        &pk,
        &token,
        "POST",
        "/storefront/account/logout",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, json) = send_signed_in(
        &app,
        &pk,
        &token,
        "POST",
        "/storefront/account/logout",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );
}

#[tokio::test]
async fn signing_in_links_the_cart_and_the_session_works_only_beside_its_key() {
    let app = app().await;
    let (pk, cart_path, _) = cart_for_three_mugs(&app).await;
    let cart = cart_path.rsplit('/').next().unwrap().to_string();
    ask(&app, &pk, "nina@example.com").await;
    let digits = last_code(&app);
    let (status, json) = login(&app, &pk, "nina@example.com", &digits, Some(&cart)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let token = json["token"].as_str().unwrap().to_string();
    let customer = json["customer"]["id"].clone();
    let (_, cart_now) = send_as(&app, &pk, "GET", &cart_path, None).await;
    assert_eq!(
        cart_now["email"], "nina@example.com",
        "the cart is hers now"
    );
    assert_eq!(cart_now["customer"], customer);

    // A bad cart header is refused before anything else happens.
    let (status, json) = login(&app, &pk, "nina@example.com", &digits, Some("nope")).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::BAD_REQUEST, "invalid_cart_header")
    );

    // The session is for the storefront, beside the key: not for the admin, not alone,
    // and no other token rides beside a publishable key.
    let (status, json) = send_as(&app, &token, "GET", "/admin/products", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "admin_credentials_required")
    );
    let (status, json) = send_as(&app, &token, "GET", "/storefront/store", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "publishable_key_required")
    );
    let (status, json) =
        send_signed_in(&app, &pk, &app.admin_key, "GET", "/storefront/store", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );
    // Any storefront route takes the session beside the key: the shopper is the actor.
    let (status, _) = send_signed_in(&app, &pk, &token, "GET", "/storefront/store", None).await;
    assert_eq!(status, StatusCode::OK);

    // Signing out everywhere ends every session.
    let (status, _) = send_signed_in(
        &app,
        &pk,
        &token,
        "POST",
        "/storefront/account/logout-all",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, json) = send_signed_in(&app, &pk, &token, "GET", "/storefront/store", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );
}

#[tokio::test]
async fn asking_for_a_code_answers_the_same_for_any_address_and_is_rate_limited() {
    let app = app().await;
    let pk = publishable(&app).await;
    // Not an email: 202, nothing sent, nobody created.
    ask(&app, &pk, "not an email").await;
    assert_eq!(emails_sent(&app), 0);
    let (_, customers) = send(&app, "GET", "/admin/customers", None).await;
    assert_eq!(customers["items"].as_array().unwrap().len(), 0);
    // Asking twice within a minute sends one email.
    ask(&app, &pk, "nina@example.com").await;
    ask(&app, &pk, "nina@example.com").await;
    assert_eq!(emails_sent(&app), 1);
    // Without a key, nobody may ask at all.
    let body = json!({ "email": "nina@example.com" }).to_string();
    let (status, json) = send_with(&app, &[], "POST", "/storefront/auth/code", Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "unauthenticated")
    );
    // The fourth and fifth from this address are the last within the limit (5 a minute).
    ask(&app, &pk, "ivo@example.com").await;
    ask(&app, &pk, "ana@example.com").await;
    let (status, json) = send_as(&app, &pk, "POST", "/storefront/auth/code", Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    );
}

// Six digits are few: guesses must stay slow from any one address, and at any one email.
#[tokio::test]
async fn guessing_login_codes_from_one_address_is_limited() {
    let app = app().await;
    let pk = publishable(&app).await;
    let (burst, _) = hypha_http::rate_limit::Rule::CustomerLoginIp.quota(&Default::default());
    for n in 0..burst {
        // Different emails, so only the per-address limit can trip.
        let (status, json) =
            login(&app, &pk, &format!("guess{n}@example.com"), "000000", None).await;
        assert_eq!(
            (status, code(&json)),
            (StatusCode::UNAUTHORIZED, "invalid_code")
        );
    }
    let (status, json) = login(&app, &pk, "again@example.com", "000000", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    );
}

#[tokio::test]
async fn guessing_one_emails_code_is_limited_per_address() {
    let app = app_behind_proxy().await;
    let pk = publishable(&app).await;
    let (burst, _) =
        hypha_http::rate_limit::Rule::CustomerLoginEmailFromIp.quota(&Default::default());
    let body = json!({ "email": "nina@example.com", "code": "000000" }).to_string();
    let path = "/storefront/auth/login";
    let attacker = [
        ("x-publishable-key", pk.as_str()),
        ("x-forwarded-for", "203.0.113.66"),
    ];
    for _ in 0..burst {
        let (status, _) = send_with(&app, &attacker, "POST", path, Some(&body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, json) = send_with(&app, &attacker, "POST", path, Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    );
    // From elsewhere, the same email is only a wrong code.
    let shopper = [
        ("x-publishable-key", pk.as_str()),
        ("x-forwarded-for", "198.51.100.7"),
    ];
    let (status, json) = send_with(&app, &shopper, "POST", path, Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "invalid_code")
    );
}

// ---------- the account ----------

/// Signs `email` in and answers (the publishable key, the session token).
pub(super) async fn signed_in(app: &TestApp, email: &str) -> (String, String) {
    let pk = publishable(app).await;
    ask(app, &pk, email).await;
    let digits = last_code(app);
    let (status, json) = login(app, &pk, email, &digits, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    (pk, json["token"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn the_account_shows_and_changes_the_customers_own_record() {
    let app = app().await;
    // A field the merchant shows storefronts, a personal one, and one kept to the admin.
    for (key, storefront, personal) in [
        ("tier", "read", false),
        ("notes", "read", true),
        ("risk", "none", false),
    ] {
        let body = json!({"owner_type": "customer", "key": key, "type": "text", "name": key,
            "personal": personal, "access": {"admin": "read_write", "storefront": storefront}});
        let (status, created) = send(
            &app,
            "POST",
            "/admin/custom-field-definitions",
            Some(&body.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
    }
    let (pk, token) = signed_in(&app, "nina@example.com").await;
    let (status, account) =
        send_signed_in(&app, &pk, &token, "GET", "/storefront/account", None).await;
    assert_eq!(status, StatusCode::OK, "{account}");
    assert_eq!(account["email"], "nina@example.com");
    assert_eq!(account["first_name"], Value::Null);
    assert_eq!(account["email_marketing"]["state"], "not_subscribed");
    assert!(account.get("erasure_due_at").is_none());
    assert!(
        account.get("redacted").is_none(),
        "the person sees everything"
    );
    let id = account["id"].as_str().unwrap().to_string();

    // Staff fill the fields; the account shows the storefront-readable ones, personal too.
    let fields = json!({"custom_fields": {"custom.tier": "gold", "custom.notes": "likes blue", "custom.risk": "high"}});
    let (status, json) = send(
        &app,
        "PATCH",
        &format!("/admin/customers/{id}/custom-fields"),
        Some(&fields.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (_, account) = send_signed_in(&app, &pk, &token, "GET", "/storefront/account", None).await;
    assert_eq!(
        account["custom_fields"],
        json!({"custom.tier": "gold", "custom.notes": "likes blue"})
    );

    // Names and phone change; the email can't be sent.
    let body = r#"{"first_name":"Nina","last_name":"Kos","phone":"+385 91 000 0000"}"#;
    let (status, account) = send_signed_in(
        &app,
        &pk,
        &token,
        "PATCH",
        "/storefront/account",
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    assert_eq!(account["first_name"], "Nina");
    assert_eq!(account["phone"], "+385 91 000 0000");
    let (status, json) = send_signed_in(
        &app,
        &pk,
        &token,
        "PATCH",
        "/storefront/account",
        Some(r#"{"email":"x@example.com"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body")
    );
    let (_, admin) = send(&app, "GET", &format!("/admin/customers/{id}"), None).await;
    assert_eq!(admin["first_name"], "Nina", "the admin sees the change");

    // The address book.
    let address = json!({"last_name": "Kos", "line1": "Ilica 1", "city": "Zagreb", "country": "HR", "default_shipping": true});
    let (status, account) = send_signed_in(
        &app,
        &pk,
        &token,
        "POST",
        "/storefront/account/addresses",
        Some(&address.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{account}");
    let address_id = account["addresses"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(account["default_shipping_address"], address_id);
    let changed =
        json!({"last_name": "Kos", "line1": "Ilica 2", "city": "Zagreb", "country": "HR"});
    let (status, account) = send_signed_in(
        &app,
        &pk,
        &token,
        "PUT",
        &format!("/storefront/account/addresses/{address_id}"),
        Some(&changed.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    assert_eq!(account["addresses"][0]["line1"], "Ilica 2");
    assert_eq!(account["default_shipping_address"], Value::Null);
    let (status, account) = send_signed_in(
        &app,
        &pk,
        &token,
        "DELETE",
        &format!("/storefront/account/addresses/{address_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    assert_eq!(account["addresses"], json!([]));

    // Consent from the person: source `storefront`, now, single opt-in.
    let (status, json) = send_signed_in(
        &app,
        &pk,
        &token,
        "PUT",
        "/storefront/account/marketing/email",
        Some(r#"{"state":"subscribed"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["applied"], true);
    assert_eq!(json["account"]["email_marketing"]["state"], "subscribed");
    assert_eq!(json["account"]["email_marketing"]["source"], "storefront");
    assert_eq!(json["account"]["email_marketing"]["opt_in_level"], "single");
    let (status, json) = send_signed_in(
        &app,
        &pk,
        &token,
        "PUT",
        "/storefront/account/marketing/fax",
        Some(r#"{"state":"subscribed"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "route_not_found")
    );

    // Without the session, nothing.
    let (status, json) = send_as(&app, &pk, "GET", "/storefront/account", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "account_required")
    );
}

#[tokio::test]
async fn the_account_lists_its_own_orders_exports_its_data_and_asks_for_erasure() {
    let app = app().await;
    // ana@example.hr placed an order as a guest; signing in makes it hers to see.
    let (order, _, _) = placed_order_for_three_mugs(&app).await;
    let (pk, token) = signed_in(&app, "ana@example.hr").await;
    let (status, page) =
        send_signed_in(&app, &pk, &token, "GET", "/storefront/account/orders", None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["id"], order);
    assert_eq!(page["has_more"], false);
    let (status, json) = send_signed_in(
        &app,
        &pk,
        &token,
        "GET",
        "/storefront/account/orders?cancelled=true",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::BAD_REQUEST, "invalid_query")
    );

    let (status, placed) = send_signed_in(
        &app,
        &pk,
        &token,
        "GET",
        &format!("/storefront/account/orders/{order}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{placed}");
    assert_eq!(placed["id"], order);
    assert!(placed["access_token"].is_string());
    assert!(placed["payment"].is_object(), "{placed}");

    // Someone else's order reads as missing, and their account lists none.
    let (pk2, other) = signed_in(&app, "ivo@example.com").await;
    let (status, json) = send_signed_in(
        &app,
        &pk2,
        &other,
        "GET",
        &format!("/storefront/account/orders/{order}"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "order_not_found")
    );
    let (_, page) = send_signed_in(
        &app,
        &pk2,
        &other,
        "GET",
        "/storefront/account/orders",
        None,
    )
    .await;
    assert_eq!(page["items"], json!([]));

    // The export: the account, the order, its payment, the emails about it.
    let (status, export) =
        send_signed_in(&app, &pk, &token, "GET", "/storefront/account/export", None).await;
    assert_eq!(status, StatusCode::OK, "{export}");
    assert_eq!(export["account"]["email"], "ana@example.hr");
    assert_eq!(export["orders"][0]["id"], order);
    assert_eq!(export["payments"].as_array().unwrap().len(), 1);
    assert!(export["emails"].is_array());

    // Erasure from the account: requested with the admin's window, confirmed by email,
    // cancellable until then.
    let before = emails_sent(&app);
    let (status, account) = send_signed_in(
        &app,
        &pk,
        &token,
        "POST",
        "/storefront/account/erasure",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    assert!(account["erasure_due_at"].is_string());
    assert_eq!(emails_sent(&app), before + 1);
    let email = app.emails.lock().unwrap().last().cloned().unwrap();
    assert_eq!(email.to.as_str(), "ana@example.hr");
    assert!(email.subject.contains("erase"), "{}", email.subject);
    let (status, json) = send_signed_in(
        &app,
        &pk,
        &token,
        "POST",
        "/storefront/account/erasure",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "erasure_already_requested")
    );
    let id = account["id"].as_str().unwrap().to_string();
    let (_, admin) = send(&app, "GET", &format!("/admin/customers/{id}"), None).await;
    assert_eq!(admin["erasure_due_at"], account["erasure_due_at"]);
    let (status, account) = send_signed_in(
        &app,
        &pk,
        &token,
        "DELETE",
        "/storefront/account/erasure",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    assert!(account.get("erasure_due_at").is_none());
    let (status, json) = send_signed_in(
        &app,
        &pk,
        &token,
        "DELETE",
        "/storefront/account/erasure",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "erasure_not_requested")
    );
}

// ---------- the cart and the admin ----------

#[tokio::test]
async fn a_signed_in_shoppers_cart_is_theirs_from_the_start_and_keeps_their_email() {
    let app = app().await;
    shop_selling_a_mug(&app).await;
    let (pk, token) = signed_in(&app, "nina@example.com").await;
    let (status, cart) = send_signed_in(
        &app,
        &pk,
        &token,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{cart}");
    assert_eq!(cart["email"], "nina@example.com");
    assert!(cart["customer"].is_string());
    let path = format!("/storefront/carts/{}", cart["id"].as_str().unwrap());

    // The email is theirs: another address is refused, the same one is fine.
    let (status, json) = send_signed_in(
        &app,
        &pk,
        &token,
        "PUT",
        &format!("{path}/email"),
        Some(r#"{"email":"other@example.com"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "cart_email_fixed")
    );
    let (status, _) = send_signed_in(
        &app,
        &pk,
        &token,
        "PUT",
        &format!("{path}/email"),
        Some(r#"{"email":"Nina@example.com"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A guest's cart is nobody's.
    let (_, guest) = send_as(
        &app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    assert_eq!(guest["customer"], Value::Null);
}

#[tokio::test]
async fn the_admin_sees_the_account_and_signs_it_out_everywhere_and_deleting_the_customer_ends_it()
{
    let app = app().await;
    let (pk, token) = signed_in(&app, "nina@example.com").await;
    let (_, account) = send_signed_in(&app, &pk, &token, "GET", "/storefront/account", None).await;
    let id = account["id"].as_str().unwrap().to_string();

    let (_, admin) = send(&app, "GET", &format!("/admin/customers/{id}"), None).await;
    assert_eq!(admin["account"]["active_sessions"], 1, "{admin}");
    assert!(admin["account"]["last_active_at"].is_string());
    let (_, guest) = send(
        &app,
        "POST",
        "/admin/customers",
        Some(r#"{"email":"guest@example.com"}"#),
    )
    .await;
    assert_eq!(guest["account"], Value::Null, "never signed in");

    let (status, _) = send(
        &app,
        "POST",
        &format!("/admin/customers/{id}/sessions/revoke"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, json) =
        send_signed_in(&app, &pk, &token, "GET", "/storefront/account", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );
    let (_, admin) = send(&app, "GET", &format!("/admin/customers/{id}"), None).await;
    assert_eq!(admin["account"]["active_sessions"], 0);
    assert_eq!(admin["account"]["last_active_at"], Value::Null);
    // A customer without an account: nothing to do, still 204.
    let guest_id = guest["id"].as_str().unwrap();
    let (status, _) = send(
        &app,
        "POST",
        &format!("/admin/customers/{guest_id}/sessions/revoke"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Signing in again works; deleting the customer takes the account with it.
    let (_, token) = signed_in(&app, "nina@example.com").await;
    let (status, _) = send(&app, "DELETE", &format!("/admin/customers/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, json) =
        send_signed_in(&app, &pk, &token, "GET", "/storefront/account", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );
    // The address is free: signing in makes a new customer.
    let (_, token) = signed_in(&app, "nina@example.com").await;
    let (status, account) =
        send_signed_in(&app, &pk, &token, "GET", "/storefront/account", None).await;
    assert_eq!(status, StatusCode::OK, "{account}");
    assert_ne!(account["id"], id);
}
