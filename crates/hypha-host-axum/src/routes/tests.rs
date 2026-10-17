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
use hypha_auth::{ApiKeyKind, Argon2id, CreateApiKey, create_api_key, totp};
use hypha_config::Config;
use hypha_core::{Ctx, Scope, Timestamp};
use hypha_stores::ensure_single_store;
use hypha_users::{NewOwner, bootstrap_owner};
use serde_json::{Value, json};
use tower::ServiceExt;

use std::net::SocketAddr;

use axum::extract::connect_info::MockConnectInfo;

use super::postgres_router;
use crate::media::Media;
use crate::state::{AppState, HostEnv, PgStorage, Storage, SystemClock, UuidV7Ids};

const OWNER_EMAIL: &str = "owner@example.com";
const OWNER_PASSWORD: &str = "owner password";

/// The app on a fresh schema, with its owner (`OWNER_EMAIL`), and a secret key with every scope
/// that `send` uses unless a test picks other credentials.
struct TestApp {
    router: Router,
    admin_key: String,
    /// The app's state, for checks the API doesn't expose (the outbox).
    state: crate::state::SharedState<PgStorage>,
}

/// The app without file storage: most routes don't need it, so their tests don't need
/// the S3 server.
async fn app() -> TestApp {
    app_from(Config::defaults(), false).await
}

/// The app with file storage (the compose S3 server), for the media routes.
async fn app_with_media() -> TestApp {
    // The compose S3 server, from `.cargo/config.toml`'s `HYPHA_S3_*`.
    let config = Config::load(None, |name| std::env::var(name).ok())
        .expect("HYPHA_S3_* (see .cargo/config.toml)");
    app_from(config, true).await
}

/// The app under `hypha.config.toml` text (design note 0017), without the environment.
async fn app_configured(toml: &str) -> TestApp {
    let config = Config::load(Some(toml), |_| None).unwrap();
    app_from(config, false).await
}

