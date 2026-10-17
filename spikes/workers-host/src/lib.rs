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
    let hyperdrive = env.hyperdrive("HYPERDRIVE").map_err(fail)?;
    let mut config: tokio_postgres::Config = hyperdrive.connection_string().parse().map_err(fail)?;
    config.ssl_mode(tokio_postgres::config::SslMode::Disable);
    let socket = hyperdrive.connect().map_err(fail)?;
    let (client, connection) = config
        .connect_raw(socket, tokio_postgres::NoTls)
        .await
        .map_err(fail)?;
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = connection.await {
            worker::console_error!("postgres connection: {e}");
        }
    });

    let store = PgStore::new(client, Arc::new(JsClock));
    let ctx = Ctx::system();
    let view = hypha_merchandising::get_product_view_by_handle(&ctx, &store, &handle)
        .await
        .map_err(fail)?;
    let country = query.get("country").map_or("HR", String::as_str);
    let offer = hypha_cart::price_product(&ctx, &store, &view.product, country)
        .await
        .map_err(fail)?;
    Ok(Json(serde_json::json!({
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
