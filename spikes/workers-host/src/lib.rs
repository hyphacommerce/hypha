//! The Workers host spike (design note 0026, §8). Three routes behind axum:
//!
//! - `GET /health`: the floor (a request that does nothing);
//! - `GET /argon2`: one Argon2id hash at the parameters staff logins use, to read its
//!   CPU time from the Worker's logs;
//! - `GET /p/{handle}?country=HR`: the storefront's product read, through the same
//!   commands and Postgres adapter as the VPS, over Hyperdrive.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use hypha_core::{Clock, Ctx, Timestamp};
use hypha_store_postgres::PgStore;
use tower_service::Service;
use worker::{Context, Date, Env, HttpRequest, event, send::SendWrapper};

/// Time from JavaScript's `Date.now()`: `SystemTime::now()` panics on this target.
struct JsClock;

impl Clock for JsClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_micros(Date::now().as_millis() as i64 * 1000)
    }
}

type Shared = Arc<SendWrapper<Env>>;

fn router(env: Env) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/argon2", get(argon2))
        .route("/p/{handle}", get(product))
        .route("/db", get(db_steps))
        .route("/connect", get(connect_only))
        .with_state(Arc::new(SendWrapper::new(env)))
}

#[event(fetch)]
async fn fetch(req: HttpRequest, env: Env, _ctx: Context) -> worker::Result<axum::http::Response<axum::body::Body>> {
    Ok(router(env).call(req).await?)
}

async fn argon2() -> Result<String, (StatusCode, String)> {
    use hypha_auth::{Argon2id, Password};
    let password = Password::new("correct horse battery staple").map_err(fail)?;
    let hash = Argon2id::OWASP
        .hash_now(&password, [7; 16])
        .map_err(|e| fail(format!("{e:?}")))?;
    Ok(format!("hashed: {} chars", format!("{hash:?}").len()))
}

#[worker::send]
async fn product(
    State(env): State<Shared>,
    Path(handle): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let t0 = Date::now().as_millis();
    let hyperdrive = env.hyperdrive("HYPERDRIVE").map_err(fail)?;
    let mut config: tokio_postgres::Config = hyperdrive.connection_string().parse().map_err(fail)?;
    worker::console_log!(
        "hyperdrive {}:{} ssl {:?} user {:?} db {:?}",
        hyperdrive.host(),
        hyperdrive.port(),
        config.get_ssl_mode(),
        config.get_user(),
        config.get_dbname()
    );
    // Three ways to reach Hyperdrive, to find the one it expects (the spike's question).
    let mode = query.get("mode").map_or("plain", String::as_str);
    let client = match mode {
        "starttls" => {
            let socket = worker::Socket::builder()
                .secure_transport(worker::SecureTransport::StartTls)
                .connect(hyperdrive.host(), hyperdrive.port())
                .map_err(fail)?;
            config.ssl_mode(tokio_postgres::config::SslMode::Require);
            let (client, connection) = config
                .connect_raw(socket, worker::postgres_tls::PassthroughTls)
                .await
                .map_err(fail)?;
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(e) = connection.await {
                    worker::console_error!("postgres connection: {e}");
                }
            });
            client
        }
        "socket" => {
            let socket = worker::Socket::builder()
                .connect(hyperdrive.host(), hyperdrive.port())
                .map_err(fail)?;
            config.ssl_mode(tokio_postgres::config::SslMode::Disable);
            let (client, connection) = config
                .connect_raw(socket, tokio_postgres::NoTls)
                .await
                .map_err(fail)?;
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(e) = connection.await {
                    worker::console_error!("postgres connection: {e}");
                }
            });
            client
        }
        _ => {
            let socket = hyperdrive.connect().map_err(fail)?;
            config.ssl_mode(tokio_postgres::config::SslMode::Disable);
            let (client, connection) = config
                .connect_raw(socket, tokio_postgres::NoTls)
                .await
                .map_err(fail)?;
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(e) = connection.await {
                    worker::console_error!("postgres connection: {e}");
                }
            });
            client
        }
    };
    let t1 = Date::now().as_millis();

    let store = PgStore::new(client, Arc::new(JsClock));
    let ctx = Ctx::system();
    let view = hypha_merchandising::get_product_view_by_handle(&ctx, &store, &handle)
        .await
        .map_err(fail)?;
    let t2 = Date::now().as_millis();
    let country = query.get("country").map_or("HR", String::as_str);
    let offer = hypha_cart::price_product(&ctx, &store, &view.product, country)
        .await
        .map_err(fail)?;
    let t3 = Date::now().as_millis();
    worker::console_log!("timing ms: connect {} product {} pricing {}", t1 - t0, t2 - t1, t3 - t2);
    Ok(Json(serde_json::json!({
        "ms": { "connect": t1 - t0, "product": t2 - t1, "pricing": t3 - t2 },
        "title": view.product.title(),
        "handle": view.product.handle().as_str(),
        "media": view.media.len(),
        "sold": offer.market.is_some(),
        "variants": offer.variants.iter().map(|v| serde_json::json!({
            "variant": v.variant.to_string(),
            "price": v.price.map(|p| p.money().amount()),
            "available": v.available,
            "sellable": v.sellable,
        })).collect::<Vec<_>>(),
    })))
}