/// Behind a trusted proxy: the test peer.
async fn app_behind_proxy() -> TestApp {
    app_configured("[server]\ntrusted_proxies = [\"127.0.0.1\"]").await
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

/// File storage under `config` (the compose server), bucket created.
async fn test_media(config: &Config) -> Media {
    let media = Media::from_config(&config.media)
        .expect("HYPHA_S3_* (see .cargo/config.toml)")
        .expect("HYPHA_S3_ENDPOINT (see .cargo/config.toml)");
    media
        .blobs
        .ensure_bucket()
        .await
        .unwrap_or_else(|e| panic!("route tests need the S3 server (`docker compose up -d`): {e}"));
    media
}

/// Everything from `config`, the one source of settings, as in `serve`; file storage
/// with `media`.
async fn app_from(mut config: Config, media: bool) -> TestApp {
    // Warnings and errors (a 500's cause is only logged) go to the test's output, which
    // a failing test prints: in CI, that's the only place to see them.
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter(tracing_subscriber::EnvFilter::new("warn"))
        .try_init();
    // The tests' rate apps and webhook receivers listen on localhost.
    config.allow_private_callbacks = true;
    let media = match media {
        true => Some(test_media(&config).await),
        false => None,
    };
    let storage = PgStorage::connect(&fresh_schema().await, &config.database)
        .await
        .unwrap();
    let store = ensure_single_store(
        &Ctx::system(),
        &storage.directory().await.unwrap(),
        &UuidV7Ids,
        "Test",
    )
    .await
    .unwrap();
    let env = HostEnv::new([7; 32], None, config.auth).with_hasher(Argon2id::new(8, 1, 1));
    let state = Arc::new(AppState {
        storage,
        ids: UuidV7Ids,
        clock: SystemClock,
        env,
        background: Default::default(),
        limiter: crate::rate_limit::MemoryRateLimiter::new(&config.rate_limits),
        publishable_keys: Default::default(),
        store: store.id(),
        store_name: store.name().to_string(),
        media,
        http: crate::outbound::client(crate::outbound::OutboundPolicy {
            allow_private: config.allow_private_callbacks,
        })
        .unwrap(),
        config,
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
    // As `serve` does at startup.
    hypha_inventory::ensure_default_location(
        &Ctx::system(),
        hypha_inventory::HasLocations::locations(&ports),
        &UuidV7Ids,
    )
    .await
    .unwrap();
    let input = CreateApiKey {
        kind: ApiKeyKind::Secret,
        name: "Tests".to_string(),
        scopes: Some(Scope::ALL.to_vec()),
        channel: None,
    };
    let (_, token) = create_api_key(&Ctx::system(), &ports, &state.env, input)
        .await
        .unwrap();
    TestApp {
        router: postgres_router()
            .with_state(state.clone())
            .layer(MockConnectInfo(SocketAddr::from(PEER))),
        admin_key: token.expose().to_string(),
        state,
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
async fn the_body_limit_and_rate_limits_are_configurable() {
    let app = app_configured(
        r#"
        [server]
        max_body = "1KiB"

        [rate_limits]
        login_ip = { burst = 1, per = "1h" }
        "#,
    )
    .await;
    let big = format!("{{\"title\": \"{}\"}}", "x".repeat(1024));
    let (status, body) = send(&app, "POST", "/admin/products", Some(&big)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::PAYLOAD_TOO_LARGE, "body_too_large")
    );

    let login = r#"{"email": "nobody@example.com", "password": "wrong-password"}"#;
    let (status, _) = send(&app, "POST", "/admin/auth/login", Some(login)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = send(&app, "POST", "/admin/auth/login", Some(login)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
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
async fn the_store_remembers_the_setup_guide() {
    let app = app().await;
    let (_, store) = send(&app, "GET", "/admin/store", None).await;
    assert_eq!(store["setup"], json!({"step": null, "finished_at": null}));
    let (status, store) = send(
        &app,
        "PATCH",
        "/admin/store",
        Some(r#"{"setup_step":"market"}"#),
    )
    .await;
    assert_eq!(
        (status, &store["setup"]["step"]),
        (StatusCode::OK, &json!("market"))
    );
    let (_, store) = send(
        &app,
        "PATCH",
        "/admin/store",
        Some(r#"{"setup_finished":true}"#),
    )
    .await;
    assert!(store["setup"]["finished_at"].is_string(), "{store}");
    let (_, store) = send(
        &app,
        "PATCH",
        "/admin/store",
        Some(r#"{"setup_finished":false}"#),
    )
    .await;
    assert_eq!(
        store["setup"],
        json!({"step": "market", "finished_at": null})
    );
    let (status, body) = send(
        &app,
        "PATCH",
        "/admin/store",
        Some(r#"{"setup_step":"No!"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_setup_step")
    );
    let (_, public) = send_as(
        &app,
        &publishable(&app).await,
        "GET",
        "/storefront/store",
        None,
    )
    .await;
    assert!(public.get("setup").is_none(), "storefronts don't see it");
}

#[tokio::test]
async fn the_store_details_are_set_cleared_and_partly_public() {
    let app = app().await;
    let (_, store) = send(&app, "GET", "/admin/store", None).await;
    assert_eq!(
        (&store["default_language"], &store["timezone"]),
        (&json!("en"), &Value::Null)
    );
    let body = json!({
        "contact_email": "Hi@Lumen.example",
        "description": " Candles. ",
        "legal_name": "Lumen d.o.o.",
        "legal_address": {"last_name": "Lumen", "line1": "Ilica 1", "city": "Zagreb", "country": "HR"},
        "timezone": "Europe/Zagreb",
        "default_language": "hr"
    })
    .to_string();
    let (status, store) = send(&app, "PATCH", "/admin/store", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{store}");
    assert_eq!(
        (
            &store["contact_email"],
            &store["description"],
            &store["legal_address"]["city"]
        ),
        (
            &json!("hi@lumen.example"),
            &json!("Candles."),
            &json!("Zagreb")
        )
    );

    let key = publishable(&app).await;
    let (_, public) = send_with(
        &app,
        &[("x-publishable-key", &key)],
        "GET",
        "/storefront/store",
        None,
    )
    .await;
    assert_eq!(
        public,
        json!({"name": store["name"], "description": "Candles.", "contact_email": "hi@lumen.example", "default_language": "hr"})
    );

    for (body, want) in [
        (r#"{"timezone":"Mars/Base"}"#, "unknown_timezone"),
        (r#"{"default_language":"en_GB"}"#, "invalid_language"),
        (r#"{"default_language":null}"#, "invalid_body"),
        (r#"{"contact_email":"nope"}"#, "invalid_email"),
    ] {
        let (status, error) = send(&app, "PATCH", "/admin/store", Some(body)).await;
        assert_eq!(
            (status, code(&error)),
            (StatusCode::UNPROCESSABLE_ENTITY, want),
            "{body}"
        );
    }
    let body = r#"{"contact_email":null,"description":null,"legal_name":null,"legal_address":null,"timezone":null}"#;
    let (_, cleared) = send(&app, "PATCH", "/admin/store", Some(body)).await;
    for field in [
        "contact_email",
        "description",
        "legal_name",
        "legal_address",
        "timezone",
    ] {
        assert_eq!(cleared[field], Value::Null, "{field}");
    }
    assert_eq!(cleared["default_language"], json!("hr"));
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

#[tokio::test]
async fn currencies_list_every_iso_code_with_its_minor_units() {
    let app = app().await;
    let (status, body) = send(&app, "GET", "/admin/currencies", None).await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), hypha_core::Currency::all().count());
    let jpy = items.iter().find(|c| c["code"] == "JPY").unwrap();
    assert_eq!(
        jpy,
        &json!({"code": "JPY", "name": "Japanese Yen", "minor_units": 0})
    );

    let (status, body) = send_with(&app, &[], "GET", "/admin/currencies", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "unauthenticated")
    );
}

#[tokio::test]
async fn markets_take_any_iso_currency_but_only_assigned_countries() {
    let app = app().await;
    let body = r#"{"name":"Japan","countries":["JP"],"currency":"JPY"}"#;
    let (status, market) = send(&app, "POST", "/admin/markets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{market}");
    assert_eq!(market["currency"], "JPY");

    let body = r#"{"name":"Kosovo","countries":["XK"],"currency":"EUR"}"#;
    let (status, error) = send(&app, "POST", "/admin/markets", Some(body)).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "unknown_country_code")
    );

    let body = r#"{"name":"Gold","countries":["CH"],"currency":"XAU"}"#;
    let (status, error) = send(&app, "POST", "/admin/markets", Some(body)).await;
    // DTOs read `Currency` itself, so it fails as the body, with the reason in the message.
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body")
    );
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("unknown currency \"XAU\"")
    );
}

#[tokio::test]
async fn the_admin_reads_its_whole_setup() {
    let app = app().await;
    let product = create_product(&app, "setup-mug").await;
    let body = r#"{"name":"Eurozone","countries":["HR","DE"],"currency":"EUR"}"#;
    let (_, market) = send(&app, "POST", "/admin/markets", Some(body)).await;
    assert_eq!(market["catalog"], Value::Null);
    let market_id = market["id"].as_str().unwrap();
    let body = format!(r#"{{"market":"{market_id}"}}"#);
    let (_, catalog) = send(&app, "POST", "/admin/catalogs", Some(&body)).await;
    let catalog_id = catalog["id"].as_str().unwrap();
    let body = format!(r#"{{"products":["{product}","prod_00000000000000000000000000000063"]}}"#);
    let url = format!("/admin/catalogs/{catalog_id}/products");
    let (status, _) = send(&app, "POST", &url, Some(&body)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, markets) = send(&app, "GET", "/admin/markets", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(markets["items"][0]["catalog"], json!(catalog_id));
    let (_, fetched) = send(&app, "GET", &format!("/admin/markets/{market_id}"), None).await;
    assert_eq!(fetched["catalog"], json!(catalog_id));
    let body = r#"{"country":"SI"}"#;
    let url = format!("/admin/markets/{market_id}/countries");
    let (_, changed) = send(&app, "POST", &url, Some(body)).await;
    assert_eq!(
        changed["catalog"],
        json!(catalog_id),
        "a write shows the catalog too"
    );
    let (status, body) = send(
        &app,
        "GET",
        "/admin/markets/mkt_00000000000000000000000000000063",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "market_not_found")
    );

    let (status, fetched) = send(&app, "GET", &format!("/admin/catalogs/{catalog_id}"), None).await;
    assert_eq!(
        (status, &fetched["market"]),
        (StatusCode::OK, &json!(market_id))
    );
    let url = format!("/admin/catalogs/{catalog_id}/products?limit=1");
    let (_, page) = send(&app, "GET", &url, None).await;
    assert_eq!(page["items"][0]["handle"], json!("setup-mug"));
    assert_eq!(page["has_more"], json!(true));
    let cursor = page["next_cursor"].as_str().unwrap();
    let url = format!("/admin/catalogs/{catalog_id}/products?limit=1&cursor={cursor}");
    let (_, page) = send(&app, "GET", &url, None).await;
    // An ID that names no product is listed with no fields.
    assert_eq!(
        page["items"][0],
        json!({"id": "prod_00000000000000000000000000000063", "title": null, "handle": null, "status": null})
    );

    let body = r#"{"currency":"EUR","adjustment_bps":-1000}"#;
    let (_, list) = send(&app, "POST", "/admin/price-lists", Some(body)).await;
    let list_id = list["id"].as_str().unwrap();
    let (_, lists) = send(&app, "GET", "/admin/price-lists", None).await;
    assert_eq!(lists["items"][0]["adjustment_bps"], json!(-1000));
    let (status, _) = send(&app, "GET", &format!("/admin/price-lists/{list_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    let url = format!("/admin/price-lists/{list_id}/fixed-prices");
    let (status, prices) = send(&app, "GET", &url, None).await;
    assert_eq!((status, &prices["items"]), (StatusCode::OK, &json!([])));

    let (_, _) = send(
        &app,
        "POST",
        "/admin/tax-classes",
        Some(r#"{"name":"Reduced"}"#),
    )
    .await;
    let body = r#"{"country":"HR","default_rate_ppm":250000}"#;
    let (_, _) = send(&app, "POST", "/admin/tax-regions", Some(body)).await;
    let (status, classes) = send(&app, "GET", "/admin/tax-classes", None).await;
    assert_eq!(
        (status, &classes["items"][0]["name"]),
        (StatusCode::OK, &json!("Reduced"))
    );
    let (_, regions) = send(&app, "GET", "/admin/tax-regions", None).await;
    assert_eq!(regions["items"][0]["country"], json!("HR"));

    let (_, countries) = send(&app, "GET", "/admin/countries", None).await;
    let countries = countries["items"].as_array().unwrap();
    assert_eq!(countries.len(), 249);
    assert!(countries.contains(&json!({"code": "RS", "name": "Serbia"})));
}

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

/// Every event the app's outbox holds, oldest first.
async fn outbox_events(app: &TestApp) -> Vec<hypha_core::StoredEvent> {
    use hypha_core::{HasOutbox, OutboxRepo};
    let ports = app.state.public_ports().await.unwrap();
    ports.outbox().events_after(0, 1000).await.unwrap()
}

#[tokio::test]
async fn responses_show_when_an_aggregate_was_created_and_changed() {
    let app = app().await;
    let id = create_product(&app, "mug").await;
    let (_, created) = send(&app, "GET", &format!("/admin/products/{id}"), None).await;
    let created_at = created["created_at"].as_str().unwrap().to_string();
    assert_eq!(created["updated_at"], created_at.as_str());
    let body = json!({"title": "Big mug"}).to_string();
    let (status, updated) =
        send(&app, "PATCH", &format!("/admin/products/{id}"), Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    // Created stays; updated moves on.
    assert_eq!(updated["created_at"], created_at.as_str());
    let time = |v: &Value| v.as_str().unwrap().parse::<jiff::Timestamp>().unwrap();
    assert!(time(&updated["updated_at"]) > time(&created["created_at"]));
    let (_, page) = send(&app, "GET", "/admin/products", None).await;
    assert_eq!(page["items"][0]["updated_at"], updated["updated_at"]);
}

#[tokio::test]
async fn the_dispatcher_cleans_up_after_a_removed_variant() {
    use hypha_core::{
        BasisPoints, Currency, Money, OptionId, OptionValueId, Price, PriceListId, ProductId,
        VariantId,
    };
    use hypha_markets::{HasPriceLists, PriceList, PriceListRepo};
    use hypha_products::{
        HasProducts, NewVariant, OptionValue, Product, ProductOption, ProductRepo,
    };
    let app = app().await;
    let id = |n: u128| uuid::Uuid::from_u128(n);
    let eur = |a: i64| Price::new(Money::new(a, Currency::EUR).unwrap()).unwrap();
    let (s, l) = (VariantId::from_uuid(id(11)), VariantId::from_uuid(id(12)));
    let product = ProductId::from_uuid(id(1));
    let list = PriceListId::from_uuid(id(5));
    {
        let ports = app.state.public_ports().await.unwrap();
        let size = ProductOption::new(
            OptionId::from_uuid(id(20)),
            "Size".to_string(),
            vec![
                OptionValue::new(OptionValueId::from_uuid(id(21)), "S".to_string()),
                OptionValue::new(OptionValueId::from_uuid(id(22)), "L".to_string()),
            ],
        );
        let variant = |v, value| NewVariant {
            id: v,
            sku: None,
            price: eur(1000),
            selections: vec![OptionValueId::from_uuid(id(value))],
        };
        let mut shirt = Product::new_configurable(
            product,
            hypha_products::Handle::new("shirt").unwrap(),
            "Shirt".to_string(),
            vec![size],
            variant(s, 21),
        )
        .unwrap();
        shirt.add_variant(variant(l, 22)).unwrap();
        ProductRepo::insert(ports.products(), &mut shirt)
            .await
            .unwrap();
        let mut prices = PriceList::new(list, Currency::EUR, BasisPoints::new(0).unwrap());
        PriceListRepo::insert(ports.price_lists(), &mut prices)
            .await
            .unwrap();
        ports
            .price_lists()
            .set_fixed_prices(list, &[(s, eur(900)), (l, eur(1100))])
            .await
            .unwrap();
    }
    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/admin/products/{product}/variants/{s}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    crate::jobs::dispatch(&app.state).await.unwrap();

    let ports = app.state.public_ports().await.unwrap();
    let left = ports
        .price_lists()
        .fixed_prices(list, &[s, l])
        .await
        .unwrap();
    assert_eq!(left.keys().collect::<Vec<_>>(), [&l]);
    let events = outbox_events(&app).await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == "price_list.prices_removed"
                && e.actor == hypha_core::Actor::System),
        "the cleanup's write is recorded, by the system"
    );
}

#[tokio::test]
async fn a_write_records_its_event_by_the_caller() {
    let app = app().await;
    let id = create_product(&app, "mug").await;
    let events = outbox_events(&app).await;
    let created = events
        .iter()
        .find(|e| e.event_type == "product.created")
        .expect("product.created recorded");
    assert_eq!(created.data["product"], id);
    // The admin key, not the system: ports act as whoever authenticated.
    assert!(
        matches!(created.actor, hypha_core::Actor::ApiKey(_)),
        "{:?}",
        created.actor
    );
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
    assert_eq!(first["has_more"], json!(true));
    // Opaque (design note 0016, section 9): not the last item's raw ID.
    let cursor = first["next_cursor"].as_str().unwrap();
    assert!(!cursor.contains("prod_"), "{cursor}");

    let uri = format!("/admin/products?limit=2&cursor={cursor}");
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
async fn products_list_sorted_and_filtered_in_pages() {
    let app = app().await;
    let banana = create_product(&app, "banana").await;
    let apple = create_product(&app, "apple").await;
    let cherry = create_product(&app, "cherry").await;
    let uri = format!("/admin/products/{cherry}/status");
    send(&app, "PATCH", &uri, Some(r#"{"status":"active"}"#)).await;
    let uri = format!("/admin/products/{banana}");
    let (status, _) = send(&app, "PATCH", &uri, Some(r#"{"vendor":"Acme"}"#)).await;
    assert_eq!(status, StatusCode::OK);

    let (status, first) = send(&app, "GET", "/admin/products?sort=title_asc&limit=2", None).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(ids(&first), [apple.as_str(), banana.as_str()]);
    let cursor = first["next_cursor"].as_str().unwrap();
    let uri = format!("/admin/products?sort=title_asc&limit=2&cursor={cursor}");
    let (_, second) = send(&app, "GET", &uri, None).await;
    assert_eq!(ids(&second), [cherry.as_str()]);
    // The same cursor under another sort is another list's.
    let uri = format!("/admin/products?sort=title_desc&limit=2&cursor={cursor}");
    let (status, body) = send(&app, "GET", &uri, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_cursor")
    );

    let (_, desc) = send(&app, "GET", "/admin/products?sort=title_desc", None).await;
    assert_eq!(
        ids(&desc),
        [cherry.as_str(), banana.as_str(), apple.as_str()]
    );
    let (_, oldest) = send(&app, "GET", "/admin/products?sort=oldest", None).await;
    assert_eq!(
        ids(&oldest),
        [banana.as_str(), apple.as_str(), cherry.as_str()]
    );
    let (_, changed) = send(
        &app,
        "GET",
        "/admin/products?sort=updated_desc&limit=1",
        None,
    )
    .await;
    assert_eq!(ids(&changed), [banana.as_str()], "changed last");

    let uri = "/admin/products?status=active&status=draft&sort=title_asc";
    let (_, both) = send(&app, "GET", uri, None).await;
    assert_eq!(ids(&both).len(), 3);
    let (_, acme) = send(&app, "GET", "/admin/products?vendor=ACME", None).await;
    assert_eq!(ids(&acme), [banana.as_str()]);
    let unknown = "pcat_00000000000000000000000000000001";
    let (status, none) = send(
        &app,
        "GET",
        &format!("/admin/products?category={unknown}"),
        None,
    )
    .await;
    assert_eq!((status, ids(&none).len()), (StatusCode::OK, 0));

    let (status, body) = send(&app, "GET", "/admin/products?sort=price", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_query")
    );
}

#[tokio::test]
async fn products_are_listed_by_text() {
    let app = app().await;
    let mug = create_product(&app, "stoneware-mug").await;
    create_product(&app, "cup").await;

    let (status, page) = send(&app, "GET", "/admin/products?q=WARE", None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let ids: Vec<&Value> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| &p["id"])
        .collect();
    assert_eq!(ids, [&json!(mug)]);
    let (status, page) = send(&app, "GET", "/admin/products?q=%20%20", None).await;
    assert_eq!(
        (status, page["items"].as_array().unwrap().len()),
        (StatusCode::OK, 2)
    );

    let long = "x".repeat(256);
    let (status, body) = send(&app, "GET", &format!("/admin/products?q={long}"), None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "field_too_long")
    );
}

#[tokio::test]
async fn a_listed_product_is_a_summary() {
    let app = app().await;
    let id = create_product(&app, "mug").await;
    let (_, mut page) = send(&app, "GET", "/admin/products", None).await;
    // When it was made and changed: checked in their own test.
    let item = page["items"][0].as_object_mut().unwrap();
    assert!(item.remove("created_at").is_some() && item.remove("updated_at").is_some());
    assert_eq!(
        page["items"][0],
        json!({
            "id": id,
            "handle": "mug",
            "title": "mug",
            "status": "draft",
            "vendor": null,
            "product_type": null,
            "variant_count": 1,
            "price_ranges": [{
                "min": { "amount": 1000, "currency": "EUR" },
                "max": { "amount": 1000, "currency": "EUR" },
            }],
            "featured_media": null,
            "channels": [],
            "categories": [],
        })
    );
}

#[tokio::test]
async fn a_products_content_is_edited_and_sanitised() {
    let app = app().await;
    let id = create_product(&app, "shirt").await;
    let product = format!("/admin/products/{id}");

    let body = json!({
        "description": "<p onclick=\"x()\">Soft</p><script>x()</script>",
        "vendor": "Acme",
        "product_type": "Shirt",
        "tags": ["Summer", "summer", "Linen"],
    });
    let (status, edited) = send(&app, "PATCH", &product, Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(
        (
            &edited["description"],
            &edited["vendor"],
            &edited["product_type"],
            &edited["tags"]
        ),
        (
            &json!("<p>Soft</p>"),
            &json!("Acme"),
            &json!("Shirt"),
            &json!(["Summer", "Linen"])
        )
    );
    let (_, list) = send(&app, "GET", "/admin/products", None).await;
    assert_eq!(
        (
            &list["items"][0]["vendor"],
            &list["items"][0]["product_type"]
        ),
        (&json!("Acme"), &json!("Shirt"))
    );

    let (status, cleared) = send(
        &app,
        "PATCH",
        &product,
        Some(r#"{"description":null,"vendor":null,"tags":[]}"#),
    )
    .await;
    assert_eq!(
        (
            status,
            &cleared["description"],
            &cleared["vendor"],
            &cleared["tags"]
        ),
        (StatusCode::OK, &json!(null), &json!(null), &json!([]))
    );

    let too_many: Vec<String> = (0..=250).map(|i| format!("t{i}")).collect();
    let body = json!({ "tags": too_many }).to_string();
    let (status, body) = send(&app, "PATCH", &product, Some(&body)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "too_many_tags")
    );
    let body = json!({ "description": "a".repeat(64 * 1024 + 1) }).to_string();
    let (status, body) = send(&app, "PATCH", &product, Some(&body)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "description_too_long")
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
async fn a_storefront_reads_its_order_with_the_access_token_only() {
    let app = app().await;
    let (order, _, _) = placed_order_for_three_mugs(&app).await;
    let pk = publishable(&app).await;
    let token = crate::state::HostEnv::new([7; 32], None, hypha_auth::AuthSettings::default())
        .order_token(order.parse().unwrap());
    let uri = format!("/storefront/orders/{order}?token={token}");
    let (status, read) = send_as(&app, &pk, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(
        (&read["id"], &read["access_token"]),
        (&json!(order), &json!(token))
    );
    assert!(
        read["lines"][0].get("image").is_some(),
        "lines carry their image (null here)"
    );
    for wrong in ["hypha_ord_00", "nope"] {
        let uri = format!("/storefront/orders/{order}?token={wrong}");
        let (status, body) = send_as(&app, &pk, "GET", &uri, None).await;
        assert_eq!(
            (status, code(&body)),
            (StatusCode::NOT_FOUND, "order_not_found"),
            "{wrong}"
        );
    }
    let (status, body) = send_as(
        &app,
        &pk,
        "GET",
        &format!("/storefront/orders/{order}"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_query")
    );
}

#[tokio::test]
async fn orders_list_by_filters_and_total() {
    let app = app().await;
    let (order, _, _) = placed_order_for_three_mugs(&app).await;
    let found = async |query: &str| {
        let (status, page) = send(&app, "GET", &format!("/admin/orders?{query}"), None).await;
        assert_eq!(status, StatusCode::OK, "{query}: {page}");
        ids(&page).len()
    };
    assert_eq!(found("q=ANA@EXAMPLE").await, 1);
    assert_eq!(found("q=nobody").await, 0);
    assert_eq!(
        found("financial_status=pending&financial_status=paid").await,
        1
    );
    assert_eq!(found("financial_status=paid").await, 0);
    assert_eq!(
        found("fulfillment_status=unfulfilled&cancelled=false").await,
        1
    );
    assert_eq!(found("country=HR&currency=EUR&sort=total_desc").await, 1);
    assert_eq!(found("placed_from=2000-01-01T00:00:00Z").await, 1);
    assert_eq!(found("placed_before=2000-01-01T00:00:00Z").await, 0);
    let (_, page) = send(&app, "GET", "/admin/orders?sort=oldest", None).await;
    assert_eq!(ids(&page), [order.as_str()]);

    for (query, expected) in [
        (
            "sort=total_asc",
            (StatusCode::UNPROCESSABLE_ENTITY, "sort_needs_currency"),
        ),
        (
            "country=XX",
            (StatusCode::UNPROCESSABLE_ENTITY, "unknown_country_code"),
        ),
        (
            "currency=EURO",
            (StatusCode::UNPROCESSABLE_ENTITY, "unknown_currency"),
        ),
        (
            "placed_from=yesterday",
            (StatusCode::BAD_REQUEST, "invalid_query"),
        ),
        (
            "financial_status=refunded",
            (StatusCode::BAD_REQUEST, "invalid_query"),
        ),
    ] {
        let (status, body) = send(&app, "GET", &format!("/admin/orders?{query}"), None).await;
        assert_eq!((status, code(&body)), expected, "{query}");
    }
    let long = "x".repeat(256);
    let (status, body) = send(&app, "GET", &format!("/admin/orders?q={long}"), None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "field_too_long")
    );
}

#[tokio::test]
async fn customers_list_by_text_and_name() {
    let app = app().await;
    for body in [
        r#"{"email":"zed@example.com","first_name":"Ana"}"#,
        r#"{"email":"ivo@example.hr","first_name":"Ivo","last_name":"Horvat"}"#,
    ] {
        let (status, _) = send(&app, "POST", "/admin/customers", Some(body)).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, page) = send(&app, "GET", "/admin/customers?sort=name_asc&limit=1", None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["email"], "zed@example.com");
    assert_eq!(page["items"][0]["address_count"], 0);
    let cursor = page["next_cursor"].as_str().unwrap();
    let uri = format!("/admin/customers?sort=name_asc&limit=1&cursor={cursor}");
    let (_, next) = send(&app, "GET", &uri, None).await;
    assert_eq!(next["items"][0]["email"], "ivo@example.hr");
    let (_, found) = send(&app, "GET", "/admin/customers?q=horv", None).await;
    assert_eq!(found["items"].as_array().unwrap().len(), 1);
    let (status, body) = send(&app, "GET", "/admin/customers?sort=total_desc", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_query")
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
    let (_, named) = send(&app, "GET", "/admin/assets?q=SHIRT&sort=name_asc", None).await;
    assert_eq!(ids(&named), [id], "by name, sorted by it");
    let (_, none) = send(&app, "GET", "/admin/assets?q=trousers", None).await;
    assert_eq!(ids(&none), Vec::<&str>::new());

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
    let body =
        json!({ "kind": "publishable", "name": "Storefront", "channel": online_store(&app).await });
    let (status, created) = send(&app, "POST", "/admin/api-keys", Some(&body.to_string())).await;
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
        (
            StatusCode::OK,
            &json!({ "name": "Test", "description": null, "contact_email": null, "default_language": "en" })
        )
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
    let body = json!({
        "kind": "publishable",
        "name": "Storefront",
        "channel": online_store(app).await,
    });
    key(app, body).await
}

/// Publishes the products to the Online Store (new products aren't sold anywhere, CH9).
async fn publish(app: &TestApp, products: &[&str]) {
    let path = format!("/admin/channels/{}/products", online_store(app).await);
    let body = json!({ "products": products }).to_string();
    let (status, json) = send(app, "POST", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");
}

/// The channel every store starts with (migration 0036).
async fn online_store(app: &TestApp) -> String {
    let (_, channels) = send(app, "GET", "/admin/channels", None).await;
    channels["items"][0]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn channels_are_listed_created_renamed_and_publish_products() {
    let app = app().await;
    let (status, channels) = send(&app, "GET", "/admin/channels", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&channels["items"][0]["name"], &channels["items"][0]["kind"]),
        (&json!("Online Store"), &json!("online_store"))
    );

    let body = r#"{"name":"Outlet","kind":"custom"}"#;
    let (status, outlet) = send(&app, "POST", "/admin/channels", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{outlet}");
    let (status, body) = send(
        &app,
        "POST",
        "/admin/channels",
        Some(r#"{"name":"OUTLET","kind":"custom"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "channel_name_taken")
    );
    let outlet_url = format!("/admin/channels/{}", outlet["id"].as_str().unwrap());
    let (status, renamed) = send(
        &app,
        "PATCH",
        &outlet_url,
        Some(r#"{"name":"Outlet store"}"#),
    )
    .await;
    assert_eq!(
        (status, &renamed["name"]),
        (StatusCode::OK, &json!("Outlet store"))
    );
    let missing = "/admin/channels/chan_00000000000000000000000000000009";
    let (status, body) = send(&app, "GET", missing, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "channel_not_found")
    );

    let mug = create_product(&app, "mug").await;
    let body = json!({ "products": [mug] }).to_string();
    let (status, _) = send(&app, "POST", &format!("{outlet_url}/products"), Some(&body)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let channels = json!({ "channels": [online_store(&app).await] }).to_string();
    let (status, set) = send(
        &app,
        "PUT",
        &format!("/admin/products/{mug}/channels"),
        Some(&channels),
    )
    .await;
    assert_eq!(
        (status, &set["channels"]),
        (StatusCode::OK, &json!([online_store(&app).await]))
    );
    let (status, body) = send(
        &app,
        "PUT",
        "/admin/products/prod_00000000000000000000000000000009/channels",
        Some(&channels),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "product_not_found")
    );
    let unknown = r#"{"channels":["chan_00000000000000000000000000000009"]}"#;
    let (status, body) = send(
        &app,
        "PUT",
        &format!("/admin/products/{mug}/channels"),
        Some(unknown),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "channel_not_found")
    );
}

#[tokio::test]
async fn a_publishable_key_needs_a_channel_and_a_secret_key_none() {
    let app = app().await;
    let (status, body) = send(
        &app,
        "POST",
        "/admin/api-keys",
        Some(r#"{"kind":"publishable","name":"Web"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "api_key_channel_required")
    );
    let unknown =
        r#"{"kind":"publishable","name":"Web","channel":"chan_00000000000000000000000000000009"}"#;
    let (status, body) = send(&app, "POST", "/admin/api-keys", Some(unknown)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "channel_not_found")
    );
    let body = json!({ "kind": "secret", "name": "Server", "scopes": ["products:read"], "channel": online_store(&app).await }).to_string();
    let (status, body) = send(&app, "POST", "/admin/api-keys", Some(&body)).await;
    assert_eq!(
        (status, code(&body)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "api_key_channel_not_allowed"
        )
    );
    let body = json!({ "kind": "publishable", "name": "Web", "channel": online_store(&app).await })
        .to_string();
    let (status, created) = send(&app, "POST", "/admin/api-keys", Some(&body)).await;
    assert_eq!(
        (status, &created["channel"]),
        (StatusCode::CREATED, &json!(online_store(&app).await))
    );
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
    // So does an active product the key's channel doesn't sell.
    let (status, json) = send_as(&app, &pk, "GET", &path, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "product_not_found")
    );

    publish(&app, &[id]).await;
    let (status, json) = send_as(&app, &pk, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["handle"], "mug");
    assert_eq!(
        (&json["description"], &json["tags"]),
        (&json!(null), &json!([]))
    );
    assert!(json.get("status").is_none());
    // No prices without a country: the base price isn't what a shopper in a market pays.
    assert!(json["variants"][0]["id"].is_string());
    assert_eq!(json["variants"][0]["price"], Value::Null);
    assert_eq!(json["sold_in_country"], Value::Null);
}

#[tokio::test]
async fn a_cart_sells_only_what_its_channel_publishes() {
    let app = app().await;
    let mug = shop_selling_a_mug(&app).await;
    let (_, mugs) = send(&app, "GET", "/admin/products?limit=1", None).await;
    let product = mugs["items"][0]["id"].as_str().unwrap().to_string();
    let pk = publishable(&app).await;
    let (_, cart) = send_as(
        &app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let lines = format!("/storefront/carts/{}/lines", cart["id"].as_str().unwrap());
    let line = json!({"variant": mug, "quantity": 1}).to_string();
    let (status, json) = send_as(&app, &pk, "POST", &lines, Some(&line)).await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let path = format!("/admin/channels/{}/products", online_store(&app).await);
    let body = json!({ "products": [product] }).to_string();
    send(&app, "DELETE", &path, Some(&body)).await;
    // The line already in the cart is unavailable; adding it again is refused.
    assert_eq!(json["lines"][0]["status"], "priced", "{json}");
    assert!(
        json["lines"][0]["handle"].is_string(),
        "links to the product"
    );
    let cart_path = lines.trim_end_matches("/lines");
    let (_, json) = send_as(&app, &pk, "GET", cart_path, None).await;
    let line_now = &json["lines"][0];
    assert_eq!(
        (&line_now["status"], &line_now["reason_code"]),
        (&json!("unavailable"), &json!("product_not_published"))
    );
    let (status, json) = send_as(&app, &pk, "POST", &lines, Some(&line)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "product_not_published")
    );

    // Another channel's storefront doesn't see the cart at all.
    let (_, outlet) = send(
        &app,
        "POST",
        "/admin/channels",
        Some(r#"{"name":"Outlet","kind":"custom"}"#),
    )
    .await;
    let body = json!({"kind": "publishable", "name": "Outlet", "channel": outlet["id"]});
    let outlet_key = key(&app, body).await;
    for (method, path, body) in [
        ("GET", cart_path, None),
        ("POST", lines.as_str(), Some(line.as_str())),
    ] {
        let (status, json) = send_as(&app, &outlet_key, method, path, body).await;
        assert_eq!(
            (status, code(&json)),
            (StatusCode::NOT_FOUND, "cart_not_found"),
            "{method} {path}"
        );
    }
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
async fn a_revoked_publishable_key_stops_at_once_though_it_was_cached() {
    let app = app().await;
    let token = publishable(&app).await;
    // Used once: its lookup is now cached (key_cache.rs).
    assert_eq!(
        send_as(&app, &token, "GET", "/storefront/store", None)
            .await
            .0,
        StatusCode::OK
    );
    let (_, keys) = send(&app, "GET", "/admin/api-keys", None).await;
    let id = keys["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["kind"] == "publishable")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = send(&app, "POST", &format!("/admin/api-keys/{id}/revoke"), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send_as(&app, &token, "GET", "/storefront/store", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
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

    // The lists staff pages read (design note 0024, AU6).
    let (status, roles) = send(&app, "GET", "/admin/roles", None).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&Value> = roles["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["name"])
        .collect();
    assert_eq!(names, [&json!("Owner"), &json!("Support")]);
    let (_, invites) = send(&app, "GET", "/admin/invites", None).await;
    assert_eq!(ids(&invites), [invited["id"].as_str().unwrap()]);
    let (_, users) = send(&app, "GET", "/admin/users", None).await;
    assert_eq!(users["items"].as_array().unwrap().len(), 1, "the owner");

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
    let (burst, every) = crate::rate_limit::Rule::LoginIp.quota(&Default::default());
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
    let app = app_behind_proxy().await;
    let (burst, _) = crate::rate_limit::Rule::LoginEmail.quota(&Default::default());
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
    let app = app_behind_proxy().await;
    let (burst, _) = crate::rate_limit::Rule::LoginEmailFromIp.quota(&Default::default());
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
    let (burst, _) = crate::rate_limit::Rule::ResetIp.quota(&Default::default());
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
    let (burst, _) = crate::rate_limit::Rule::LoginIp.quota(&Default::default());
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
    let app = app_behind_proxy().await;
    let (burst, _) = crate::rate_limit::Rule::ResetEmailFromIp.quota(&Default::default());
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
    let (burst, _) = crate::rate_limit::Rule::InviteIp.quota(&Default::default());
    for _ in 0..burst {
        send_with(&app, &[], "POST", "/admin/invites/accept", Some("{")).await;
    }
    let (status, json) = send_with(&app, &[], "POST", "/admin/invites/accept", Some("{")).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    );
}

// ---------- shipping (design note 0015) ----------

#[tokio::test]
async fn shipping_zones_and_options_over_http() {
    let app = app().await;
    let (status, zone) = send(
        &app,
        "POST",
        "/admin/shipping-zones",
        Some(r#"{"name":"Croatia","countries":["HR"]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{zone}");
    let zone_path = format!("/admin/shipping-zones/{}", zone["id"].as_str().unwrap());

    let (status, json) = send(
        &app,
        "POST",
        "/admin/shipping-zones",
        Some(r#"{"name":"Balkans","countries":["HR","SI"]}"#),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "country_in_other_zone")
    );

    // Free over 50 EUR, else 4.90.
    let free_over = json!({
        "name": "Standard", "code": "standard",
        "rate": {"type": "builtin", "basis": "subtotal", "tables": [
            {"currency": "EUR", "tiers": [{"up_to": 5000, "amount": 490}, {"amount": 0}]}
        ]},
        "estimate": {"min_days": 2, "max_days": 4}
    });
    let options = format!("{zone_path}/options");
    let (status, json) = send(&app, "POST", &options, Some(&free_over.to_string())).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(
        json["options"][0]["rate"]["tables"][0]["tiers"][1]["up_to"],
        Value::Null
    );
    assert_eq!(json["options"][0]["enabled"], true);
    let standard = json["options"][0]["id"].as_str().unwrap().to_string();

    let express = json!({
        "name": "Express", "code": "express",
        "rate": {"type": "provider", "provider": "app_courier", "service": "express"}
    });
    let (_, json) = send(&app, "POST", &options, Some(&express.to_string())).await;
    let express = json["options"][1]["id"].as_str().unwrap().to_string();

    let unbounded_tier = json!({
        "name": "Bad", "code": "bad",
        "rate": {"type": "builtin", "basis": "weight", "tables": [
            {"currency": "EUR", "tiers": [{"up_to": 1000, "amount": 490}]}
        ]}
    });
    let (status, json) = send(&app, "POST", &options, Some(&unbounded_tier.to_string())).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_shipping_rate")
    );
    let negative = json!({
        "name": "Bad", "code": "bad",
        "rate": {"type": "builtin", "basis": "flat", "tables": [
            {"currency": "EUR", "tiers": [{"amount": -1}]}
        ]}
    });
    let (status, json) = send(&app, "POST", &options, Some(&negative.to_string())).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "negative_amount")
    );

    let order = json!({ "options": [express, standard] }).to_string();
    let (status, json) = send(&app, "PUT", &format!("{options}/order"), Some(&order)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["options"][0]["code"], "express");

    // `null` clears the estimate; absent fields stay.
    let one = format!("{options}/{standard}");
    let (status, json) = send(&app, "PATCH", &one, Some(r#"{"estimate":null}"#)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["options"][1]["estimate"], Value::Null);
    assert_eq!(json["options"][1]["name"], "Standard");

    let missing = format!("{options}/shipopt_{}", "0".repeat(32));
    let (status, json) = send(&app, "PATCH", &missing, Some(r#"{"enabled":false}"#)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "shipping_option_not_found")
    );

    let (status, json) = send(&app, "DELETE", &one, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["options"].as_array().unwrap().len(), 1);

    let (status, json) = send(&app, "GET", "/admin/shipping-zones", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["items"][0]["countries"], json!(["HR"]));

    let (status, _) = send(&app, "DELETE", &zone_path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, json) = send(&app, "GET", &zone_path, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "shipping_zone_not_found")
    );
}

#[tokio::test]
async fn shipping_setup_needs_shipping_scopes() {
    let app = app().await;
    let read_only = key(
        &app,
        json!({"kind": "secret", "name": "Reader", "scopes": ["shipping:read"]}),
    )
    .await;
    let (status, _) = send_as(&app, &read_only, "GET", "/admin/shipping-zones", None).await;
    assert_eq!(status, StatusCode::OK);
    let body = r#"{"name":"Croatia","countries":["HR"]}"#;
    let (status, json) = send_as(
        &app,
        &read_only,
        "POST",
        "/admin/shipping-zones",
        Some(body),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
}

/// What the fake rate app saw, per request: the signature, the request ID and the body.
type Seen = Arc<std::sync::Mutex<Vec<(String, String, Vec<u8>)>>>;

/// One "express" rate at 7 EUR, as a rate app answers.
fn express_rate() -> String {
    json!({"rates": [{
        "service": "express", "name": "Courier Express",
        "amount": {"amount": 700, "currency": "EUR"},
        "estimate": {"min_days": 1, "max_days": 1},
        "reference": "quote-42"
    }]})
    .to_string()
}

/// A rate app on a free local port that answers every POST with `answer`.
async fn rate_app(answer: String) -> (String, Seen) {
    use axum::{body::Bytes, http::HeaderMap, routing::post};
    let seen: Seen = Default::default();
    let log = Arc::clone(&seen);
    let app = Router::new().route(
        "/rates",
        post(move |headers: HeaderMap, body: Bytes| {
            let log = Arc::clone(&log);
            let answer = answer.clone();
            async move {
                let header = |name: &str| {
                    headers
                        .get(name)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string()
                };
                log.lock().unwrap().push((
                    header(crate::outbound::SIGNATURE_HEADER),
                    header(crate::shipping::REQUEST_ID_HEADER),
                    body.to_vec(),
                ));
                ([(CONTENT_TYPE, "application/json")], answer)
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/rates", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });
    (url, seen)
}

/// Creates an app from `body`; returns its secret.
async fn create_app(app: &TestApp, body: Value) -> String {
    let (status, created) = send(app, "POST", "/admin/apps", Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let secret = created["secret"].as_str().unwrap().to_string();
    assert!(secret.starts_with(hypha_apps::SECRET_PREFIX));
    secret
}

/// App `key` as a rate provider calling `callback`; returns the app's secret.
async fn rate_provider(app: &TestApp, key: &str, callback: &str) -> String {
    let secret = create_app(app, json!({"key": key, "name": key})).await;
    let body = json!({"key": key, "callback_url": callback});
    let (status, created) = send(
        app,
        "POST",
        "/admin/shipping-providers",
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    secret
}

/// A market for HR in EUR selling one active 10 EUR mug; returns the mug's variant.
async fn shop_selling_a_mug(app: &TestApp) -> String {
    market_selling_a_mug(app).await.1
}

/// `shop_selling_a_mug`, with the market's ID too.
async fn market_selling_a_mug(app: &TestApp) -> (String, String) {
    let (_, market) = send(
        app,
        "POST",
        "/admin/markets",
        Some(r#"{"name":"Croatia","countries":["HR"],"currency":"EUR"}"#),
    )
    .await;
    let (_, mug) = send(
        app,
        "POST",
        "/admin/products",
        Some(r#"{"handle":"mug","title":"Mug","price":{"amount":1000,"currency":"EUR"}}"#),
    )
    .await;
    let mug_id = mug["id"].as_str().unwrap();
    send(
        app,
        "PATCH",
        &format!("/admin/products/{mug_id}/status"),
        Some(r#"{"status":"active"}"#),
    )
    .await;
    let body = json!({ "market": market["id"] }).to_string();
    let (_, catalog) = send(app, "POST", "/admin/catalogs", Some(&body)).await;
    let path = format!(
        "/admin/catalogs/{}/products",
        catalog["id"].as_str().unwrap()
    );
    let body = json!({ "products": [mug_id] }).to_string();
    let (status, _) = send(app, "POST", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    publish(app, &[mug_id]).await;
    (
        market["id"].as_str().unwrap().to_string(),
        mug["variants"][0]["id"].as_str().unwrap().to_string(),
    )
}

#[tokio::test]
async fn a_cart_ships_by_an_app_providers_signed_quote() {
    let app = app().await;
    let mug = shop_selling_a_mug(&app).await;
    let (callback, seen) = rate_app(express_rate()).await;

    let secret = rate_provider(&app, "app_courier", &callback).await;
    let (_, fetched) = send(&app, "GET", "/admin/shipping-providers/app_courier", None).await;
    assert!(fetched.get("secret").is_none());

    let (_, zone) = send(
        &app,
        "POST",
        "/admin/shipping-zones",
        Some(r#"{"name":"Croatia","countries":["HR"]}"#),
    )
    .await;
    let options = format!(
        "/admin/shipping-zones/{}/options",
        zone["id"].as_str().unwrap()
    );
    let express = json!({"name": "Express", "code": "express",
        "rate": {"type": "provider", "provider": "app_courier", "service": "express"}});
    send(&app, "POST", &options, Some(&express.to_string())).await;
    let backup = json!({"name": "Post", "code": "post", "is_backup": true,
        "rate": {"type": "builtin", "basis": "flat", "tables": [
            {"currency": "EUR", "tiers": [{"amount": 500}]}]}});
    let (_, zone) = send(&app, "POST", &options, Some(&backup.to_string())).await;
    let express_id = zone["options"][0]["id"].as_str().unwrap().to_string();

    let pk = publishable(&app).await;
    let (_, cart) = send_as(
        &app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let cart_path = format!("/storefront/carts/{}", cart["id"].as_str().unwrap());
    let line = json!({"variant": mug, "quantity": 2}).to_string();
    let (_, cart) = send_as(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/lines"),
        Some(&line),
    )
    .await;
    let group = cart["shipping_groups"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(cart["shipping_ready"], false);

    let (status, json) = send_as(
        &app,
        &pk,
        "GET",
        &format!("{cart_path}/shipping-options"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "shipping_address_required")
    );
    let address = |country: &str| {
        json!({"address": {"last_name": "Horvat", "line1": "Ilica 1", "city": "Zagreb",
            "country": country}})
        .to_string()
    };
    let set_address = format!("{cart_path}/shipping-address");
    let (status, json) = send_as(&app, &pk, "PUT", &set_address, Some(&address("SI"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    let (status, json) = send_as(&app, &pk, "PUT", &set_address, Some(&address("HR"))).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["shipping_address"]["city"], "Zagreb");

    // The provider quotes; the backup stays hidden while it does.
    let (status, offers) = send_as(
        &app,
        &pk,
        "GET",
        &format!("{cart_path}/shipping-options"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{offers}");
    let group_offers = &offers["groups"][0];
    assert_eq!(group_offers["group"], group.as_str());
    assert_eq!(group_offers["offers"].as_array().unwrap().len(), 1);
    assert_eq!(
        group_offers["offers"][0]["amount"],
        json!({"amount": 700, "currency": "EUR"})
    );

    // Every call was signed with the secret over "<t>.<body>".
    {
        let seen = seen.lock().unwrap();
        let (signature, request_id, body) = seen.last().unwrap();
        let at: i64 = signature
            .strip_prefix("t=")
            .and_then(|s| s.split(',').next())
            .and_then(|t| t.parse().ok())
            .unwrap();
        assert_eq!(*signature, crate::outbound::sign(&secret, at, body));
        let sent: Value = serde_json::from_slice(body).unwrap();
        assert_eq!(sent["request"]["destination"]["city"], "Zagreb");
        assert_eq!(sent["request"]["items"][0]["quantity"], 2);
        // One ID for the request, in the header and the body.
        assert!(request_id.starts_with("rreq_"));
        assert_eq!(sent["id"], request_id.as_str());
    }

    let select = format!("{cart_path}/shipping/{group}");
    let unknown = json!({"option": format!("shipopt_{}", "0".repeat(32))}).to_string();
    let (status, json) = send_as(&app, &pk, "PUT", &select, Some(&unknown)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "shipping_option_unknown")
    );
    let pick = json!({"option": express_id}).to_string();
    let (status, cart) = send_as(&app, &pk, "PUT", &select, Some(&pick)).await;
    assert_eq!(status, StatusCode::OK, "{cart}");
    assert_eq!(cart["shipping_ready"], true);
    assert_eq!(
        cart["shipping_total"],
        json!({"amount": 700, "currency": "EUR"})
    );
    assert_eq!(cart["total"], json!({"amount": 2700, "currency": "EUR"}));
    assert!(cart["shipping_groups"][0]["selected"]["quote_expires_at"].is_string());

    send_as(
        &app,
        &pk,
        "PUT",
        &format!("{cart_path}/email"),
        Some(r#"{"email":"a@example.hr"}"#),
    )
    .await;
    let (status, order) = send_as(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/checkout"),
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{order}");
    let shipped = &order["shipping_lines"][0];
    assert_eq!(shipped["provider"], "app_courier");
    assert_eq!(shipped["reference"], "quote-42");
    assert_eq!(shipped["amount"], json!({"amount": 700, "currency": "EUR"}));
    assert_eq!(
        order["shipping_total"],
        json!({"amount": 700, "currency": "EUR"})
    );
    assert_eq!(order["total"], json!({"amount": 2700, "currency": "EUR"}));
}

#[tokio::test]
async fn a_failing_provider_brings_out_the_backup() {
    let app = app().await;
    let mug = shop_selling_a_mug(&app).await;
    // Nothing listens on port 9 (discard) here: the connection is refused.
    rate_provider(&app, "app_down", "http://127.0.0.1:9/rates").await;
    let (status, json) = send(
        &app,
        "GET",
        "/admin/shipping-providers/app_down/services",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::BAD_GATEWAY, "shipping_provider_failed")
    );

    let (_, zone) = send(
        &app,
        "POST",
        "/admin/shipping-zones",
        Some(r#"{"name":"Croatia","countries":["HR"]}"#),
    )
    .await;
    let options = format!(
        "/admin/shipping-zones/{}/options",
        zone["id"].as_str().unwrap()
    );
    let express = json!({"name": "Express", "code": "express",
        "rate": {"type": "provider", "provider": "app_down", "service": "express"}});
    send(&app, "POST", &options, Some(&express.to_string())).await;
    let backup = json!({"name": "Post", "code": "post", "is_backup": true,
        "rate": {"type": "builtin", "basis": "flat", "tables": [
            {"currency": "EUR", "tiers": [{"amount": 500}]}]}});
    send(&app, "POST", &options, Some(&backup.to_string())).await;

    let pk = publishable(&app).await;
    let (_, cart) = send_as(
        &app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let cart_path = format!("/storefront/carts/{}", cart["id"].as_str().unwrap());
    let line = json!({"variant": mug, "quantity": 1}).to_string();
    send_as(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/lines"),
        Some(&line),
    )
    .await;
    let address =
        r#"{"address":{"last_name":"Horvat","line1":"Ilica 1","city":"Zagreb","country":"HR"}}"#;
    send_as(
        &app,
        &pk,
        "PUT",
        &format!("{cart_path}/shipping-address"),
        Some(address),
    )
    .await;

    let (status, offers) = send_as(
        &app,
        &pk,
        "GET",
        &format!("{cart_path}/shipping-options"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{offers}");
    let group = &offers["groups"][0];
    assert_eq!(group["offers"][0]["code"], "post");
    assert_eq!(group["unavailable"][0]["reason"], "provider_failed");
}

#[tokio::test]
async fn an_oversized_answer_fails_the_rate_app() {
    let app = app().await;
    let mug = shop_selling_a_mug(&app).await;
    let huge = format!(
        r#"{{"rates": [], "padding": "{}"}}"#,
        "x".repeat(300 * 1024)
    );
    let (callback, _) = rate_app(huge).await;
    rate_provider(&app, "app_big", &callback).await;
    let (_, zone) = send(
        &app,
        "POST",
        "/admin/shipping-zones",
        Some(r#"{"name":"Croatia","countries":["HR"]}"#),
    )
    .await;
    let options = format!(
        "/admin/shipping-zones/{}/options",
        zone["id"].as_str().unwrap()
    );
    let express = json!({"name": "Express", "code": "express",
        "rate": {"type": "provider", "provider": "app_big", "service": "express"}});
    send(&app, "POST", &options, Some(&express.to_string())).await;

    let pk = publishable(&app).await;
    let (_, cart) = send_as(
        &app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let cart_path = format!("/storefront/carts/{}", cart["id"].as_str().unwrap());
    let line = json!({"variant": mug, "quantity": 1}).to_string();
    send_as(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/lines"),
        Some(&line),
    )
    .await;
    let address =
        r#"{"address":{"last_name":"Horvat","line1":"Ilica 1","city":"Zagreb","country":"HR"}}"#;
    send_as(
        &app,
        &pk,
        "PUT",
        &format!("{cart_path}/shipping-address"),
        Some(address),
    )
    .await;
    let (status, offers) = send_as(
        &app,
        &pk,
        "GET",
        &format!("{cart_path}/shipping-options"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{offers}");
    assert_eq!(
        offers["groups"][0]["unavailable"][0]["reason"],
        "provider_failed"
    );
}

#[tokio::test]
async fn asking_for_shipping_options_is_rate_limited_per_cart() {
    let app = app().await;
    let pk = publishable(&app).await;
    let cart = |n: u8| format!("/storefront/carts/cart_{}", format!("{n:x}").repeat(32));
    for _ in 0..10 {
        let path = format!("{}/shipping-options", cart(1));
        let (status, json) = send_as(&app, &pk, "GET", &path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    }
    // Checkout may renew quotes too, so it counts against the same cart.
    let (status, json) = send_as(
        &app,
        &pk,
        "POST",
        &format!("{}/checkout", cart(1)),
        Some("{}"),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    );
    // Another cart, from the same address (a server-rendered storefront), is unaffected.
    let path = format!("{}/shipping-options", cart(2));
    let (status, json) = send_as(&app, &pk, "GET", &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
}

#[tokio::test]
async fn creating_carts_is_rate_limited_per_address() {
    let app = app().await;
    let pk = publishable(&app).await;
    // Counted before the body is read: an invalid one costs the same.
    for _ in 0..20 {
        let (status, json) = send_as(&app, &pk, "POST", "/storefront/carts", Some("{}")).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    }
    let (status, json) = send_as(&app, &pk, "POST", "/storefront/carts", Some("{}")).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    );
}

#[tokio::test]
async fn a_callback_url_cannot_hide_its_host_behind_credentials() {
    let app = app().await;
    create_app(&app, json!({"key": "app_sneaky", "name": "Sneaky"})).await;
    let body =
        json!({"key": "app_sneaky", "callback_url": "http://localhost:80@evil.example/rates"});
    let (status, json) = send(
        &app,
        "POST",
        "/admin/shipping-providers",
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_callback_url")
    );
}

#[tokio::test]
async fn a_provider_needs_its_app() {
    let app = app().await;
    let body = json!({"key": "app_ghost", "callback_url": "https://ghost.example/rates"});
    let (status, json) = send(
        &app,
        "POST",
        "/admin/shipping-providers",
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "provider_app_not_found")
    );
}

// ---------- apps and webhooks (design note 0016) ----------

#[tokio::test]
async fn apps_are_created_changed_and_deleted() {
    let app = app().await;
    let secret = create_app(
        &app,
        json!({"key": "app_erp", "name": "ERP", "subscriptions": ["order.*"],
            "webhook": {"url": "https://erp.example/hooks"}}),
    )
    .await;
    let (status, fetched) = send(&app, "GET", "/admin/apps/app_erp", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(fetched.get("secret").is_none());
    assert_eq!(
        fetched["webhook"],
        json!({"url": "https://erp.example/hooks", "timeout_ms": 10000})
    );
    assert_eq!(fetched["subscriptions"], json!(["order.*"]));

    let (status, json) = send(
        &app,
        "POST",
        "/admin/apps",
        Some(r#"{"key":"app_erp","name":"Again"}"#),
    )
    .await;
    assert_eq!((status, code(&json)), (StatusCode::CONFLICT, "app_exists"));
    let (status, json) = send(
        &app,
        "POST",
        "/admin/apps",
        Some(r#"{"key":"app_bad","name":"Bad","subscriptions":["Orders"]}"#),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_subscription")
    );

    // Merge patch: `webhook: null` removes it, absent fields stay.
    let (status, patched) = send(
        &app,
        "PATCH",
        "/admin/apps/app_erp",
        Some(r#"{"webhook":null,"enabled":false}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(
        (&patched["webhook"], &patched["enabled"], &patched["name"]),
        (&Value::Null, &json!(false), &json!("ERP"))
    );

    let (status, rotated) = send(&app, "POST", "/admin/apps/app_erp/secret", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(rotated["secret"].as_str().unwrap(), secret);

    // Deleting the app takes its rate settings with it.
    let body = json!({"key": "app_erp", "callback_url": "https://erp.example/rates"});
    send(
        &app,
        "POST",
        "/admin/shipping-providers",
        Some(&body.to_string()),
    )
    .await;
    let (status, _) = send(&app, "DELETE", "/admin/apps/app_erp", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, json) = send(&app, "GET", "/admin/apps/app_erp", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "app_not_found")
    );
    let (status, _) = send(&app, "GET", "/admin/shipping-providers/app_erp", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, list) = send(&app, "GET", "/admin/apps", None).await;
    assert_eq!(list["items"], json!([]));
}

/// What a webhook receiver saw: signature, event ID, event type, delivery ID, body.
type Hooks = Arc<std::sync::Mutex<Vec<(String, String, String, String, Vec<u8>)>>>;

/// A local webhook receiver answering `status`; its URL and what it saw.
async fn webhook_receiver(status: StatusCode) -> (String, Hooks) {
    use axum::{body::Bytes, http::HeaderMap, routing::post};
    let seen: Hooks = Default::default();
    let log = Arc::clone(&seen);
    let app = Router::new().route(
        "/hooks",
        post(move |headers: HeaderMap, body: Bytes| {
            let log = Arc::clone(&log);
            async move {
                let header = |name: &str| {
                    headers
                        .get(name)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string()
                };
                log.lock().unwrap().push((
                    header(crate::outbound::SIGNATURE_HEADER),
                    header(crate::webhooks::EVENT_ID_HEADER),
                    header(crate::webhooks::EVENT_TYPE_HEADER),
                    header(crate::webhooks::DELIVERY_ID_HEADER),
                    body.to_vec(),
                ));
                status
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hooks", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });
    (url, seen)
}

#[tokio::test]
async fn a_subscribed_app_gets_a_signed_webhook() {
    let app = app().await;
    let (url, seen) = webhook_receiver(StatusCode::NO_CONTENT).await;
    let secret = create_app(
        &app,
        json!({"key": "app_erp", "name": "ERP", "subscriptions": ["product.created"],
            "webhook": {"url": url}}),
    )
    .await;
    let product = create_product(&app, "mug").await;
    crate::jobs::dispatch(&app.state).await.unwrap();

    let (signature, event_id, event_type, delivery_id, body) = {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "one event, one webhook");
        seen[0].clone()
    };
    let at: i64 = signature
        .strip_prefix("t=")
        .and_then(|s| s.split(',').next())
        .and_then(|t| t.parse().ok())
        .unwrap();
    assert_eq!(signature, crate::outbound::sign(&secret, at, &body));
    let sent: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(event_type, "product.created");
    assert_eq!(sent["id"], event_id.as_str());
    assert_eq!(sent["type"], "product.created");
    assert_eq!(sent["subject"]["id"], product.as_str());
    assert_eq!(sent["subject"]["version"], 1);

    let (status, page) = send(&app, "GET", "/admin/apps/app_erp/deliveries", None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let delivery = &page["items"][0];
    assert_eq!(delivery["id"], delivery_id.as_str());
    assert_eq!(delivery["event_id"], event_id.as_str());
    assert_eq!(
        (&delivery["status"], &delivery["attempts"]),
        (&json!("succeeded"), &json!(1))
    );

    // The event as sent, for debugging.
    let (status, event) = send(&app, "GET", &format!("/admin/events/{event_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(event, sent);
}

#[tokio::test]
async fn a_failed_webhook_is_listed_and_retried() {
    let app = app().await;
    let (url, seen) = webhook_receiver(StatusCode::SERVICE_UNAVAILABLE).await;
    create_app(
        &app,
        json!({"key": "app_erp", "name": "ERP", "subscriptions": ["product.*"],
            "webhook": {"url": url}}),
    )
    .await;
    create_product(&app, "mug").await;
    crate::jobs::dispatch(&app.state).await.unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1);

    let (_, page) = send(
        &app,
        "GET",
        "/admin/apps/app_erp/deliveries?status=pending",
        None,
    )
    .await;
    let delivery = page["items"][0].clone();
    assert_eq!(delivery["last_error"], "answered HTTP 503");
    let (_, none) = send(
        &app,
        "GET",
        "/admin/apps/app_erp/deliveries?status=succeeded",
        None,
    )
    .await;
    assert_eq!(none["items"], json!([]));

    // Retried: due now, so the next run sends it again without waiting its minute.
    let retry = format!(
        "/admin/apps/app_erp/deliveries/{}/retry",
        delivery["id"].as_str().unwrap()
    );
    let (status, retried) = send(&app, "POST", &retry, None).await;
    assert_eq!(status, StatusCode::OK, "{retried}");
    assert_eq!(retried["status"], "pending");
    crate::jobs::dispatch(&app.state).await.unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);

    let unknown = format!(
        "/admin/apps/app_erp/deliveries/{}/retry",
        hypha_core::DeliveryId::from_uuid(uuid::Uuid::from_u128(7))
    );
    let (status, json) = send(&app, "POST", &unknown, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "delivery_not_found")
    );
    let missing = format!(
        "/admin/events/{}",
        hypha_core::EventId::from_uuid(uuid::Uuid::from_u128(7))
    );
    let (status, json) = send(&app, "GET", &missing, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "event_not_found")
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

/// A value matching a path parameter's schema in `doc`: its example if it has one, an
/// ID from its prefix pattern (`^prod_[0-9a-f]{32}$`), or a country code.
fn example_for(doc: &Value, param: &Value) -> String {
    if let Some(example) = param["example"].as_str() {
        return example.to_string();
    }
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

// ---------- fulfilment (design note 0018) ----------

/// A shop selling mugs that need no shipping, 10 at the warehouse, and an order for 3
/// placed through the storefront. Returns (order ID, its line ID, the warehouse's ID).
async fn placed_order_for_three_mugs(app: &TestApp) -> (String, String, String) {
    let (market, mug) = market_selling_a_mug(app).await;
    let (_, mugs) = send(app, "GET", "/admin/products?limit=1", None).await;
    let product = mugs["items"][0]["id"].as_str().unwrap().to_string();
    let (status, json) = send(
        app,
        "PATCH",
        &format!("/admin/products/{product}/variants/{mug}"),
        Some(r#"{"requires_shipping":false}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let body = json!({"name": "Warehouse", "markets": [market]}).to_string();
    let (_, location) = send(app, "POST", "/admin/locations", Some(&body)).await;
    let location = location["id"].as_str().unwrap().to_string();
    let body = json!({ "variant": mug }).to_string();
    let (_, item) = send(app, "POST", "/admin/inventory-items", Some(&body)).await;
    let level = format!(
        "/admin/inventory-items/{}/levels/{location}",
        item["id"].as_str().unwrap()
    );
    let (status, json) = send(app, "PUT", &level, Some(r#"{"on_hand":10}"#)).await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let pk = publishable(app).await;
    let (_, cart) = send_as(
        app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let cart_path = format!("/storefront/carts/{}", cart["id"].as_str().unwrap());
    let line = json!({"variant": mug, "quantity": 3}).to_string();
    send_as(app, &pk, "POST", &format!("{cart_path}/lines"), Some(&line)).await;
    send_as(
        app,
        &pk,
        "PUT",
        &format!("{cart_path}/email"),
        Some(r#"{"email":"ana@example.hr"}"#),
    )
    .await;
    let (status, order) = send_as(
        app,
        &pk,
        "POST",
        &format!("{cart_path}/checkout"),
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{order}");
    assert_eq!(order["channel"], online_store(app).await.as_str());
    let (_, orders) = send(app, "GET", "/admin/orders", None).await;
    assert_eq!(orders["items"][0]["channel"], order["channel"]);
    (
        order["id"].as_str().unwrap().to_string(),
        order["lines"][0]["id"].as_str().unwrap().to_string(),
        location,
    )
}

#[tokio::test]
async fn an_order_is_fulfilled_and_taken_back_over_http() {
    let app = app().await;
    let (order, line, warehouse) = placed_order_for_three_mugs(&app).await;
    let order_path = format!("/admin/orders/{order}");

    // Everything waits at the warehouse.
    let (status, json) = send(&app, "GET", &order_path, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["fulfillment_status"], "unfulfilled");
    assert_eq!(
        json["unfulfilled"],
        json!([{"line": line, "location": warehouse, "quantity": 3}])
    );
    assert_eq!(json["lines"][0]["fulfilled_quantity"], 0);

    let ship = |quantity: u32| {
        json!({"location": warehouse, "lines": [{"line": line, "quantity": quantity}],
            "tracking": [{"carrier": "DHL", "number": "JD1", "url": "https://dhl.example/JD1"}],
            "notify": true})
        .to_string()
    };
    let fulfillments = format!("{order_path}/fulfillments");
    let (status, shipped) = send(&app, "POST", &fulfillments, Some(&ship(2))).await;
    assert_eq!(status, StatusCode::CREATED, "{shipped}");
    assert_eq!(shipped["status"], "shipped");
    assert_eq!(shipped["lines"], json!([{"line": line, "quantity": 2}]));
    assert_eq!(shipped["tracking"][0]["number"], "JD1");
    assert!(shipped["id"].as_str().unwrap().starts_with("ful_"));

    let (_, json) = send(&app, "GET", &order_path, None).await;
    assert_eq!(json["fulfillment_status"], "partially_fulfilled");
    assert_eq!(json["lines"][0]["fulfilled_quantity"], 2);
    assert_eq!(json["unfulfilled"][0]["quantity"], 1);

    // More than is left: what's left comes back to act on.
    let (status, json) = send(&app, "POST", &fulfillments, Some(&ship(2))).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "over_fulfilled")
    );
    assert_eq!(
        json["details"],
        json!({"line": line, "requested": 2, "unfulfilled": 1})
    );

    // The tracking changes; the list shows it.
    let fulfillment = format!("/admin/fulfillments/{}", shipped["id"].as_str().unwrap());
    let tracking = r#"{"tracking":[{"number":"JD2"}],"notify":true}"#;
    let (status, json) = send(&app, "PATCH", &fulfillment, Some(tracking)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["tracking"],
        json!([{"carrier": null, "number": "JD2", "url": null}])
    );
    let (_, list) = send(&app, "GET", &fulfillments, None).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["tracking"][0]["number"], "JD2");

    // Standing fulfilments block cancelling the order.
    let cancel = format!("{order_path}/cancel");
    let reason = r#"{"reason":"customer"}"#;
    let (status, json) = send(&app, "POST", &cancel, Some(reason)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "order_has_fulfillments")
    );
    assert_eq!(json["details"], json!({"fulfillments": [shipped["id"]]}));

    // Taken back: its units are reserved again.
    let (status, json) = send(&app, "POST", &format!("{fulfillment}/cancel"), None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "cancelled");
    assert!(json["cancelled_at"].is_string());
    let (_, json) = send(&app, "GET", &order_path, None).await;
    assert_eq!(json["fulfillment_status"], "unfulfilled");
    assert_eq!(json["unfulfilled"][0]["quantity"], 3);
    let (status, json) = send(&app, "PATCH", &fulfillment, Some(tracking)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "fulfillment_cancelled")
    );

    // Now the order cancels, and stays cancelled with its first reason.
    let (status, json) = send(
        &app,
        "POST",
        &cancel,
        Some(r#"{"reason":"inventory","notify":true}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["cancel_reason"], "inventory");
    assert!(json["cancelled_at"].is_string());
    assert_eq!(json["unfulfilled"], json!([]));
    let (_, again) = send(&app, "POST", &cancel, Some(reason)).await;
    assert_eq!(again["cancel_reason"], "inventory");
    let (status, json) = send(&app, "POST", &fulfillments, Some(&ship(1))).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "order_cancelled")
    );
    let (status, json) = send(&app, "POST", &format!("{order_path}/paid"), None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "order_cancelled")
    );

    // The list shows the cancellation too.
    let (_, page) = send(&app, "GET", "/admin/orders", None).await;
    assert!(page["items"][0]["cancelled_at"].is_string());
}

#[tokio::test]
async fn bad_fulfilments_are_refused_with_codes() {
    let app = app().await;
    let (order, line, warehouse) = placed_order_for_three_mugs(&app).await;
    let fulfillments = format!("/admin/orders/{order}/fulfillments");
    let post = |body: Value| {
        let app = &app;
        let path = fulfillments.clone();
        async move { send(app, "POST", &path, Some(&body.to_string())).await }
    };
    let unknown_location = format!("loc_{}", "0".repeat(32));
    for (body, status, expected) in [
        (
            json!({"location": unknown_location, "lines": [{"line": line, "quantity": 1}]}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "location_not_found",
        ),
        (
            json!({"location": warehouse, "lines": [{"line": format!("ordl_{}", "0".repeat(32)), "quantity": 1}]}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_order_line",
        ),
        (
            json!({"location": warehouse, "lines": []}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "fulfillment_has_no_lines",
        ),
        (
            json!({"location": warehouse, "lines": [{"line": line, "quantity": 0}]}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_quantity",
        ),
        (
            json!({"location": warehouse, "lines": [{"line": line, "quantity": 1}],
                "tracking": [{"number": "1", "url": "dhl.example"}]}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_tracking_url",
        ),
    ] {
        let (got, json) = post(body).await;
        assert_eq!((got, code(&json)), (status, expected), "{json}");
    }
    let missing = format!("/admin/orders/ord_{}/fulfillments", "0".repeat(32));
    let (status, json) = send(&app, "GET", &missing, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "order_not_found")
    );
    let missing = format!("/admin/fulfillments/ful_{}", "0".repeat(32));
    let (status, json) = send(&app, "GET", &missing, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "fulfillment_not_found")
    );
}

#[tokio::test]
async fn fulfilling_needs_fulfillment_scopes_and_cancelling_orders_write() {
    let app = app().await;
    let (order, line, warehouse) = placed_order_for_three_mugs(&app).await;
    let orders_only = key(
        &app,
        json!({"kind": "secret", "name": "o", "scopes": ["orders:write"]}),
    )
    .await;
    let shipper = key(
        &app,
        json!({"kind": "secret", "name": "s", "scopes": ["fulfillments:write"]}),
    )
    .await;
    let body = json!({"location": warehouse, "lines": [{"line": line, "quantity": 1}]}).to_string();
    let fulfillments = format!("/admin/orders/{order}/fulfillments");
    let (status, _) = send_as(&app, &orders_only, "POST", &fulfillments, Some(&body)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, json) = send_as(&app, &shipper, "POST", &fulfillments, Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    // A shipper reads fulfilments but can't cancel the order.
    let (status, _) = send_as(&app, &shipper, "GET", &fulfillments, None).await;
    assert_eq!(status, StatusCode::OK);
    let cancel = format!("/admin/orders/{order}/cancel");
    let (status, _) = send_as(
        &app,
        &shipper,
        "POST",
        &cancel,
        Some(r#"{"reason":"staff"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_retried_fulfilment_replays_the_first() {
    let app = app().await;
    let (order, line, warehouse) = placed_order_for_three_mugs(&app).await;
    let body = json!({"location": warehouse, "lines": [{"line": line, "quantity": 1}]}).to_string();
    let path = format!("/admin/orders/{order}/fulfillments");
    let auth = format!("Bearer {}", app.admin_key);
    let headers = [
        (AUTHORIZATION.as_str(), auth.as_str()),
        ("idempotency-key", "ship-1"),
    ];
    let (status, first) = send_with(&app, &headers, "POST", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let (status, again) = send_with(&app, &headers, "POST", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert_eq!(again["id"], first["id"]);
    let (_, json) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    assert_eq!(json["lines"][0]["fulfilled_quantity"], 1);
}

#[tokio::test]
async fn a_store_starts_with_a_default_location() {
    let app = app().await;
    let (status, json) = send(&app, "GET", "/admin/locations", None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "Default");
    assert_eq!(items[0]["markets"], json!([]));
}

#[tokio::test]
async fn a_storefront_never_sees_why_an_order_was_cancelled() {
    let app = app().await;
    let mug = shop_selling_a_mug(&app).await;
    let (_, product) = send(&app, "GET", "/admin/products?limit=1", None).await;
    let product = product["items"][0]["id"].as_str().unwrap().to_string();
    send(
        &app,
        "PATCH",
        &format!("/admin/products/{product}/variants/{mug}"),
        Some(r#"{"requires_shipping":false}"#),
    )
    .await;
    let pk = publishable(&app).await;
    let (_, cart) = send_as(
        &app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let cart_path = format!("/storefront/carts/{}", cart["id"].as_str().unwrap());
    let line = json!({"variant": mug, "quantity": 1}).to_string();
    send_as(
        &app,
        &pk,
        "POST",
        &format!("{cart_path}/lines"),
        Some(&line),
    )
    .await;
    send_as(
        &app,
        &pk,
        "PUT",
        &format!("{cart_path}/email"),
        Some(r#"{"email":"a@example.hr"}"#),
    )
    .await;
    let checkout = format!("{cart_path}/checkout");
    let auth = format!("Bearer {pk}");
    let headers = [
        (AUTHORIZATION.as_str(), auth.as_str()),
        ("idempotency-key", "buy-1"),
    ];
    let (status, order) = send_with(&app, &headers, "POST", &checkout, Some("{}")).await;
    assert_eq!(status, StatusCode::CREATED, "{order}");

    let cancel = format!("/admin/orders/{}/cancel", order["id"].as_str().unwrap());
    let (status, cancelled) = send(&app, "POST", &cancel, Some(r#"{"reason":"fraud"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled["cancel_reason"], "fraud");

    // The shopper's retry replays the order as it is now, without the admin's fields.
    let (status, replayed) = send_with(&app, &headers, "POST", &checkout, Some("{}")).await;
    assert_eq!(status, StatusCode::CREATED, "{replayed}");
    assert_eq!(replayed["id"], order["id"]);
    assert!(replayed.get("cancel_reason").is_none(), "{replayed}");
    assert!(replayed.get("cancelled_at").is_none(), "{replayed}");
}

// ---------- search (design note 0019) ----------

/// Delivers events and rebuilds search until both are done, as the jobs would.
async fn settle_search(app: &TestApp) {
    for _ in 0..20 {
        let dispatched = crate::jobs::dispatch(&app.state).await.unwrap();
        let synced = crate::jobs::search_sync(&app.state).await.unwrap();
        let (_, status) = send(app, "GET", "/admin/search/status", None).await;
        if !dispatched && !synced && status["queued"] == 0 {
            return;
        }
    }
    panic!("search never settled");
}

#[tokio::test]
async fn a_storefront_searches_what_its_channel_publishes() {
    let app = app().await;
    let ids = shop_selling(&app, &["mug", "cup"]).await;
    let (_, outlet) = send(
        &app,
        "POST",
        "/admin/channels",
        Some(r#"{"name":"Outlet","kind":"custom"}"#),
    )
    .await;
    let outlet = outlet["id"].as_str().unwrap().to_string();
    let outlet_key = key(
        &app,
        json!({"kind": "publishable", "name": "Outlet", "channel": outlet}),
    )
    .await;
    let pk = publishable(&app).await;
    let titles = |found: &Value| -> Vec<String> {
        let items = found["items"].as_array().unwrap();
        items
            .iter()
            .map(|h| h["title"].as_str().unwrap().to_string())
            .collect()
    };
    let url = "/storefront/search?country=HR&sort=title";

    settle_search(&app).await;
    let (_, found) = send_as(&app, &pk, "GET", url, None).await;
    assert_eq!(titles(&found), ["cup", "mug"]);
    let (_, found) = send_as(&app, &outlet_key, "GET", url, None).await;
    assert_eq!(found["total"], 0, "the outlet sells nothing yet");

    // Moving the cup to the outlet moves it in search too.
    let body = json!({ "channels": [outlet] }).to_string();
    let path = format!("/admin/products/{}/channels", ids[1]);
    let (status, json) = send(&app, "PUT", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    settle_search(&app).await;
    let (_, cup) = send(&app, "GET", &format!("/admin/products/{}", ids[1]), None).await;
    assert_eq!(cup["channels"], json!([outlet]));
    let (_, list) = send(&app, "GET", "/admin/products", None).await;
    let online = online_store(&app).await;
    assert_eq!(
        (&list["items"][0]["channels"], &list["items"][1]["channels"]),
        (&json!([outlet]), &json!([online])),
        "newest first: the cup, then the mug"
    );
    let (_, found) = send_as(&app, &pk, "GET", url, None).await;
    assert_eq!(titles(&found), ["mug"]);
    assert_eq!(
        found["facets"][0]["key"], "product_type",
        "facets still come"
    );
    let (_, found) = send_as(&app, &outlet_key, "GET", url, None).await;
    assert_eq!(titles(&found), ["cup"]);
}

#[tokio::test]
async fn the_storefront_searches_its_market() {
    let app = app().await;
    let mug = shop_selling_a_mug(&app).await;
    settle_search(&app).await;
    let pk = publishable(&app).await;

    let (status, found) =
        send_as(&app, &pk, "GET", "/storefront/search?country=HR&q=MU", None).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found["total"], 1);
    let hit = &found["items"][0];
    assert_eq!(
        (
            &hit["variant"],
            &hit["title"],
            &hit["price"],
            &hit["available"]
        ),
        (
            &json!(mug),
            &json!("Mug"),
            &json!({ "amount": 1000, "currency": "EUR" }),
            &json!(true)
        )
    );
    assert_eq!(found["price"]["min"]["amount"], 1000);
    assert_eq!(
        (&found["has_more"], &found["next_cursor"]),
        (&json!(false), &json!(null))
    );

    let (_, none) = send_as(
        &app,
        &pk,
        "GET",
        "/storefront/search?country=HR&q=cup",
        None,
    )
    .await;
    assert_eq!((&none["total"], &none["price"]), (&json!(0), &json!(null)));
}

/// A market for HR in EUR whose catalog sells active products with these handles (10
/// EUR each, from `create_product`); their IDs.
async fn shop_selling(app: &TestApp, handles: &[&str]) -> Vec<String> {
    let mut ids = Vec::new();
    for handle in handles {
        let id = create_product(app, handle).await;
        let path = format!("/admin/products/{id}/status");
        send(app, "PATCH", &path, Some(r#"{"status":"active"}"#)).await;
        ids.push(id);
    }
    let body = r#"{"name":"Croatia","countries":["HR"],"currency":"EUR"}"#;
    let (_, market) = send(app, "POST", "/admin/markets", Some(body)).await;
    let body = json!({ "market": market["id"] }).to_string();
    let (_, catalog) = send(app, "POST", "/admin/catalogs", Some(&body)).await;
    let path = format!(
        "/admin/catalogs/{}/products",
        catalog["id"].as_str().unwrap()
    );
    let body = json!({ "products": ids }).to_string();
    let (status, _) = send(app, "POST", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    publish(app, &ids.iter().map(String::as_str).collect::<Vec<_>>()).await;
    ids
}

#[tokio::test]
async fn storefront_search_pages_with_a_cursor_bound_to_its_query() {
    let app = app().await;
    shop_selling(&app, &["mug", "cup"]).await;
    settle_search(&app).await;
    let pk = publishable(&app).await;

    let first_url = "/storefront/search?country=HR&sort=title&limit=1";
    let (_, first) = send_as(&app, &pk, "GET", first_url, None).await;
    assert_eq!(
        (&first["total"], &first["has_more"]),
        (&json!(2), &json!(true))
    );
    assert_eq!(first["items"][0]["handle"], "cup");
    let cursor = first["next_cursor"].as_str().unwrap();
    let (_, second) = send_as(
        &app,
        &pk,
        "GET",
        &format!("{first_url}&cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(second["items"][0]["handle"], "mug");
    assert_eq!(second["next_cursor"], json!(null));

    // The cursor belongs to that query.
    let (status, body) = send_as(
        &app,
        &pk,
        "GET",
        &format!("/storefront/search?country=HR&sort=newest&limit=1&cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_cursor")
    );
}

#[tokio::test]
async fn storefront_search_refuses_bad_queries_with_codes() {
    let app = app().await;
    shop_selling_a_mug(&app).await;
    let pk = publishable(&app).await;
    for (query, status, expected) in [
        (
            "country=DE",
            StatusCode::UNPROCESSABLE_ENTITY,
            "no_market_for_country",
        ),
        (
            "country=HR&filter=Red",
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_filter",
        ),
        (
            "country=HR&filter=color:Red",
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_facet",
        ),
        (
            "country=HR&price_min=9&price_max=1",
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_price_range",
        ),
        (
            "country=HR&sort=cheapest",
            StatusCode::BAD_REQUEST,
            "invalid_query",
        ),
        (
            "country=HR&country=DE",
            StatusCode::BAD_REQUEST,
            "invalid_query",
        ),
        ("q=mug", StatusCode::BAD_REQUEST, "invalid_query"),
    ] {
        let (got, body) = send_as(
            &app,
            &pk,
            "GET",
            &format!("/storefront/search?{query}"),
            None,
        )
        .await;
        assert_eq!((got, code(&body)), (status, expected), "{query}: {body}");
    }
}

#[tokio::test]
async fn search_filters_repeat_and_facets_count_listings() {
    let app = app().await;
    let mug = create_product(&app, "shirt").await;
    // Colour (Red, Blue): the first variant is Red; add Blue.
    let options = format!("/admin/products/{mug}/options");
    let body = r#"{"options":[{"name":"Color","values":[{"label":"Red"},{"label":"Blue"}]}]}"#;
    let (_, sized) = send(&app, "PUT", &options, Some(body)).await;
    let blue = sized["options"][0]["values"][1]["id"].clone();
    let body = json!({ "price": { "amount": 1200, "currency": "EUR" }, "selections": [blue] });
    let (status, _) = send(
        &app,
        "POST",
        &format!("/admin/products/{mug}/variants"),
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    send(
        &app,
        "PATCH",
        &format!("/admin/products/{mug}/status"),
        Some(r#"{"status":"active"}"#),
    )
    .await;
    let (_, market) = send(
        &app,
        "POST",
        "/admin/markets",
        Some(r#"{"name":"Croatia","countries":["HR"],"currency":"EUR"}"#),
    )
    .await;
    let body = json!({ "market": market["id"] }).to_string();
    let (_, catalog) = send(&app, "POST", "/admin/catalogs", Some(&body)).await;
    let body = json!({ "products": [mug] }).to_string();
    send(
        &app,
        "POST",
        &format!(
            "/admin/catalogs/{}/products",
            catalog["id"].as_str().unwrap()
        ),
        Some(&body),
    )
    .await;
    publish(&app, &[&mug]).await;
    settle_search(&app).await;
    let pk = publishable(&app).await;

    let url = "/storefront/search?country=HR&group=variant&filter=option.color:Red&filter=option.Color:Blue";
    let (status, both) = send_as(&app, &pk, "GET", url, None).await;
    assert_eq!(
        (status, &both["total"]),
        (StatusCode::OK, &json!(2)),
        "{both}"
    );
    let (_, one) = send_as(
        &app,
        &pk,
        "GET",
        "/storefront/search?country=HR&filter=option.color:Blue",
        None,
    )
    .await;
    assert_eq!(one["items"][0]["price"]["amount"], 1200);
    // One listing; the colour facet still shows Red and Blue.
    let colour = one["facets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "option.color")
        .unwrap()
        .clone();
    assert_eq!(
        colour["values"],
        json!([{"value": "Blue", "count": 1}, {"value": "Red", "count": 1}])
    );

    // Splitting by colour makes two listings, after a rebuild.
    let (status, settings) = send(
        &app,
        "PATCH",
        "/admin/search/settings",
        Some(r#"{"split_by":["Color"],"facets":[{"key":"option.color","label":"Colour"}]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    settle_search(&app).await;
    let (_, split) = send_as(&app, &pk, "GET", "/storefront/search?country=HR", None).await;
    assert_eq!(split["total"], 2);
    assert_eq!(
        split["facets"],
        json!([{
            "key": "option.color",
            "label": "Colour",
            "values": [{"value": "Blue", "count": 1}, {"value": "Red", "count": 1}],
        }])
    );
}

#[tokio::test]
async fn search_settings_reindex_and_status_over_http() {
    let app = app().await;
    let (status, settings) = send(&app, "GET", "/admin/search/settings", None).await;
    assert_eq!(
        (status, &settings),
        (StatusCode::OK, &json!({"split_by": [], "facets": []}))
    );
    let (status, body) = send(
        &app,
        "PATCH",
        "/admin/search/settings",
        Some(r#"{"split_by":["a","b","c","d"]}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "too_many_splits")
    );
    let (status, body) = send(
        &app,
        "PATCH",
        "/admin/search/settings",
        Some(r#"{"facets":[{"key":"colour"}]}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "unknown_facet")
    );

    let (status, queued) = send(&app, "POST", "/admin/search/reindex", None).await;
    assert_eq!(
        (status, &queued["queued"]),
        (StatusCode::ACCEPTED, &json!(1))
    );
    settle_search(&app).await;

    // A publishable key reads the storefront only.
    let pk = publishable(&app).await;
    let (status, body) = send_as(&app, &pk, "GET", "/admin/search/settings", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "admin_credentials_required")
    );
}

// ---------- storefront reads (design note 0021, step 4) ----------

#[tokio::test]
async fn a_product_page_is_one_read_priced_in_the_shoppers_country() {
    let app = app().await;
    let (market, mug_variant) = market_selling_a_mug(&app).await;
    let pk = publishable(&app).await;
    let get = |path: String| {
        let (app, pk) = (&app, pk.clone());
        async move { send_as(app, &pk, "GET", &path, None).await }
    };

    let (status, page) = get("/storefront/products/by-handle/mug?country=HR".into()).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(
        (
            &page["sold_in_country"],
            &page["currency"],
            &page["prices_include_tax"]
        ),
        (&json!(true), &json!("EUR"), &json!(false))
    );
    let variant = &page["variants"][0];
    assert_eq!(
        (&variant["id"], &variant["price"], &variant["available"]),
        (
            &json!(mug_variant),
            &json!({"amount": 1000, "currency": "EUR"}),
            &json!(true)
        )
    );
    // Its stock isn't tracked: no limit on how many.
    assert_eq!(variant["available_quantity"], Value::Null);
    assert_eq!(page["options"], json!([]));

    // By ID, the same read; without a country, no prices.
    let id = page["id"].as_str().unwrap().to_string();
    let (_, by_id) = get(format!("/storefront/products/{id}?country=HR")).await;
    assert_eq!(by_id, page);
    let (_, bare) = get(format!("/storefront/products/{id}")).await;
    assert_eq!(
        (
            &bare["sold_in_country"],
            &bare["variants"][0]["price"],
            &bare["variants"][0]["available"]
        ),
        (&Value::Null, &Value::Null, &Value::Null)
    );
    assert_eq!(bare["variants"][0]["available_quantity"], json!(0));

    // A country the store doesn't sell to: the product, marked not sold there (Q2).
    let (status, elsewhere) = get("/storefront/products/by-handle/mug?country=US".into()).await;
    assert_eq!(
        (
            status,
            &elsewhere["sold_in_country"],
            &elsewhere["currency"],
            &elsewhere["variants"][0]["price"]
        ),
        (StatusCode::OK, &json!(false), &Value::Null, &Value::Null)
    );

    for (path, want, want_code) in [
        (
            "/storefront/products/by-handle/cup?country=HR",
            StatusCode::NOT_FOUND,
            "product_not_found",
        ),
        (
            "/storefront/products/by-handle/Not%20A%20Handle",
            StatusCode::NOT_FOUND,
            "product_not_found",
        ),
        (
            "/storefront/products/by-handle/mug?country=hr",
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_country_code",
        ),
        (
            "/storefront/products/by-handle/mug?country=XK",
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_country_code",
        ),
        (
            "/storefront/products/by-handle/mug?currency=EUR",
            StatusCode::BAD_REQUEST,
            "invalid_query",
        ),
    ] {
        let (status, body) = get(path.to_string()).await;
        assert_eq!((status, code(&body)), (want, want_code), "{path}");
    }

    // A draft reads as missing.
    send(
        &app,
        "PATCH",
        &format!("/admin/products/{id}/status"),
        Some(r#"{"status":"draft"}"#),
    )
    .await;
    let (status, body) = get("/storefront/products/by-handle/mug?country=HR".into()).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "product_not_found")
    );

    let (status, countries) = get("/storefront/countries".into()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        countries["items"],
        json!([{"country": "HR", "name": "Croatia", "market": market, "currency": "EUR", "minor_units": 2, "prices_include_tax": false}])
    );
}

#[tokio::test]
async fn markets_are_renamed_price_lists_adjusted_and_keys_listed() {
    let app = app().await;
    let (_, mug_variant) = market_selling_a_mug(&app).await;
    let (_, markets) = send(&app, "GET", "/admin/markets", None).await;
    let market = markets["items"][0]["id"].as_str().unwrap().to_string();
    let url = format!("/admin/markets/{market}");
    let (status, renamed) = send(&app, "PATCH", &url, Some(r#"{"name":" Hrvatska "}"#)).await;
    assert_eq!(
        (status, &renamed["name"]),
        (StatusCode::OK, &json!("Hrvatska"))
    );
    let (status, body) = send(&app, "PATCH", &url, Some(r#"{"name":"  "}"#)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "field_required")
    );

    let (_, list) = send(
        &app,
        "POST",
        "/admin/price-lists",
        Some(r#"{"currency":"EUR"}"#),
    )
    .await;
    let url = format!("/admin/price-lists/{}", list["id"].as_str().unwrap());
    let (status, list) = send(&app, "PATCH", &url, Some(r#"{"adjustment_bps":-1500}"#)).await;
    assert_eq!(
        (status, &list["adjustment_bps"]),
        (StatusCode::OK, &json!(-1500))
    );
    let body = json!({"fixed_prices": [{"variant": mug_variant, "price": {"amount": 750, "currency": "EUR"}}]});
    send(
        &app,
        "PUT",
        &format!("{url}/fixed-prices"),
        Some(&body.to_string()),
    )
    .await;
    let (_, prices) = send(&app, "GET", &format!("{url}/fixed-prices"), None).await;
    assert_eq!(prices["items"][0]["product"]["title"], json!("Mug"));
    assert_eq!(prices["items"][0]["product"]["variant_title"], Value::Null);

    publishable(&app).await;
    let (status, keys) = send(&app, "GET", "/admin/api-keys?limit=1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        keys["items"][0]["kind"],
        json!("publishable"),
        "newest first"
    );
    assert!(
        keys["items"][0].get("token").is_none(),
        "tokens are shown only once"
    );
}

#[tokio::test]
async fn a_products_whole_variant_list_is_set_in_one_put() {
    let app = app().await;
    let product = create_product(&app, "tee").await;
    let options = r#"{"options":[{"name":"Size","values":[{"label":"S"},{"label":"M"}]}],"remove_variants":[]}"#;
    let url = format!("/admin/products/{product}/options");
    let (status, tee) = send(&app, "PUT", &url, Some(options)).await;
    assert_eq!(status, StatusCode::OK, "{tee}");
    let values = &tee["options"][0]["values"];
    let (small, medium) = (values[0]["id"].clone(), values[1]["id"].clone());
    let first = tee["variants"][0]["id"].clone();

    // "Create all variants": the admin sends every combination as an explicit list.
    let body = json!({"variants": [
        {"id": first, "sku": "TEE-S", "price": {"amount": 1000, "currency": "EUR"}, "selections": [small], "weight_grams": 180, "requires_shipping": true},
        {"id": null, "sku": "TEE-M", "price": {"amount": 1100, "currency": "EUR"}, "selections": [medium], "weight_grams": null, "requires_shipping": true}
    ]});
    let url = format!("/admin/products/{product}/variants");
    let (status, tee) = send(&app, "PUT", &url, Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{tee}");
    let skus: Vec<&Value> = tee["variants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| &v["sku"])
        .collect();
    assert_eq!(skus, [&json!("TEE-S"), &json!("TEE-M")]);
    assert_eq!(tee["variants"][0]["weight_grams"], json!(180));

    // Every key is required, so nothing is cleared by leaving it out.
    let body = json!({"variants": [{"id": first, "price": {"amount": 1000, "currency": "EUR"}, "selections": [small], "weight_grams": null, "requires_shipping": true}]});
    let (status, error) = send(&app, "PUT", &url, Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body")
    );
    let body = json!({"variants": [
        {"id": null, "sku": null, "price": {"amount": 1, "currency": "EUR"}, "selections": [small], "weight_grams": null, "requires_shipping": true},
        {"id": null, "sku": null, "price": {"amount": 1, "currency": "EUR"}, "selections": [small], "weight_grams": null, "requires_shipping": true}
    ]});
    let (status, error) = send(&app, "PUT", &url, Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::CONFLICT, "duplicate_option_combination")
    );
    let (_, stored) = send(&app, "GET", &format!("/admin/products/{product}"), None).await;
    assert_eq!(
        stored["variants"].as_array().unwrap().len(),
        2,
        "a refused list changes nothing"
    );
}

// ---------- categories (design note 0022) ----------

#[tokio::test]
async fn categories_form_a_tree_storefronts_see_shown_and_search_filters_by() {
    let app = app().await;
    let ids = shop_selling(&app, &["shirt", "trousers", "lamp"]).await;
    let create = |body: Value| {
        let app = &app;
        async move {
            let (status, json) =
                send(app, "POST", "/admin/categories", Some(&body.to_string())).await;
            assert_eq!(status, StatusCode::CREATED, "{json}");
            json["id"].as_str().unwrap().to_string()
        }
    };
    let clothing = create(json!({"name": "Clothing", "handle": "clothing", "visible": true, "description": "<p>Wear</p><script>x()</script>"})).await;
    let shirts =
        create(json!({"name": "Shirts", "handle": "shirts", "parent": clothing, "visible": true}))
            .await;
    let sale = create(json!({"name": "Sale", "handle": "sale", "visible": false})).await;

    // `visible` is required (CA10); handles are unique; a parent must exist.
    let (status, body) = send(
        &app,
        "POST",
        "/admin/categories",
        Some(r#"{"name":"X","handle":"x"}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body")
    );
    let (status, body) = send(
        &app,
        "POST",
        "/admin/categories",
        Some(r#"{"name":"S","handle":"shirts","visible":true}"#),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "category_handle_taken")
    );
    let missing = "pcat_00000000000000000000000000000063";
    let body = json!({"name": "Y", "handle": "y", "visible": true, "parent": missing}).to_string();
    let (status, body) = send(&app, "POST", "/admin/categories", Some(&body)).await;
    assert_eq!(
        (status, code(&body)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "category_parent_not_found"
        )
    );

    // The tree, nested; a category with its breadcrumb; a cycle refused.
    let (_, tree) = send(&app, "GET", "/admin/categories", None).await;
    assert_eq!(tree["items"][0]["children"][0]["handle"], json!("shirts"));
    let (_, page) = send(&app, "GET", &format!("/admin/categories/{shirts}"), None).await;
    assert_eq!(page["ancestors"][0]["handle"], json!("clothing"));
    let (_, page) = send(&app, "GET", &format!("/admin/categories/{clothing}"), None).await;
    assert_eq!(page["description"], json!("<p>Wear</p>"));
    let body = json!({"parent": shirts}).to_string();
    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/admin/categories/{clothing}"),
        Some(&body),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "category_cycle")
    );
    let (status, body) = send(&app, "GET", &format!("/admin/categories/{missing}"), None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "category_not_found")
    );

    // Membership: shirt in Shirts and Sale, trousers in Clothing.
    let body = json!({"categories": [shirts, sale]}).to_string();
    let (status, set) = send(
        &app,
        "PUT",
        &format!("/admin/products/{}/categories", ids[0]),
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let body = json!({"products": [ids[1]]}).to_string();
    let (status, _) = send(
        &app,
        "POST",
        &format!("/admin/categories/{clothing}/products"),
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, product) = send(&app, "GET", &format!("/admin/products/{}", ids[1]), None).await;
    assert_eq!(product["categories"], json!([clothing]));
    let (_, members) = send(
        &app,
        "GET",
        &format!("/admin/categories/{clothing}/products"),
        None,
    )
    .await;
    assert_eq!(members["items"][0]["handle"], json!("trousers"));
    let body = json!({"categories": [missing]}).to_string();
    let (status, body) = send(
        &app,
        "PUT",
        &format!("/admin/products/{}/categories", ids[0]),
        Some(&body),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "category_not_found")
    );

    // A parent with children can't go.
    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/admin/categories/{clothing}"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "category_has_children")
    );

    // The storefront: hidden Sale is gone; Clothing lists its subcategory's products.
    let pk = publishable(&app).await;
    let (_, shown) = send_as(&app, &pk, "GET", "/storefront/categories", None).await;
    let top: Vec<&Value> = shown["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| &c["handle"])
        .collect();
    assert_eq!(top, [&json!("clothing")]);
    let (status, body) = send_as(
        &app,
        &pk,
        "GET",
        "/storefront/categories/by-handle/sale",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "category_not_found")
    );
    let (_, shirts_page) = send_as(
        &app,
        &pk,
        "GET",
        "/storefront/categories/by-handle/shirts",
        None,
    )
    .await;
    assert_eq!(shirts_page["ancestors"][0]["name"], json!("Clothing"));

    settle_search(&app).await;
    let titles = |found: &Value| -> Vec<String> {
        found["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["title"].as_str().unwrap().to_string())
            .collect()
    };
    let url = format!("/storefront/search?country=HR&sort=title&category={clothing}");
    let (status, found) = send_as(&app, &pk, "GET", &url, None).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(titles(&found), ["shirt", "trousers"]);
    let url = format!("/storefront/search?country=HR&category={sale}");
    let (status, body) = send_as(&app, &pk, "GET", &url, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "category_not_found")
    );

    // Moving Shirts to the top level takes the shirt out of Clothing's listing.
    let body = json!({"parent": null}).to_string();
    let (status, moved) = send(
        &app,
        "PATCH",
        &format!("/admin/categories/{shirts}"),
        Some(&body),
    )
    .await;
    assert_eq!((status, &moved["parent"]), (StatusCode::OK, &Value::Null));
    settle_search(&app).await;
    let url = format!("/storefront/search?country=HR&sort=title&category={clothing}");
    let (_, found) = send_as(&app, &pk, "GET", &url, None).await;
    assert_eq!(titles(&found), ["trousers"]);
}
