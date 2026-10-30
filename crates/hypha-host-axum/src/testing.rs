//! A test host for route tests: the API on a fresh Postgres database, driven with `oneshot`
//! (no socket), with its owner and a secret key holding every scope. The route tests use it,
//! and so can a spore's own (feature `testing`): build the host as a deployment with the
//! spore (`hypha_host_axum::deployment!`), then `app_with::<D>(toml)` and `send`.
//!
//! It needs a Postgres it may create databases on at `HYPHA_TEST_DATABASE_URL` (or
//! `DATABASE_URL`). Each test gets a database of its own, named after it and copied from a
//! template migrated once per set of migrations, so tests run in parallel and a rerun
//! starts clean.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test support: a broken fixture panics the test using it"
)]
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::connect_info::MockConnectInfo,
    http::{
        Request, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
};
use hypha_auth::{ApiKeyKind, Argon2id, CreateApiKey, create_api_key};
use hypha_config::Config;
use hypha_core::{Ctx, Scope};
use hypha_http::host::{AfterWrites, HostMail, MailQueued};
use hypha_http::mail::{MailHost, MailParts};
use hypha_http::media::Media;
use hypha_http::state::{AppState, HostEnv, HostParts};
use hypha_notifications::{OutgoingEmail, Sent};
use hypha_store_postgres::testing::TestDatabase;
use hypha_stores::ensure_single_store;
use hypha_users::{NewOwner, bootstrap_owner};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::platform::{Background, UuidV7Ids};
use crate::storage::PgDatabase;

pub const OWNER_EMAIL: &str = "owner@example.com";
pub const OWNER_PASSWORD: &str = "owner password";

/// The app on a fresh database, with its owner (`OWNER_EMAIL`), and a secret key with every scope
/// that `send` uses unless a test picks other credentials.
pub struct TestApp<D: crate::Deployment = crate::Stock> {
    pub(crate) router: Router,
    pub(crate) admin_key: String,
    /// The app's state, for checks the API doesn't expose (the outbox).
    pub(crate) state: hypha_http::state::SharedState<crate::storage::PgStorage<D>>,
    /// What the app's email sender was given (a recording sender, never SMTP, whatever
    /// `.cargo/config.toml` says), oldest first.
    pub(crate) emails: Arc<Mutex<Vec<OutgoingEmail>>>,
    /// Last, so it's dropped after the state's connections.
    pub(crate) _database: TestDatabase,
}

impl<D: crate::Deployment> TestApp<D> {
    /// The router, for a request the helpers can't make (`oneshot` a `Request`).
    pub fn router(&self) -> Router {
        self.router.clone()
    }

    /// The secret key with every scope that `send` uses.
    pub fn admin_key(&self) -> &str {
        &self.admin_key
    }

    /// The app's state, for what the API doesn't expose: running a job step
    /// (`hypha_http::jobs::dispatch(app.state())`), reading the outbox.
    pub fn state(&self) -> &hypha_http::state::SharedState<crate::storage::PgStorage<D>> {
        &self.state
    }

    /// The emails the app sent, oldest first.
    pub fn emails(&self) -> Vec<OutgoingEmail> {
        self.emails.lock().map(|e| e.clone()).unwrap_or_default()
    }
}

/// The app built as `D` (its spores' migrations, apps, routes, handlers and settings), under
/// `hypha.config.toml` text without the environment: `"[spores.stripe]\napi_base = \"…\""`.
/// Calls to private addresses are allowed, so a fake of the service the spore calls can
/// listen on localhost.
pub async fn app_with<D: crate::Deployment>(toml: &str) -> TestApp<D> {
    let config = Config::load(Some(toml), |_| None).expect("the test's configuration");
    app_with_spores::<D>(config).await
}

/// A sender that keeps what it's given, named `test` (so test sends aren't refused as
/// they are with the `log` sender).
pub(crate) fn recording_sender() -> (HostMail, Arc<Mutex<Vec<OutgoingEmail>>>) {
    let emails: Arc<Mutex<Vec<OutgoingEmail>>> = Arc::default();
    let kept = Arc::clone(&emails);
    let sender = HostMail::new("test", move |email| {
        kept.lock().unwrap().push(email);
        Box::pin(async { Ok(Sent::default()) })
    });
    (sender, emails)
}

