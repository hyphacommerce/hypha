//! Route-level tests: the router on Postgres driven with `oneshot`, no socket. They check
//! what only the HTTP layer decides (status codes, error codes, extractors); domain
//! rules are tested in their crates and end to end in `scripts/e2e.sh`.
//!
//! They need Postgres at `DATABASE_URL` (`.cargo/config.toml` points it at the database
//! `docker compose up -d` starts).
//! Each test gets a schema of its own, named after it, so they run in parallel and a
//! rerun starts clean.

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{
        Request, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE},
    },
};
use hypha_auth::{ApiKeyKind, Argon2id, CreateApiKey, HasApiKeys, create_api_key, totp};
use hypha_core::{Ctx, Scope, Timestamp};
use hypha_stores::ensure_single_store;
use hypha_users::{NewOwner, bootstrap_owner};
use serde_json::{Value, json};
use tower::ServiceExt;

use std::net::SocketAddr;

use axum::extract::connect_info::MockConnectInfo;

use super::postgres_router;
use crate::client_ip::TrustedProxies;
use crate::media::Media;
use crate::state::{AppState, HostEnv, PgStorage, Storage, SystemClock, UuidV7Ids};

const OWNER_EMAIL: &str = "owner@example.com";
const OWNER_PASSWORD: &str = "owner password";

/// The app on a fresh schema, with its owner (`OWNER_EMAIL`), and a secret key with every scope
/// that `send` uses unless a test picks other credentials.
struct TestApp {
    router: Router,
    admin_key: String,
}

/// The app without file storage: most routes don't need it, so their tests don't need
/// the S3 server.
async fn app() -> TestApp {
    app_with(TrustedProxies::default()).await
}

async fn app_with(trusted_proxies: TrustedProxies) -> TestApp {
    app_from(trusted_proxies, None).await
}

/// The app with file storage (the compose S3 server), for the media routes.
async fn app_with_media() -> TestApp {
    app_from(TrustedProxies::default(), Some(test_media().await)).await
}

/// Requests in these tests come from this peer (`MockConnectInfo`), as if over a socket.
const PEER: ([u8; 4], u16) = ([127, 0, 0, 1], 4000);

/// A schema of its own for the running test, emptied, and the URL that connects to it.
/// libtest runs each test on a thread named after it, so the name is stable across runs
/// and unique within one.
async fn fresh_schema() -> String {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL (see .cargo/config.toml)");
    let test = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_string();
    // Postgres names are at most 63 bytes: a readable tail of the test's name, and a hash
    // of all of it so two long names with the same tail stay apart.
    let hash = test.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    let tail: String = test
        .rsplit("::")
        .next()
        .unwrap_or(&test)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(40)
        .collect();
    let schema = format!("rt_{hash:016x}_{tail}").to_lowercase();

    let config = hypha_store_postgres::database_config(&url).expect("DATABASE_URL");
    let (client, connection) = config
        .connect(tokio_postgres::NoTls)
        .await
        .unwrap_or_else(|e| {
            panic!("route tests need Postgres at {url} (`docker compose up -d`): {e}")
        });
    tokio::spawn(connection);
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema};"
        ))
        .await
        .expect("reset the test's schema");
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}options=-csearch_path%3D{schema}")
}

/// File storage from `HYPHA_S3_*` (the compose server by default), bucket created.
async fn test_media() -> Media {
    let media = Media::from_env()
        .expect("HYPHA_S3_* (see .cargo/config.toml)")
        .expect("HYPHA_S3_ENDPOINT (see .cargo/config.toml)");
    media
        .blobs
        .ensure_bucket()
        .await
        .unwrap_or_else(|e| panic!("route tests need the S3 server (`docker compose up -d`): {e}"));
    media
}

async fn app_from(trusted_proxies: TrustedProxies, media: Option<Media>) -> TestApp {
    let storage = PgStorage::connect(&fresh_schema().await).await.unwrap();
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
        background: Default::default(),
        limiter: Default::default(),
        trusted_proxies,
        store: store.id(),
        store_name: store.name().to_string(),
        media,
    });
    let ports = state.public_ports().await.unwrap();
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
    let (_, token) = create_api_key(&Ctx::system(), ports.api_keys(), &state.env, input)
        .await
        .unwrap();
    TestApp {
        router: postgres_router()
            .with_state(state.clone())
            .layer(MockConnectInfo(SocketAddr::from(PEER))),
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
    let (status, body) = send(&app, "DELETE", "/admin/products", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed")
    );
}

#[tokio::test]
async fn bad_bodies_are_rejected_with_codes() {
    let app = app().await;

    let (status, body) = send(&app, "POST", "/admin/products", Some("{")).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "malformed_json")
    );

    let request = Request::builder()
        .method("POST")
        .uri("/admin/products")
        .header(AUTHORIZATION, format!("Bearer {}", app.admin_key))
        .body(Body::from("{}"))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let typo =
        r#"{"handle":"mug","title":"Mug","price":{"amount":1,"currency":"EUR"},"titel":"x"}"#;
    let (status, body) = send(&app, "POST", "/admin/products", Some(typo)).await;
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
        "/admin/products",
        Some(r#"{"handle":"mug","title":"Mug","price":{"amount":1000,"currency":"EUR"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();

    let (status, fetched) = send(&app, "GET", &format!("/admin/products/{id}"), None).await;
    assert_eq!((status, &fetched), (StatusCode::OK, &created));

    let unknown = "/admin/products/prod_00000000000000000000000000000063";
    let (status, body) = send(&app, "GET", unknown, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "product_not_found")
    );

    let (status, body) = send(
        &app,
        "GET",
        "/admin/products/cart_00000000000000000000000000000063",
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
    let (status, body) = send(&app, "GET", "/admin/store", None).await;
    assert_eq!(
        (status, body["name"].as_str()),
        (StatusCode::OK, Some("Test"))
    );
}

