//! Route-level tests: the in-memory router driven with `oneshot`, no socket. They check
//! what only the HTTP layer decides (status codes, error codes, extractors); domain
//! rules are tested in their crates and end to end in `scripts/e2e.sh`.

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{
        Request, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE},
    },
};
use hypha_auth::{ApiKeyKind, Argon2id, CreateApiKey, create_api_key, totp};
use hypha_core::{Ctx, Scope, Timestamp};
use hypha_stores::ensure_single_store;
use hypha_users::{NewOwner, bootstrap_owner};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::memory_router;
use crate::state::{AppState, HostEnv, MemoryStorage, Storage, SystemClock, UuidV7Ids};

const OWNER_EMAIL: &str = "owner@example.com";
const OWNER_PASSWORD: &str = "owner password";

/// The in-memory app with its owner (`OWNER_EMAIL`), and a secret key with every scope
/// that `send` uses unless a test picks other credentials.
struct TestApp {
    router: Router,
    admin_key: String,
}

async fn app() -> TestApp {
    let storage = MemoryStorage::default();
    let store = ensure_single_store(
        &Ctx::system(),
        &storage.directory().await.unwrap(),
        &UuidV7Ids,
        "Test",
    )
    .await
    .unwrap();
    let env = HostEnv::new([7; 32], None).with_hasher(Argon2id::new(8, 1, 1));
    let state = Arc::new(AppState {
        storage,
        ids: UuidV7Ids,
        clock: SystemClock,
        env,
        store: store.id(),
    });
    let ports = state.ports().await.unwrap();
    let owner = NewOwner {
        email: OWNER_EMAIL.to_string(),
        name: "Owner".to_string(),
        password: OWNER_PASSWORD.to_string(),
    };
    bootstrap_owner(&Ctx::system(), &ports, &state.env, owner)
        .await
        .unwrap();
    let input = CreateApiKey {
        kind: ApiKeyKind::Secret,
        name: "Tests".to_string(),
        scopes: Some(Scope::ALL.to_vec()),
    };
    let (_, token) = create_api_key(&Ctx::system(), &ports.api_keys, &state.env, input)
        .await
        .unwrap();
    TestApp {
        router: memory_router().with_state(state.clone()),
        admin_key: token.expose().to_string(),
    }
}

/// Sends one request with the admin key and returns the status and the JSON body
/// (`null` if empty).
async fn send(app: &TestApp, method: &str, uri: &str, body: Option<&str>) -> (StatusCode, Value) {
    let auth = format!("Bearer {}", app.admin_key);
    send_with(app, &[(AUTHORIZATION.as_str(), &auth)], method, uri, body).await
}

/// Like `send`, as `token` (a session or key) instead of the admin key.
async fn send_as(
    app: &TestApp,
    token: &str,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let auth = format!("Bearer {token}");
    send_with(app, &[(AUTHORIZATION.as_str(), &auth)], method, uri, body).await
}

/// Sends one request with exactly `headers` (no credentials unless among them).
async fn send_with(
    app: &TestApp,
    headers: &[(&str, &str)],
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if body.is_some() {
        request = request.header(CONTENT_TYPE, "application/json");
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

fn code(body: &Value) -> &str {
    body["code"].as_str().unwrap_or_default()
}

#[tokio::test]
async fn health_is_ok() {
    let app = app().await;
    assert_eq!(
        send(&app, "GET", "/health", None).await,
        (StatusCode::OK, json!({ "status": "ok" }))
    );
}

#[tokio::test]
async fn unknown_routes_and_methods_get_json_errors() {
    let app = app().await;
    let (status, body) = send(&app, "GET", "/nope", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "route_not_found")
    );
    let (status, body) = send(&app, "DELETE", "/products", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed")
    );
}

#[tokio::test]
async fn bad_bodies_are_rejected_with_codes() {
    let app = app().await;

    let (status, body) = send(&app, "POST", "/products", Some("{")).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "malformed_json")
    );

    let request = Request::builder()
        .method("POST")
        .uri("/products")
        .header(AUTHORIZATION, format!("Bearer {}", app.admin_key))
        .body(Body::from("{}"))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let typo =
        r#"{"handle":"mug","title":"Mug","price":{"amount":1,"currency":"EUR"},"titel":"x"}"#;
    let (status, body) = send(&app, "POST", "/products", Some(typo)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body")
    );
}