/// Requests in these tests come from this peer (`MockConnectInfo`), as if over a socket.
pub(crate) const PEER: ([u8; 4], u16) = ([127, 0, 0, 1], 4000);

/// A database of its own for the running test, copied from a template migrated with
/// `D`'s migrations, and dropped with the `TestDatabase`. libtest names each test's thread
/// after the test, so the name is stable across runs and unique within one.
pub(crate) async fn fresh_database<D: crate::Deployment>() -> TestDatabase {
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
    let name = format!("rt_{hash:016x}_{tail}").to_lowercase();
    TestDatabase::new(&name, &crate::migrations::<D>())
        .await
        .expect("HYPHA_TEST_DATABASE_URL or DATABASE_URL (see .cargo/config.toml)")
}

/// File storage under `config` (the compose server), bucket created.
pub(crate) async fn test_media(config: &Config) -> Media {
    let media = Media::from_config(
        &config.media,
        |name| std::env::var(name).ok(),
        crate::platform::host_http,
        crate::platform::runtime().clock,
    )
    .expect("HYPHA_S3_* (see .cargo/config.toml)")
    .expect("HYPHA_S3_ENDPOINT (see .cargo/config.toml)");
    media
        .blobs
        .ensure_bucket()
        .await
        .unwrap_or_else(|e| panic!("route tests need the S3 server (`docker compose up -d`): {e}"));
    media
}

/// The app built as `D` (its spores' migrations, apps, routes, handlers and settings), under
/// `config`.
pub(crate) async fn app_with_spores<D: crate::Deployment>(config: Config) -> TestApp<D> {
    app_for::<D>(config, false, None).await
}

/// `app_sending_with` for a host built with `D`'s spores.
pub(crate) async fn app_for<D: crate::Deployment>(
    mut config: Config,
    media: bool,
    sender: Option<HostMail>,
) -> TestApp<D> {
    // The tests' rate apps and webhook receivers listen on localhost.
    config.allow_private_callbacks = true;
    app_exactly::<D>(config, media, sender).await
}

pub(crate) async fn app_exactly<D: crate::Deployment>(
    config: Config,
    media: bool,
    sender: Option<HostMail>,
) -> TestApp<D> {
    // Warnings and errors (a 500's cause is only logged) go to the test's output, which
    // a failing test prints: in CI, that's the only place to see them.
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter(tracing_subscriber::EnvFilter::new("warn"))
        .try_init();
    let media = match media {
        true => Some(Arc::new(test_media(&config).await)),
        false => None,
    };
    let runtime = crate::platform::runtime();
    let database = fresh_database::<D>().await;
    let db = PgDatabase::connect(
        database.url(),
        &config.database,
        runtime.clock.clone(),
        &crate::migrations::<D>(),
    )
    .await
    .unwrap();
    let store = ensure_single_store(
        &Ctx::system(),
        &db.store().await.unwrap(),
        &UuidV7Ids,
        "Test",
    )
    .await
    .unwrap();
    let env = HostEnv::new([7; 32], config.auth, runtime.clone())
        .with_hasher(Argon2id::new(8, 1, 1))
        .with_staff(config.staff.clone());
    let (recording, emails) = recording_sender();
    let mail = Arc::new(MailHost::new(
        MailParts {
            sender: sender.unwrap_or(recording),
            from: config.email.from.clone(),
            store: store.id(),
            admin_url: config.server.admin_url.clone(),
            order_tokens: env.order_tokens().clone(),
            media: media.clone(),
            queued: MailQueued::none(),
        },
        &runtime,
    ));
    let outbound = hypha_http::outbound::OutboundPolicy {
        allow_private: config.allow_private_callbacks,
    };
    let client = crate::calls::client(outbound).unwrap();
    let spores =
        hypha_http::spores::spore_host(&env, crate::calls::host_calls(client.clone()), outbound);
    let storage =
        crate::storage::PgStorage::<D>::new(db, Arc::clone(&mail), &spores, &config.spores)
            .unwrap();
    let host = HostParts {
        env,
        background: Background::default().host(),
        limiter: crate::start::host_limiter(Arc::new(crate::limiter::MemoryRateLimiter::new(
            &config.rate_limits,
        ))),
        after_writes: AfterWrites::none(),
        calls: crate::calls::host_calls(client),
        media,
        mail,
        publishable_keys: Default::default(),
    };
    let state = Arc::new(AppState::new(
        storage,
        store.id(),
        store.name().to_string(),
        host,
        config,
    ));
    // As `prepare` does at startup.
    let max_body = state.config.server.max_body;
    state.install(D::router(max_body), D::spore_rows());
    crate::start::register_spores(&state).await.unwrap();
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
        router: D::router(max_body)
            .layer(axum::middleware::from_fn(crate::start::record_peer))
            .with_state(state.clone())
            .layer(MockConnectInfo(SocketAddr::from(PEER))),
        admin_key: token.expose().to_string(),
        state,
        emails,
        _database: database,
    }
}

