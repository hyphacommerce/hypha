//! Customer accounts (design note 0034): a code emailed, a session beside the publishable
//! key, signing out.

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
async fn send_signed_in(
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
    assert!(email.subject.contains(&digits), "{}", email.subject);

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
    // The customer was created at the first request (AC2), and the admin sees them.
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