#[tokio::test]
async fn a_created_product_can_be_fetched_and_an_unknown_one_is_404() {
    let app = app().await;
    let (status, created) = send(
        &app,
        "POST",
        "/products",
        Some(r#"{"handle":"mug","title":"Mug","price":{"amount":1000,"currency":"EUR"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();

    let (status, fetched) = send(&app, "GET", &format!("/products/{id}"), None).await;
    assert_eq!((status, &fetched), (StatusCode::OK, &created));

    let unknown = "/products/prod_00000000000000000000000000000063";
    let (status, body) = send(&app, "GET", unknown, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "product_not_found")
    );

    let (status, body) = send(
        &app,
        "GET",
        "/products/cart_00000000000000000000000000000063",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_path")
    );
}

#[tokio::test]
async fn the_store_is_the_one_created_at_startup() {
    let app = app().await;
    let (status, body) = send(&app, "GET", "/store", None).await;
    assert_eq!(
        (status, body["name"].as_str()),
        (StatusCode::OK, Some("Test"))
    );
}

// ---------- authentication (design note 0008) ----------

/// Logs the owner in; returns the session token.
async fn owner_session(app: &TestApp) -> String {
    let body = json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }).to_string();
    let (status, login) = send_with(app, &[], "POST", "/auth/login", Some(&body)).await;
    assert_eq!(
        (status, login["status"].as_str()),
        (StatusCode::OK, Some("authenticated"))
    );
    login["session"]["token"].as_str().unwrap().to_string()
}