#[tokio::test]
async fn the_store_can_be_renamed() {
    let app = app().await;
    let (status, body) = send(&app, "PATCH", "/admin/store", Some(r#"{"name":" Lumen "}"#)).await;
    assert_eq!(
        (status, body["name"].as_str()),
        (StatusCode::OK, Some("Lumen"))
    );
    // Absent keeps (JSON Merge Patch); `null` can't clear a required name.
    let (status, body) = send(&app, "PATCH", "/admin/store", Some("{}")).await;
    assert_eq!(
        (status, body["name"].as_str()),
        (StatusCode::OK, Some("Lumen"))
    );
    let (status, body) = send(&app, "PATCH", "/admin/store", Some(r#"{"name":null}"#)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body")
    );
    let (status, body) = send(&app, "PATCH", "/admin/store", Some(r#"{"name":"  "}"#)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "store_name_empty")
    );
    let (_, body) = send(&app, "GET", "/admin/store", None).await;
    assert_eq!(body["name"].as_str(), Some("Lumen"));
}

#[tokio::test]
async fn changing_the_store_needs_stores_write_even_with_nothing_to_change() {
    let app = app().await;
    let (_, created) = send(
        &app,
        "POST",
        "/admin/api-keys",
        Some(r#"{"kind":"secret","name":"Reader","scopes":["stores:read"]}"#),
    )
    .await;
    let token = created["token"].as_str().unwrap();
    for body in [r#"{"name":"Lumen"}"#, "{}"] {
        let (status, json) = send_as(&app, token, "PATCH", "/admin/store", Some(body)).await;
        assert_eq!(
            (status, code(&json)),
            (StatusCode::FORBIDDEN, "missing_scope"),
            "{body}"
        );
    }
}

// ---------- lists (design note 0012) ----------

async fn create_product(app: &TestApp, handle: &str) -> String {
    let body = json!({
        "handle": handle,
        "title": handle,
        "price": { "amount": 1000, "currency": "EUR" },
    });
    let (status, created) = send(app, "POST", "/admin/products", Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().unwrap().to_string()
}

fn ids(page: &Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn products_list_newest_first_in_pages() {
    let app = app().await;
    let a = create_product(&app, "a").await;
    let b = create_product(&app, "b").await;
    let c = create_product(&app, "c").await;

    let (status, first) = send(&app, "GET", "/admin/products?limit=2", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids(&first), [c.as_str(), b.as_str()]);
    assert_eq!(
        (&first["has_more"], &first["next_cursor"]),
        (&json!(true), &json!(b))
    );

    let uri = format!("/admin/products?limit=2&cursor={b}");
    let (_, second) = send(&app, "GET", &uri, None).await;
    assert_eq!(ids(&second), [a.as_str()]);
    assert_eq!(
        (&second["has_more"], &second["next_cursor"]),
        (&json!(false), &Value::Null)
    );

    let uri = format!("/admin/products/{b}/status");
    let (status, _) = send(&app, "PATCH", &uri, Some(r#"{"status":"active"}"#)).await;
    assert_eq!(status, StatusCode::OK);
    let (_, active) = send(&app, "GET", "/admin/products?status=active", None).await;
    assert_eq!(ids(&active), [b.as_str()]);
    let (_, drafts) = send(&app, "GET", "/admin/products?status=draft", None).await;
    assert_eq!(ids(&drafts), [c.as_str(), a.as_str()]);
}

#[tokio::test]
async fn a_listed_product_is_a_summary() {
    let app = app().await;
    let id = create_product(&app, "mug").await;
    let (_, page) = send(&app, "GET", "/admin/products", None).await;
    assert_eq!(
        page["items"][0],
        json!({
            "id": id,
            "handle": "mug",
            "title": "mug",
            "status": "draft",
            "variant_count": 1,
            "price_ranges": [{
                "min": { "amount": 1000, "currency": "EUR" },
                "max": { "amount": 1000, "currency": "EUR" },
            }],
            "featured_media": null,
        })
    );
}

#[tokio::test]
async fn a_product_is_edited_over_http() {
    let app = app().await;
    let id = create_product(&app, "shirt").await;
    create_product(&app, "mug").await;
    let product = format!("/admin/products/{id}");

    let (status, renamed) = send(
        &app,
        "PATCH",
        &product,
        Some(r#"{"title":"Linen shirt","handle":"linen-shirt"}"#),
    )
    .await;
    assert_eq!(
        (status, &renamed["title"], &renamed["handle"]),
        (StatusCode::OK, &json!("Linen shirt"), &json!("linen-shirt"))
    );
    let (status, body) = send(&app, "PATCH", &product, Some(r#"{"handle":"mug"}"#)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "handle_taken")
    );
    let (status, body) = send(&app, "PATCH", &product, Some(r#"{"title":null}"#)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Size (S, M): the first variant becomes S; then add M.
    let options = format!("{product}/options");
    let body = r#"{"options":[{"name":"Size","values":[{"label":"S"},{"label":"M"}]}]}"#;
    let (status, sized) = send(&app, "PUT", &options, Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{sized}");
    assert_eq!(sized["variants"][0]["title"], "S");
    let size = &sized["options"][0];
    let (s, m) = (&size["values"][0]["id"], &size["values"][1]["id"]);
    let first = sized["variants"][0]["id"].as_str().unwrap().to_string();

    let variants = format!("{product}/variants");
    let body = json!({ "sku": "SHIRT-M", "price": { "amount": 1200, "currency": "EUR" }, "selections": [m] });
    let (status, added) = send(&app, "POST", &variants, Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::CREATED, "{added}");
    let second = added["variants"][1]["id"].as_str().unwrap().to_string();
    assert_eq!(added["variants"][1]["title"], "M");
    let (status, body) = send(&app, "POST", &variants, Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "duplicate_option_combination")
    );

    let first_url = format!("{variants}/{first}");
    let (status, body) = send(&app, "PATCH", &first_url, Some(r#"{"sku":"SHIRT-M"}"#)).await;
    assert_eq!((status, code(&body)), (StatusCode::CONFLICT, "sku_taken"));
    let (status, patched) = send(
        &app,
        "PATCH",
        &first_url,
        Some(r#"{"sku":"SHIRT-S","price":{"amount":900,"currency":"EUR"}}"#),
    )
    .await;
    assert_eq!(
        (
            status,
            &patched["variants"][0]["sku"],
            &patched["variants"][0]["price"]["amount"]
        ),
        (StatusCode::OK, &json!("SHIRT-S"), &json!(900))
    );
    let (status, cleared) = send(&app, "PATCH", &first_url, Some(r#"{"sku":null}"#)).await;
    assert_eq!(
        (status, &cleared["variants"][0]["sku"]),
        (StatusCode::OK, &Value::Null)
    );

    // Removing M, which the second variant uses: refused, then named.
    let only_s = json!({ "options": [{ "id": size["id"], "name": "Size", "values": [{ "id": s, "label": "S" }] }] });
    let (status, body) = send(&app, "PUT", &options, Some(&only_s.to_string())).await;
    assert_eq!(
        (status, code(&body), &body["details"]),
        (
            StatusCode::CONFLICT,
            "option_value_in_use",
            &json!({ "variants": [second] })
        )
    );
    let mut with_removal = only_s.clone();
    with_removal["remove_variants"] = json!([second]);
    let (status, body) = send(&app, "PUT", &options, Some(&with_removal.to_string())).await;
    assert_eq!(
        (status, body["variants"].as_array().unwrap().len()),
        (StatusCode::OK, 1)
    );

    let (status, body) = send(&app, "DELETE", &first_url, None).await;
    assert_eq!((status, code(&body)), (StatusCode::CONFLICT, "no_variants"));
    let unknown = format!("{variants}/var_00000000000000000000000000000000");
    let (status, body) = send(&app, "DELETE", &unknown, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "variant_not_found")
    );
}

#[tokio::test]
async fn orders_list_starts_empty() {
    let app = app().await;
    let (status, page) = send(&app, "GET", "/admin/orders", None).await;
    assert_eq!(
        (status, page),
        (
            StatusCode::OK,
            json!({ "items": [], "has_more": false, "next_cursor": null })
        )
    );
}

#[tokio::test]
async fn bad_list_queries_are_rejected_with_codes() {
    let app = app().await;
    for (uri, expected) in [
        (
            "/admin/products?limit=0",
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid_limit"),
        ),
        (
            "/admin/orders?limit=101",
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid_limit"),
        ),
        (
            "/admin/products?limit=ten",
            (StatusCode::BAD_REQUEST, "invalid_query"),
        ),
        (
            "/admin/products?status=Active",
            (StatusCode::BAD_REQUEST, "invalid_query"),
        ),
        (
            "/admin/products?sort=title",
            (StatusCode::BAD_REQUEST, "invalid_query"),
        ),
        (
            "/admin/orders?cursor=prod_00000000000000000000000000000063",
            (StatusCode::BAD_REQUEST, "invalid_cursor"),
        ),
        (
            "/admin/orders?after=x",
            (StatusCode::BAD_REQUEST, "invalid_query"),
        ),
    ] {
        let (status, body) = send(&app, "GET", uri, None).await;
        assert_eq!((status, code(&body)), expected, "{uri}");
    }
    // Credentials are checked before the query is read.
    let (status, body) = send_with(&app, &[], "GET", "/admin/products?limit=0", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "unauthenticated")
    );
}

#[tokio::test]
async fn lists_need_read_scopes() {
    let app = app().await;
    let customers = key(
        &app,
        json!({ "kind": "secret", "name": "Customers", "scopes": ["customers:read"] }),
    )
    .await;
    for uri in ["/admin/products", "/admin/orders"] {
        let (status, body) = send_as(&app, &customers, "GET", uri, None).await;
        assert_eq!(
            (status, code(&body)),
            (StatusCode::FORBIDDEN, "missing_scope"),
            "{uri}"
        );
    }
}

// ---------- media (design note 0013) ----------

/// A 3 × 2 PNG's first bytes (signature and IHDR), padded to `len` bytes.
fn png(len: usize) -> Vec<u8> {
    let mut file = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    file.extend_from_slice(&3u32.to_be_bytes());
    file.extend_from_slice(&2u32.to_be_bytes());
    file.extend_from_slice(&[8, 6, 0, 0, 0]);
    file.resize(len, 0);
    file
}

/// Starts an upload of `len` bytes of PNG; returns the created asset and upload.
async fn start_png_upload(app: &TestApp, len: usize) -> Value {
    let body = json!({ "filename": "Shirt front.png", "mime_type": "image/png", "byte_size": len });
    let (status, created) = send(app, "POST", "/admin/assets", Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created
}

/// Sends `body` with the upload request, as a browser would; the status storage gave.
async fn put_file(upload: &Value, body: Vec<u8>) -> u16 {
    let mut request = reqwest::Client::new()
        .put(upload["url"].as_str().unwrap())
        .body(body);
    for header in upload["headers"].as_array().unwrap() {
        request = request.header(
            header["name"].as_str().unwrap(),
            header["value"].as_str().unwrap(),
        );
    }
    request.send().await.unwrap().status().as_u16()
}

#[tokio::test]
async fn an_upload_goes_straight_to_storage_and_completes_into_a_ready_asset() {
    let app = app_with_media().await;
    let created = start_png_upload(&app, 100).await;
    let asset = &created["asset"];
    let id = asset["id"].as_str().unwrap();
    assert_eq!(
        (&asset["status"], &asset["url"]),
        (&json!("pending"), &Value::Null)
    );
    assert_eq!(created["upload"]["method"], "PUT");

    assert_eq!(put_file(&created["upload"], png(100)).await, 200);
    let (status, ready) = send(&app, "POST", &format!("/admin/assets/{id}/complete"), None).await;
    assert_eq!(status, StatusCode::OK, "{ready}");
    assert_eq!(
        (
            &ready["status"],
            &ready["kind"],
            &ready["width"],
            &ready["height"]
        ),
        (&json!("ready"), &json!("image"), &json!(3), &json!(2))
    );
    // No public media URL in tests: the file comes signed.
    let file = reqwest::get(ready["url"].as_str().unwrap()).await.unwrap();
    assert_eq!(file.bytes().await.unwrap().to_vec(), png(100));

    let (_, page) = send(&app, "GET", "/admin/assets?kind=image", None).await;
    assert_eq!(ids(&page), [id]);
    let (_, videos) = send(&app, "GET", "/admin/assets?kind=video", None).await;
    assert_eq!(ids(&videos), Vec::<&str>::new());

    let uri = format!("/admin/assets/{id}");
    let (status, patched) = send(&app, "PATCH", &uri, Some(r#"{"alt":"A blue shirt"}"#)).await;
    assert_eq!(
        (status, &patched["alt"], &patched["title"]),
        (StatusCode::OK, &json!("A blue shirt"), &Value::Null)
    );
    let (status, _) = send(&app, "DELETE", &uri, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(&app, "GET", &uri, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "asset_not_found")
    );
}

/// Uploads a PNG and completes it; returns the asset's ID.
async fn ready_png(app: &TestApp) -> String {
    let created = start_png_upload(app, 100).await;
    let id = created["asset"]["id"].as_str().unwrap().to_string();
    assert_eq!(put_file(&created["upload"], png(100)).await, 200);
    let (status, _) = send(app, "POST", &format!("/admin/assets/{id}/complete"), None).await;
    assert_eq!(status, StatusCode::OK);
    id
}

#[tokio::test]
async fn media_attach_to_products_and_guard_their_assets() {
    let app = app_with_media().await;
    let product = create_product(&app, "shirt").await;
    let (first, second) = (ready_png(&app).await, ready_png(&app).await);
    let media = format!("/admin/products/{product}/media");

    let body = json!({ "assets": [second, first] });
    let (status, attached) = send(&app, "PUT", &media, Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{attached}");
    let items = attached["media"].as_array().unwrap();
    assert_eq!(
        (&items[0]["asset_id"], &items[1]["asset_id"]),
        (&json!(second), &json!(first))
    );
    assert_eq!(
        (&items[0]["kind"], &items[0]["width"], &items[0]["height"]),
        (&json!("image"), &json!(3), &json!(2))
    );
    let file = reqwest::get(items[0]["url"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(file.bytes().await.unwrap().to_vec(), png(100));

    let (_, page) = send(&app, "GET", "/admin/products", None).await;
    assert_eq!(
        page["items"][0]["featured_media"]["asset_id"],
        json!(second)
    );

    let variant = attached["variants"][0]["id"].as_str().unwrap();
    let uri = format!("/admin/products/{product}/variants/{variant}/media");
    let body = json!({ "asset": first });
    let (status, shown) = send(&app, "PUT", &uri, Some(&body.to_string())).await;
    assert_eq!(
        (status, &shown["variants"][0]["media_asset_id"]),
        (StatusCode::OK, &json!(first))
    );

    let (status, used) = send(&app, "GET", &format!("/admin/assets/{first}/usages"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        used["items"],
        json!([{ "owner": { "kind": "product", "id": product }, "role": "gallery" }])
    );

    let (status, body) = send(&app, "DELETE", &format!("/admin/assets/{first}"), None).await;
    assert_eq!(
        (status, code(&body), &body["details"]),
        (
            StatusCode::CONFLICT,
            "asset_in_use",
            &json!({
                "usages": [{ "owner": { "kind": "product", "id": product }, "role": "gallery" }],
                "has_more": false,
            })
        )
    );

    // Detaching clears the variant's image and frees the asset.
    let body = json!({ "assets": [second] });
    let (_, detached) = send(&app, "PUT", &media, Some(&body.to_string())).await;
    assert_eq!(detached["variants"][0]["media_asset_id"], Value::Null);
    let (status, _) = send(&app, "DELETE", &format!("/admin/assets/{first}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let body = json!({ "assets": [first] });
    let (status, body) = send(&app, "PUT", &media, Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "asset_not_found")
    );
    let pending = start_png_upload(&app, 100).await;
    let body = json!({ "assets": [pending["asset"]["id"]] });
    let (status, body) = send(&app, "PUT", &media, Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "asset_not_ready")
    );
}

#[tokio::test]
async fn the_upload_url_cant_replace_a_checked_file() {
    let app = app_with_media().await;
    let created = start_png_upload(&app, 100).await;
    let id = created["asset"]["id"].as_str().unwrap();
    assert_eq!(put_file(&created["upload"], png(100)).await, 200);
    let (_, ready) = send(&app, "POST", &format!("/admin/assets/{id}/complete"), None).await;

    // Storage still takes the URL until it expires, but only into the upload key.
    let mut other = b"%PDF-1.7".to_vec();
    other.resize(100, 0);
    assert_eq!(put_file(&created["upload"], other).await, 200);
    let served = reqwest::get(ready["url"].as_str().unwrap()).await.unwrap();
    assert_eq!(served.bytes().await.unwrap().to_vec(), png(100));
}

#[tokio::test]
async fn completing_checks_what_was_stored() {
    let app = app_with_media().await;
    let created = start_png_upload(&app, 100).await;
    let id = created["asset"]["id"].as_str().unwrap();
    let complete = format!("/admin/assets/{id}/complete");
    let (status, body) = send(&app, "POST", &complete, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "upload_missing")
    );

    let mut pdf = b"%PDF-1.7".to_vec();
    pdf.resize(100, 0);
    assert_eq!(put_file(&created["upload"], pdf).await, 200);
    let (status, body) = send(&app, "POST", &complete, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "upload_mismatch")
    );
    let (_, asset) = send(&app, "GET", &format!("/admin/assets/{id}"), None).await;
    assert_eq!(asset["status"], "failed");
    let (status, body) = send(&app, "POST", &complete, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "asset_not_pending")
    );
}

#[tokio::test]
async fn storage_refuses_an_upload_of_another_size() {
    let app = app_with_media().await;
    let created = start_png_upload(&app, 100).await;
    assert_ne!(put_file(&created["upload"], png(99)).await, 200);
}

#[tokio::test]
async fn bad_uploads_are_refused_with_codes() {
    let app = app_with_media().await;
    for (body, expected) in [
        (
            json!({ "filename": "logo.svg", "mime_type": "image/svg+xml", "byte_size": 10 }),
            "unsupported_media_type",
        ),
        (
            json!({ "filename": "huge.png", "mime_type": "image/png", "byte_size": 21 * 1024 * 1024 }),
            "file_too_large",
        ),
        (
            json!({ "filename": "a.png", "mime_type": "image/png", "byte_size": 0 }),
            "empty_file",
        ),
        (
            json!({ "filename": "a.png", "mime_type": "image/png", "byte_size": 1, "checksum": "abc" }),
            "invalid_checksum",
        ),
    ] {
        let (status, json) = send(&app, "POST", "/admin/assets", Some(&body.to_string())).await;
        assert_eq!(
            (status, code(&json)),
            (StatusCode::UNPROCESSABLE_ENTITY, expected),
            "{body}"
        );
    }
}

#[tokio::test]
async fn the_library_needs_media_scopes() {
    let app = app_with_media().await;
    let products = key(
        &app,
        json!({ "kind": "secret", "name": "Products", "scopes": ["products:write"] }),
    )
    .await;
    let (status, body) = send_as(&app, &products, "GET", "/admin/assets", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
}

#[tokio::test]
async fn without_file_storage_the_library_is_unavailable() {
    let app = app().await;
    let (status, body) = send(&app, "GET", "/admin/assets", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::SERVICE_UNAVAILABLE, "media_not_configured")
    );
}

// ---------- authentication (design note 0008) ----------

/// Logs the owner in; returns the session token.
async fn owner_session(app: &TestApp) -> String {
    let body = json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }).to_string();
    let (status, login) = send_with(app, &[], "POST", "/admin/auth/login", Some(&body)).await;
    assert_eq!(
        (status, login["status"].as_str()),
        (StatusCode::OK, Some("authenticated"))
    );
    login["session"]["token"].as_str().unwrap().to_string()
}

/// Creates a key as the admin; returns its token.
async fn key(app: &TestApp, body: Value) -> String {
    let (status, created) = send(app, "POST", "/admin/api-keys", Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn no_credentials_is_401_with_a_bearer_challenge() {
    let app = app().await;
    let request = Request::builder()
        .uri("/admin/store")
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()[WWW_AUTHENTICATE], "Bearer");
    let (_, body) = send_with(&app, &[], "GET", "/admin/store", None).await;
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
        let (status, body) = send_with(
            &app,
            &[("authorization", &auth)],
            "GET",
            "/admin/store",
            None,
        )
        .await;
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
        "/admin/store",
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
    let (status, me) = send_as(&app, &session, "GET", "/admin/me", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["user"]["email"], OWNER_EMAIL);
    assert_eq!(me["role"]["name"], "Owner");
    assert_eq!(me["mfa_enabled"], false);
    assert_eq!(
        me["scopes"].as_array().map(Vec::len),
        Some(Scope::ALL.len())
    );

    assert_eq!(
        send_as(&app, &session, "POST", "/admin/auth/logout", None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (status, body) = send_as(&app, &session, "GET", "/admin/me", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );
}

#[tokio::test]
async fn a_wrong_password_is_401() {
    let app = app().await;
    let body = json!({ "email": OWNER_EMAIL, "password": "nope nope" }).to_string();
    let (status, body) = send_with(&app, &[], "POST", "/admin/auth/login", Some(&body)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "invalid_credentials")
    );
}

#[tokio::test]
async fn me_routes_need_a_session_not_a_key() {
    let app = app().await;
    let (status, body) = send(&app, "GET", "/admin/me", None).await;
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
    let (status, body) = send_as(&app, &reader, "POST", "/admin/products", Some(product)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
}

#[tokio::test]
async fn a_publishable_key_works_on_the_storefront_only() {
    let app = app().await;
    let (status, created) = send(
        &app,
        "POST",
        "/admin/api-keys",
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
    let (status, store) = send_with(&app, &header, "GET", "/storefront/store", None).await;
    assert_eq!(
        (status, &store),
        (StatusCode::OK, &json!({ "name": "Test" }))
    );
    // In the Authorization header too.
    let (status, _) = send_as(&app, token, "GET", "/storefront/store", None).await;
    assert_eq!(status, StatusCode::OK);
    // Never on the admin surface, whatever its scopes (the store read is one of them).
    for (method, path, body) in [
        ("GET", "/admin/store", None),
        ("POST", "/admin/customers", Some(r#"{"email":"a@b.co"}"#)),
    ] {
        let (status, json) = send_as(&app, token, method, path, body).await;
        assert_eq!(
            (status, code(&json)),
            (StatusCode::FORBIDDEN, "admin_credentials_required"),
            "{path}"
        );
    }
}

#[tokio::test]
async fn the_storefront_takes_no_admin_credentials() {
    let app = app().await;
    let session = owner_session(&app).await;
    for token in [app.admin_key.as_str(), session.as_str()] {
        let (status, json) = send_as(&app, token, "GET", "/storefront/store", None).await;
        assert_eq!(
            (status, code(&json)),
            (StatusCode::FORBIDDEN, "publishable_key_required")
        );
    }
}

/// A publishable key, made with the admin key.
async fn publishable(app: &TestApp) -> String {
    let body = json!({ "kind": "publishable", "name": "Storefront" });
    key(app, body).await
}

#[tokio::test]
async fn a_storefront_sees_active_products_only_without_admin_fields() {
    let app = app().await;
    let (_, product) = send(
        &app,
        "POST",
        "/admin/products",
        Some(r#"{"handle":"mug","title":"Mug","price":{"amount":1000,"currency":"EUR"}}"#),
    )
    .await;
    let id = product["id"].as_str().unwrap();
    let pk = publishable(&app).await;
    let path = format!("/storefront/products/{id}");

    // A draft reads as missing.
    let (status, json) = send_as(&app, &pk, "GET", &path, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "product_not_found")
    );

    let activate = r#"{"status":"active"}"#;
    send(
        &app,
        "PATCH",
        &format!("/admin/products/{id}/status"),
        Some(activate),
    )
    .await;
    let (status, json) = send_as(&app, &pk, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["handle"], "mug");
    assert!(json.get("status").is_none());
    // No prices: the base price isn't what a shopper in a market pays.
    assert!(json["variants"][0]["id"].is_string());
    assert!(json["variants"][0].get("price").is_none());
}

#[tokio::test]
async fn linking_a_cart_to_a_customer_is_admin_only() {
    let app = app().await;
    let pk = publishable(&app).await;
    let cart = "/storefront/carts/cart_00000000000000000000000000000063/customer";
    let (status, json) = send_as(&app, &pk, "PUT", cart, Some(r#"{"customer":null}"#)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "route_not_found")
    );
}

#[tokio::test]
async fn a_revoked_key_stops_working() {
    let app = app().await;
    let (_, created) = send(
        &app,
        "POST",
        "/admin/api-keys",
        Some(r#"{"kind":"secret","name":"Temp","scopes":["stores:read"]}"#),
    )
    .await;
    let token = created["token"].as_str().unwrap();
    let id = created["id"].as_str().unwrap();
    assert_eq!(
        send_as(&app, token, "GET", "/admin/store", None).await.0,
        StatusCode::OK
    );
    let (status, revoked) = send(&app, "POST", &format!("/admin/api-keys/{id}/revoke"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(revoked["revoked_at"].is_string());
    assert_eq!(
        send_as(&app, token, "GET", "/admin/store", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, body) = send(&app, "POST", &format!("/admin/api-keys/{id}/revoke"), None).await;
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
    let (status, body) = send_as(&app, &limited, "POST", "/admin/api-keys", Some(&body)).await;
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
        "/admin/me/mfa/totp",
        Some(r#"{"password":"nope"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "wrong_password")
    );
    let password = json!({ "password": OWNER_PASSWORD }).to_string();
    let (status, setup) = send_as(
        &app,
        &session,
        "POST",
        "/admin/me/mfa/totp",
        Some(&password),
    )
    .await;
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
        "/admin/me/mfa/totp/confirm",
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
        "/admin/me/mfa/totp/confirm",
        Some(&confirm),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let recovery = codes["recovery_codes"][0].as_str().unwrap().to_string();

    let body = json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }).to_string();
    let (status, login) = send_with(&app, &[], "POST", "/admin/auth/login", Some(&body)).await;
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
        send_as(&app, challenge, "GET", "/admin/me", None).await.0,
        StatusCode::UNAUTHORIZED
    );

    let wrong = json!({ "challenge": challenge, "code": "000000" }).to_string();
    let (status, body) = send_with(&app, &[], "POST", "/admin/auth/login/mfa", Some(&wrong)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "invalid_mfa_code")
    );

    // The confirm used this step's TOTP code, so finish with a recovery code.
    let right = json!({ "challenge": challenge, "code": recovery }).to_string();
    let (status, done) = send_with(&app, &[], "POST", "/admin/auth/login/mfa", Some(&right)).await;
    assert_eq!(
        (status, done["status"].as_str()),
        (StatusCode::OK, Some("authenticated"))
    );
    let new_session = done["session"]["token"].as_str().unwrap();
    let (_, me) = send_as(&app, new_session, "GET", "/admin/me", None).await;
    assert_eq!(me["mfa_enabled"], true);
}

#[tokio::test]
async fn roles_and_invites_over_http() {
    let app = app().await;
    let (status, role) = send(
        &app,
        "POST",
        "/admin/roles",
        Some(r#"{"name":"Support","scopes":["orders:write","customers:read"]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(role["scopes"], json!(["customers:read", "orders:write"]));
    let id = role["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        "POST",
        "/admin/roles",
        Some(r#"{"name":"support","scopes":["orders:read"]}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "role_name_taken")
    );

    let invite = json!({ "email": "bob@example.com", "role": id }).to_string();
    let (status, invited) = send(&app, "POST", "/admin/invites", Some(&invite)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(invited["email"], "bob@example.com");
    assert!(invited.get("token").is_none());

    let unknown_role = "role_00000000000000000000000000000063";
    let (status, body) = send(&app, "GET", &format!("/admin/roles/{unknown_role}"), None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "role_not_found")
    );
    let invite = json!({ "email": "eve@example.com", "role": unknown_role }).to_string();
    let (status, body) = send(&app, "POST", "/admin/invites", Some(&invite)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "role_not_found")
    );

    let accept =
        json!({ "token": "hypha_inv_nope", "name": "Bob", "password": "bob password" }).to_string();
    let (status, body) = send_with(&app, &[], "POST", "/admin/invites/accept", Some(&accept)).await;
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
            send_with(&app, &[], "POST", "/admin/auth/password-reset", Some(&body))
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
    let (_, me) = send_as(&app, &session, "GET", "/admin/me", None).await;
    let id = me["user"]["id"].as_str().unwrap();
    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/admin/users/{id}"),
        Some(r#"{"status":"disabled"}"#),
    )
    .await;
    assert_eq!((status, code(&body)), (StatusCode::CONFLICT, "last_owner"));
}

#[tokio::test]
async fn a_bad_token_is_refused_before_the_body_is_looked_at() {
    let app = app().await;
    let fake = format!("Bearer hypha_sk_{}", "0".repeat(64));
    for body in ["{", r#"{"titel":"x"}"#] {
        let (status, json) = send_with(
            &app,
            &[("authorization", &fake)],
            "POST",
            "/admin/products",
            Some(body),
        )
        .await;
        assert_eq!(
            (status, code(&json)),
            (StatusCode::UNAUTHORIZED, "invalid_token"),
            "{body}"
        );
    }
    // Keyed routes too: nothing is parsed or fingerprinted for a stranger.
    let fake_publishable = format!("Bearer hypha_pk_{}", "0".repeat(64));
    let (status, json) = send_with(
        &app,
        &[
            ("authorization", &fake_publishable),
            ("idempotency-key", "k1"),
        ],
        "POST",
        "/storefront/carts",
        Some("{"),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );
}

// ---------- rate limits (design note 0009) ----------

fn login_body(email: &str) -> String {
    json!({ "email": email, "password": "wrong password" }).to_string()
}

#[tokio::test]
async fn too_many_logins_from_one_address_get_429_with_retry_after() {
    let app = app().await;
    let (burst, every) = crate::rate_limit::Rule::LoginIp.quota();
    for n in 0..burst {
        // Different emails, so only the per-address limit can trip.
        let body = login_body(&format!("user{n}@example.com"));
        let (status, _) = send_with(&app, &[], "POST", "/admin/auth/login", Some(&body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let request = Request::builder()
        .method("POST")
        .uri("/admin/auth/login")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(login_body("again@example.com")))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response.headers()[axum::http::header::RETRY_AFTER],
        every.as_secs().to_string()
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(code(&body), "rate_limited");
    assert_eq!(body["details"]["retry_after_secs"], every.as_secs());
}

#[tokio::test]
async fn one_email_is_limited_however_many_addresses_try_it() {
    // Behind a trusted proxy (the test peer), each request names another client.
    let app = app_with(TrustedProxies::parse("127.0.0.1").unwrap()).await;
    let (burst, _) = crate::rate_limit::Rule::LoginEmail.quota();
    let body = login_body(OWNER_EMAIL);
    for n in 0..burst {
        let client = format!("203.0.{}.{}", n / 250, n % 250);
        let headers = [("x-forwarded-for", client.as_str())];
        let (status, _) = send_with(&app, &headers, "POST", "/admin/auth/login", Some(&body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{client}");
    }
    let headers = [("x-forwarded-for", "198.51.100.1")];
    let (status, json) = send_with(&app, &headers, "POST", "/admin/auth/login", Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    );
    // The address itself is fine: another email from it goes through.
    let other = login_body("someone@example.com");
    let (status, _) = send_with(&app, &headers, "POST", "/admin/auth/login", Some(&other)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn hammering_an_email_from_one_address_does_not_lock_its_owner_out() {
    let app = app_with(TrustedProxies::parse("127.0.0.1").unwrap()).await;
    let (burst, _) = crate::rate_limit::Rule::LoginEmailFromIp.quota();
    let attacker = [("x-forwarded-for", "203.0.113.66")];
    let wrong = login_body(OWNER_EMAIL);
    for _ in 0..burst {
        send_with(&app, &attacker, "POST", "/admin/auth/login", Some(&wrong)).await;
    }
    let (status, _) = send_with(&app, &attacker, "POST", "/admin/auth/login", Some(&wrong)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    // The owner, elsewhere, logs in as usual.
    let owner = [("x-forwarded-for", "198.51.100.7")];
    let right = json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }).to_string();
    let (status, json) = send_with(&app, &owner, "POST", "/admin/auth/login", Some(&right)).await;
    assert_eq!(
        (status, json["status"].as_str()),
        (StatusCode::OK, Some("authenticated"))
    );
}

#[tokio::test]
async fn something_that_is_not_an_email_still_gets_its_answer() {
    // A huge "email" isn't an email, so no email limit keys it (`auth::limit_email`,
    // tested there); the request is just a wrong login.
    let app = app().await;
    let huge = "x".repeat(100_000);
    let body = json!({ "email": huge, "password": "guess" }).to_string();
    let (status, json) = send_with(&app, &[], "POST", "/admin/auth/login", Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "invalid_credentials")
    );
}

#[tokio::test]
async fn asking_for_reset_links_does_not_block_using_one() {
    let app = app().await;
    let (burst, _) = crate::rate_limit::Rule::ResetIp.quota();
    for n in 0..burst {
        let body = json!({ "email": format!("user{n}@example.com") }).to_string();
        send_with(&app, &[], "POST", "/admin/auth/password-reset", Some(&body)).await;
    }
    let confirm = json!({ "token": "hypha_rst_nope", "password": "new password" }).to_string();
    let (status, json) = send_with(
        &app,
        &[],
        "POST",
        "/admin/auth/password-reset/confirm",
        Some(&confirm),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );
}

#[tokio::test]
async fn a_forged_forwarded_for_does_not_escape_the_limit() {
    // No trusted proxies: the header is ignored, so all of these are the same client.
    let app = app().await;
    let (burst, _) = crate::rate_limit::Rule::LoginIp.quota();
    let mut last = StatusCode::OK;
    for n in 0..=burst {
        let client = format!("203.0.113.{n}");
        let body = login_body(&format!("user{n}@example.com"));
        let headers = [("x-forwarded-for", client.as_str())];
        last = send_with(&app, &headers, "POST", "/admin/auth/login", Some(&body))
            .await
            .0;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn reset_requests_for_one_email_are_limited_per_address() {
    let app = app_with(TrustedProxies::parse("127.0.0.1").unwrap()).await;
    let (burst, _) = crate::rate_limit::Rule::ResetEmailFromIp.quota();
    let body = json!({ "email": OWNER_EMAIL }).to_string();
    let here = [("x-forwarded-for", "203.0.113.1")];
    for _ in 0..burst {
        let (status, _) = send_with(
            &app,
            &here,
            "POST",
            "/admin/auth/password-reset",
            Some(&body),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }
    let (status, json) = send_with(
        &app,
        &here,
        "POST",
        "/admin/auth/password-reset",
        Some(&body),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    );
    // From elsewhere the same email still gets its 202.
    let there = [("x-forwarded-for", "198.51.100.1")];
    let (status, _) = send_with(
        &app,
        &there,
        "POST",
        "/admin/auth/password-reset",
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn the_address_limit_comes_before_the_body_is_parsed() {
    let app = app().await;
    let (burst, _) = crate::rate_limit::Rule::InviteIp.quota();
    for _ in 0..burst {
        send_with(&app, &[], "POST", "/admin/invites/accept", Some("{")).await;
    }
    let (status, json) = send_with(&app, &[], "POST", "/admin/invites/accept", Some("{")).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    );
}

// ---------- the route table (design note 0010) ----------

#[test]
fn every_documented_route_is_listed_once_under_its_surface() {
    use super::openapi::{Doc, rows};
    let mut seen = std::collections::HashSet::new();
    for row in rows() {
        assert!(
            seen.insert((row.method, row.path)),
            "{} twice",
            row.operation_id
        );
        let prefix = match row.doc {
            Doc::Admin => "/admin/",
            Doc::Storefront => "/storefront/",
        };
        assert!(row.path.starts_with(prefix), "{}", row.operation_id);
    }
}

/// A value matching a path parameter's schema in `doc`: an ID from its prefix pattern
/// (`^prod_[0-9a-f]{32}$`), or a country code.
fn example_for(doc: &Value, param: &Value) -> String {
    let schema = &param["schema"];
    let schema = match schema["$ref"].as_str() {
        Some(r) => &doc["components"]["schemas"][r.trim_start_matches("#/components/schemas/")],
        None => schema,
    };
    let pattern = schema["pattern"].as_str().unwrap_or_default();
    match pattern.strip_prefix('^').and_then(|p| p.split_once('_')) {
        Some((prefix, _)) => format!("{prefix}_{}", "0".repeat(32)),
        None if pattern.contains("[A-Z]{2}") => "HR".to_string(),
        None => panic!("no example for path parameter {param}"),
    }
}

#[tokio::test]
async fn every_route_refuses_the_other_surfaces_credentials() {
    // The table's surface and the handler's `Caller<A>` must agree; this catches a
    // storefront route taking admin credentials or the reverse, for every route.
    use super::openapi::{Doc, document};
    let app = app().await;
    let publishable = publishable(&app).await;
    for which in Doc::ALL {
        let doc = serde_json::to_value(document(which)).unwrap();
        let (wrong, expected) = match which {
            Doc::Admin => (publishable.as_str(), "admin_credentials_required"),
            Doc::Storefront => (app.admin_key.as_str(), "publishable_key_required"),
        };
        for (template, item) in doc["paths"].as_object().unwrap() {
            for (method, op) in item.as_object().unwrap() {
                if op["security"] == json!([{}]) {
                    continue; // needs no credentials at all
                }
                let mut path = template.clone();
                for param in op["parameters"].as_array().into_iter().flatten() {
                    if param["in"] == "path" {
                        let name = param["name"].as_str().unwrap();
                        path = path.replace(&format!("{{{name}}}"), &example_for(&doc, param));
                    }
                }
                let method = method.to_uppercase();
                let body = (method != "GET").then_some("{}");
                let (status, json) = send_as(&app, wrong, &method, &path, body).await;
                assert_eq!(
                    (status, code(&json)),
                    (StatusCode::FORBIDDEN, expected),
                    "{method} {path}"
                );
            }
        }
    }
}