fn fail(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Runs ever more of what the adapter does, logging each step, to find where a query
/// through Hyperdrive stops. Every transaction here ends explicitly; with one dropped
/// instead (a ROLLBACK queued without waiting), the next query never answered (0026, §8).
#[worker::send]
async fn db_steps(State(env): State<Shared>) -> Result<String, (StatusCode, String)> {
    let hyperdrive = env.hyperdrive("HYPERDRIVE").map_err(fail)?;
    let config: tokio_postgres::Config = hyperdrive.connection_string().parse().map_err(fail)?;
    let socket = hyperdrive.connect().map_err(fail)?;
    let (mut client, connection) = config
        .connect_raw(socket, tokio_postgres::NoTls)
        .await
        .map_err(fail)?;
    wasm_bindgen_futures::spawn_local(async move {
        worker::console_log!("connection task running");
        match connection.await {
            Ok(()) => worker::console_log!("connection task ended"),
            Err(e) => worker::console_error!("postgres connection: {e}"),
        }
    });
    let mut out = String::new();
    let step = |out: &mut String, what: &str| {
        worker::console_log!("step: {what}");
        out.push_str(what);
        out.push('\n');
    };
    step(&mut out, "connected");
    client.simple_query("SELECT 1").await.map_err(fail)?;
    step(&mut out, "simple_query ok");
    let row = client
        .query_typed_one("SELECT $1::text AS v", &[(&"x", tokio_postgres::types::Type::TEXT)])
        .await
        .map_err(fail)?;
    step(&mut out, &format!("query_typed ok: {}", row.get::<_, String>("v")));
    {
        let tx = client
            .build_transaction()
            .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()
            .await
            .map_err(fail)?;
        step(&mut out, "transaction started");
        tx.query_typed("SELECT count(*) FROM products", &[]).await.map_err(fail)?;
        step(&mut out, "query in transaction ok");
        let (a, b) = futures_join(
            tx.query_typed("SELECT count(*) FROM variants", &[]),
            tx.query_typed("SELECT count(*) FROM markets", &[]),
        )
        .await;
        a.map_err(fail)?;
        b.map_err(fail)?;
        step(&mut out, "two pipelined queries ok");
        tx.commit().await.map_err(fail)?;
        step(&mut out, "transaction committed (awaited)");
    }
    {
        let tx = client.transaction().await.map_err(fail)?;
        tx.query_typed("SELECT 1", &[]).await.map_err(fail)?;
        tx.rollback().await.map_err(fail)?;
        step(&mut out, "second transaction rolled back (awaited)");
    }
    client.simple_query("SELECT 2").await.map_err(fail)?;
    step(&mut out, "query after an explicit rollback ok");
    Ok(out)
}

async fn futures_join<A: std::future::Future, B: std::future::Future>(a: A, b: B) -> (A::Output, B::Output) {
    let mut a = std::pin::pin!(a);
    let mut b = std::pin::pin!(b);
    let (mut ra, mut rb) = (None, None);
    std::future::poll_fn(|cx| {
        if ra.is_none() {
            if let std::task::Poll::Ready(v) = a.as_mut().poll(cx) {
                ra = Some(v);
            }
        }
        if rb.is_none() {
            if let std::task::Poll::Ready(v) = b.as_mut().poll(cx) {
                rb = Some(v);
            }
        }
        if ra.is_some() && rb.is_some() {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await;
    (ra.unwrap(), rb.unwrap())
}

/// Connects to Hyperdrive and runs one trivial query: what every request pays before
/// its own work (the handshake, including SCRAM authentication).
#[worker::send]
async fn connect_only(State(env): State<Shared>) -> Result<String, (StatusCode, String)> {
    let hyperdrive = env.hyperdrive("HYPERDRIVE").map_err(fail)?;
    let config: tokio_postgres::Config = hyperdrive.connection_string().parse().map_err(fail)?;
    let (client, connection) = config
        .connect_raw(hyperdrive.connect().map_err(fail)?, tokio_postgres::NoTls)
        .await
        .map_err(fail)?;
    wasm_bindgen_futures::spawn_local(async move {
        let _ = connection.await;
    });
    client.simple_query("SELECT 1").await.map_err(fail)?;
    Ok("connected".into())
}