/// Sends one request with the admin key and returns the status and the JSON body
/// (`null` if empty).
pub async fn send<D: crate::Deployment>(
    app: &TestApp<D>,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let auth = format!("Bearer {}", app.admin_key);
    send_with(app, &[(AUTHORIZATION.as_str(), &auth)], method, uri, body).await
}

/// Like `send`, as `token` (a session or key) instead of the admin key.
pub async fn send_as<D: crate::Deployment>(
    app: &TestApp<D>,
    token: &str,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let auth = format!("Bearer {token}");
    send_with(app, &[(AUTHORIZATION.as_str(), &auth)], method, uri, body).await
}

/// Sends one request with exactly `headers` (no credentials unless among them).
pub async fn send_with<D: crate::Deployment>(
    app: &TestApp<D>,
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

/// Runs `statement` on the test's schema, for state no route can set (time passing).
pub async fn sql<D: crate::Deployment>(app: &TestApp<D>, statement: &str) {
    use hypha_store_postgres::PgConnection;
    let ports = app.state.public_ports().await.unwrap();
    let mut conn = ports.conn().await;
    conn.client().batch_execute(statement).await.unwrap();
}

/// Creates a key as the admin; returns its token.
pub(crate) async fn key<D: crate::Deployment>(app: &TestApp<D>, body: Value) -> String {
    let (status, created) = send(app, "POST", "/admin/api-keys", Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["token"].as_str().unwrap().to_string()
}

/// A publishable key for the Online Store, made with the admin key: a storefront's
/// credentials.
pub async fn publishable<D: crate::Deployment>(app: &TestApp<D>) -> String {
    let body = json!({
        "kind": "publishable",
        "name": "Storefront",
        "channel": online_store(app).await,
    });
    key(app, body).await
}

/// Publishes the products to the Online Store (new products aren't sold anywhere, CH9).
pub(crate) async fn publish<D: crate::Deployment>(app: &TestApp<D>, products: &[&str]) {
    let path = format!("/admin/channels/{}/products", online_store(app).await);
    let body = json!({ "products": products }).to_string();
    let (status, json) = send(app, "POST", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");
}

/// The channel every store starts with (migration).
pub(crate) async fn online_store<D: crate::Deployment>(app: &TestApp<D>) -> String {
    let (_, channels) = send(app, "GET", "/admin/channels", None).await;
    channels["items"][0]["id"].as_str().unwrap().to_string()
}

/// A market for HR in EUR selling one active 10 EUR mug, published to the Online Store:
/// returns (the market's ID, the mug's variant ID).
pub async fn market_selling_a_mug<D: crate::Deployment>(app: &TestApp<D>) -> (String, String) {
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

/// A shop selling mugs that need no shipping, 10 at the warehouse, and a storefront
/// cart for 3 with an email, before choosing how to pay. Returns (the publishable key,
/// the cart's path, the warehouse's ID).
pub async fn cart_for_three_mugs<D: crate::Deployment>(
    app: &TestApp<D>,
) -> (String, String, String) {
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
    (pk, cart_path, location)
}