/// Creates a key as the admin; returns its token.
async fn key(app: &TestApp, body: Value) -> String {
    let (status, created) = send(app, "POST", "/api-keys", Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn no_credentials_is_401_with_a_bearer_challenge() {
    let app = app().await;
    let request = Request::builder()
        .uri("/store")
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()[WWW_AUTHENTICATE], "Bearer");
    let (_, body) = send_with(&app, &[], "GET", "/store", None).await;
    assert_eq!(code(&body), "unauthenticated");
    // Health checks need none.
    assert_eq!(
        send_with(&app, &[], "GET", "/health", None).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn bad_credentials_are_invalid_token() {
    let app = app().await;
    let session = owner_session(&app).await;
    for auth in [
        "Bearer nope".to_string(),
        format!("Basic {session}"),
        format!("Bearer {}", session.replace("hypha_sess_", "hypha_rst_")),
        format!("Bearer hypha_sk_{}", "0".repeat(64)),
    ] {
        let (status, body) =
            send_with(&app, &[("authorization", &auth)], "GET", "/store", None).await;
        assert_eq!(
            (status, code(&body)),
            (StatusCode::UNAUTHORIZED, "invalid_token"),
            "{auth}"
        );
    }
    // A secret key is not a publishable one.
    let secret = app.admin_key.clone();
    let (status, body) = send_with(
        &app,
        &[("x-publishable-key", &secret)],
        "GET",
        "/store",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );
}

#[tokio::test]
async fn the_owner_logs_in_sees_themselves_and_logs_out() {
    let app = app().await;
    let session = owner_session(&app).await;
    let (status, me) = send_as(&app, &session, "GET", "/me", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["user"]["email"], OWNER_EMAIL);
    assert_eq!(me["role"]["name"], "Owner");
    assert_eq!(me["mfa_enabled"], false);
    assert_eq!(
        me["scopes"].as_array().map(Vec::len),
        Some(Scope::ALL.len())
    );

    assert_eq!(
        send_as(&app, &session, "POST", "/auth/logout", None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (status, body) = send_as(&app, &session, "GET", "/me", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );
}

#[tokio::test]
async fn a_wrong_password_is_401() {
    let app = app().await;
    let body = json!({ "email": OWNER_EMAIL, "password": "nope nope" }).to_string();
    let (status, body) = send_with(&app, &[], "POST", "/auth/login", Some(&body)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "invalid_credentials")
    );
}

#[tokio::test]
async fn me_routes_need_a_session_not_a_key() {
    let app = app().await;
    let (status, body) = send(&app, "GET", "/me", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "session_required")
    );
}

#[tokio::test]
async fn a_key_has_only_its_scopes() {
    let app = app().await;
    let reader = key(
        &app,
        json!({ "kind": "secret", "name": "Reader", "scopes": ["products:read"] }),
    )
    .await;
    let product = r#"{"handle":"mug","title":"Mug","price":{"amount":1000,"currency":"EUR"}}"#;
    let (status, body) = send_as(&app, &reader, "POST", "/products", Some(product)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
}

#[tokio::test]
async fn a_publishable_key_works_in_its_own_header_with_storefront_scopes() {
    let app = app().await;
    let (status, created) = send(
        &app,
        "POST",
        "/api-keys",
        Some(r#"{"kind":"publishable","name":"Storefront"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(
        created["redacted"]
            .as_str()
            .unwrap()
            .starts_with("hypha_pk_…")
    );
    let token = created["token"].as_str().unwrap();
    let header = [("x-publishable-key", token)];
    assert_eq!(
        send_with(&app, &header, "GET", "/store", None).await.0,
        StatusCode::OK
    );
    let (status, body) = send_with(
        &app,
        &header,
        "POST",
        "/customers",
        Some(r#"{"email":"a@b.co"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
}

#[tokio::test]
async fn a_revoked_key_stops_working() {
    let app = app().await;
    let (_, created) = send(
        &app,
        "POST",
        "/api-keys",
        Some(r#"{"kind":"secret","name":"Temp","scopes":["stores:read"]}"#),
    )
    .await;
    let token = created["token"].as_str().unwrap();
    let id = created["id"].as_str().unwrap();
    assert_eq!(
        send_as(&app, token, "GET", "/store", None).await.0,
        StatusCode::OK
    );
    let (status, revoked) = send(&app, "POST", &format!("/api-keys/{id}/revoke"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(revoked["revoked_at"].is_string());
    assert_eq!(
        send_as(&app, token, "GET", "/store", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, body) = send(&app, "POST", &format!("/api-keys/{id}/revoke"), None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "api_key_revoked")
    );
}

#[tokio::test]
async fn nobody_creates_a_key_beyond_their_scopes() {
    let app = app().await;
    let limited = key(
        &app,
        json!({ "kind": "secret", "name": "Keys", "scopes": ["api_keys:write"] }),
    )
    .await;
    let body = json!({ "kind": "secret", "name": "More", "scopes": ["orders:read"] }).to_string();
    let (status, body) = send_as(&app, &limited, "POST", "/api-keys", Some(&body)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "not_allowed_to_grant")
    );
}

/// The current TOTP code for a base32 `secret`.
fn totp_code(secret: &str) -> String {
    let secret = totp::from_base32(secret).unwrap();
    let now: Timestamp = hypha_core::Clock::now(&SystemClock);
    format!("{:06}", totp::code_at(&secret, totp::step_at(now)))
}

#[tokio::test]
async fn two_factor_setup_and_login_over_http() {
    let app = app().await;
    let session = owner_session(&app).await;
    let (status, body) = send_as(
        &app,
        &session,
        "POST",
        "/me/mfa/totp",
        Some(r#"{"password":"nope"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "wrong_password")
    );
    let password = json!({ "password": OWNER_PASSWORD }).to_string();
    let (status, setup) = send_as(&app, &session, "POST", "/me/mfa/totp", Some(&password)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(
        setup["uri"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/Test:owner%40example.com?")
    );

    // A wrong code from someone logged in is 422, not 401.
    let (status, body) = send_as(
        &app,
        &session,
        "POST",
        "/me/mfa/totp/confirm",
        Some(r#"{"code":"000000"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_mfa_code")
    );

    let confirm = json!({ "code": totp_code(setup["secret"].as_str().unwrap()) }).to_string();
    let (status, codes) = send_as(
        &app,
        &session,
        "POST",
        "/me/mfa/totp/confirm",
        Some(&confirm),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let recovery = codes["recovery_codes"][0].as_str().unwrap().to_string();

    let body = json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }).to_string();
    let (status, login) = send_with(&app, &[], "POST", "/auth/login", Some(&body)).await;
    assert_eq!(
        (status, login["status"].as_str()),
        (StatusCode::OK, Some("mfa_required"))
    );
    let challenge = login["challenge"]["token"].as_str().unwrap();
    assert_eq!(
        login["challenge"]["methods"],
        json!(["totp", "recovery_code"])
    );
    // The challenge is no session.
    assert_eq!(
        send_as(&app, challenge, "GET", "/me", None).await.0,
        StatusCode::UNAUTHORIZED
    );

    let wrong = json!({ "challenge": challenge, "code": "000000" }).to_string();
    let (status, body) = send_with(&app, &[], "POST", "/auth/login/mfa", Some(&wrong)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "invalid_mfa_code")
    );

    // The confirm used this step's TOTP code, so finish with a recovery code.
    let right = json!({ "challenge": challenge, "code": recovery }).to_string();
    let (status, done) = send_with(&app, &[], "POST", "/auth/login/mfa", Some(&right)).await;
    assert_eq!(
        (status, done["status"].as_str()),
        (StatusCode::OK, Some("authenticated"))
    );
    let new_session = done["session"]["token"].as_str().unwrap();
    let (_, me) = send_as(&app, new_session, "GET", "/me", None).await;
    assert_eq!(me["mfa_enabled"], true);
}

#[tokio::test]
async fn roles_and_invites_over_http() {
    let app = app().await;
    let (status, role) = send(
        &app,
        "POST",
        "/roles",
        Some(r#"{"name":"Support","scopes":["orders:write","customers:read"]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(role["scopes"], json!(["customers:read", "orders:write"]));
    let id = role["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        "POST",
        "/roles",
        Some(r#"{"name":"support","scopes":["orders:read"]}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "role_name_taken")
    );

    let invite = json!({ "email": "bob@example.com", "role": id }).to_string();
    let (status, invited) = send(&app, "POST", "/invites", Some(&invite)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(invited["email"], "bob@example.com");
    assert!(invited.get("token").is_none());

    let unknown_role = "role_00000000000000000000000000000063";
    let (status, body) = send(&app, "GET", &format!("/roles/{unknown_role}"), None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "role_not_found")
    );
    let invite = json!({ "email": "eve@example.com", "role": unknown_role }).to_string();
    let (status, body) = send(&app, "POST", "/invites", Some(&invite)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "role_not_found")
    );

    let accept =
        json!({ "token": "hypha_inv_nope", "name": "Bob", "password": "bob password" }).to_string();
    let (status, body) = send_with(&app, &[], "POST", "/invites/accept", Some(&accept)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );
}

#[tokio::test]
async fn a_reset_request_is_202_whoever_asks() {
    let app = app().await;
    for email in [OWNER_EMAIL, "nobody@example.com", "not an email"] {
        let body = json!({ "email": email }).to_string();
        assert_eq!(
            send_with(&app, &[], "POST", "/auth/password-reset", Some(&body))
                .await
                .0,
            StatusCode::ACCEPTED
        );
    }
}

#[tokio::test]
async fn the_last_owner_cannot_be_disabled_over_http() {
    let app = app().await;
    let session = owner_session(&app).await;
    let (_, me) = send_as(&app, &session, "GET", "/me", None).await;
    let id = me["user"]["id"].as_str().unwrap();
    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/users/{id}"),
        Some(r#"{"status":"disabled"}"#),
    )
    .await;
    assert_eq!((status, code(&body)), (StatusCode::CONFLICT, "last_owner"));
}
