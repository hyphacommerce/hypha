//! Route-level tests: the in-memory router driven with `oneshot`, no socket. They check
//! what only the HTTP layer decides (status codes, error codes, extractors); domain
//! rules are tested in their crates and end to end in `scripts/e2e.sh`.

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::CONTENT_TYPE},
};
use hypha_core::Ctx;
use hypha_stores::ensure_single_store;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::memory_router;
use crate::state::{AppState, MemoryStorage, Storage, UuidV7Ids};

async fn app() -> Router {
    let storage = MemoryStorage::default();
    let store = ensure_single_store(
        &Ctx::system(),
        &storage.directory().await.unwrap(),
        &UuidV7Ids,
        "Test",
    )
    .await
    .unwrap();
    let state = Arc::new(AppState {
        storage,
        ids: UuidV7Ids,
        store: store.id(),
    });
    memory_router().with_state(state)
}

/// Sends one request and returns the status and the JSON body (`null` if empty).
async fn send(app: &Router, method: &str, uri: &str, body: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if body.is_some() {
        request = request.header(CONTENT_TYPE, "application/json");
    }
    let request = request
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
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
        .body(Body::from("{}"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
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
