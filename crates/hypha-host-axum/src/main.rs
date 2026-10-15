mod client_ip;
mod error;
mod extract;
mod media;
mod outbound;
mod patch;
mod rate_limit;
mod routes;
mod shipping;
mod state;

use std::sync::Arc;

use axum::Router;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use hypha_core::Ctx;
use hypha_stores::ensure_single_store;
use hypha_users::{HasUsers, NewOwner, UserRepo, bootstrap_owner};

use crate::client_ip::TrustedProxies;
use crate::state::{AppState, HostEnv, PgStorage, SharedState, Storage, SystemClock, UuidV7Ids};

/// `hypha-host-axum openapi [DIR]` writes the OpenAPI documents to `DIR` (default
/// `openapi`) and exits (design note 0010, S6). Run it after changing routes or DTOs;
/// a test fails while the committed files are out of date.
fn write_openapi(dir: &str) -> std::io::Result<()> {
    use routes::openapi::{Doc, document_json};
    std::fs::create_dir_all(dir)?;
    for doc in Doc::ALL {
        let path = std::path::Path::new(dir).join(doc.file_name());
        std::fs::write(&path, document_json(doc))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("openapi") {
        let dir = args.get(2).map_or("openapi", String::as_str);
        if let Err(e) = write_openapi(dir) {
            eprintln!("cannot write the OpenAPI documents: {e}");
            std::process::exit(1);
        }
        return;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    let admin_url = std::env::var("HYPHA_ADMIN_URL").ok();
    let secret_key = match std::env::var("HYPHA_SECRET_KEY")
        .ok()
        .map(|k| parse_key(&k))
    {
        Some(Ok(key)) => Some(key),
        Some(Err(e)) => {
            tracing::error!("HYPHA_SECRET_KEY: {e}");
            std::process::exit(1);
        }
        None => None,
    };

    // Postgres, migrated on startup.
    let Ok(url) = std::env::var("DATABASE_URL") else {
        tracing::error!(
            "DATABASE_URL is required, e.g. postgres://hypha:hypha@localhost:5432/hypha \
             (`docker compose up -d` starts that database)."
        );
        std::process::exit(1);
    };
    // Data outlives the process, so the key must too: a new key each start would make
    // every stored authenticator secret unreadable.
    let Some(secret_key) = secret_key else {
        tracing::error!(
            "HYPHA_SECRET_KEY is required: 64 hex characters, e.g. from \
             `openssl rand -hex 32`. Keep it; it seals 2FA secrets."
        );
        std::process::exit(1);
    };
    let storage = match PgStorage::connect(&url).await {
        Ok(storage) => storage,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };
    tracing::info!("storage: Postgres (migrations applied)");
    let env = HostEnv::new(secret_key, admin_url);
    serve(routes::postgres_router(), storage, env).await;
}

/// A 32-byte key written as 64 hex characters.
fn parse_key(hex: &str) -> Result<[u8; 32], String> {
    let hex = hex.trim();
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("expected 64 hex characters (32 bytes)".to_string());
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    Ok(key)
}

/// With no staff user yet, `HYPHA_OWNER_EMAIL` and `HYPHA_OWNER_PASSWORD` (and
/// optionally `HYPHA_OWNER_NAME`) create the owner (design note 0008, A11). Ignored once
/// anyone exists.
async fn ensure_owner<S: Storage>(state: &AppState<S>) -> Result<(), String> {
    let ports = state
        .public_ports()
        .await
        .map_err(|_| "cannot reach storage")?;
    let email = std::env::var("HYPHA_OWNER_EMAIL").ok();
    let password = std::env::var("HYPHA_OWNER_PASSWORD").ok();
    let (Some(email), Some(password)) = (email, password) else {
        if !ports.users().any().await.map_err(|e| e.to_string())? {
            tracing::warn!(
                "no staff user yet: set HYPHA_OWNER_EMAIL and HYPHA_OWNER_PASSWORD to create \
                 the owner, then restart"
            );
        }
        return Ok(());
    };
    let name = std::env::var("HYPHA_OWNER_NAME").unwrap_or_else(|_| "Owner".to_string());
    let input = NewOwner {
        email,
        name,
        password,
    };
    match bootstrap_owner(&Ctx::system(), &ports, &state.env, input).await {
        Ok(Some(owner)) => {
            tracing::info!(user = %owner.id(), email = %owner.email(), "created the owner")
        }
        Ok(None) => {}
        Err(e) => return Err(e.to_string()),
    }
    Ok(())
}

async fn serve<S: Storage>(router: Router<SharedState<S>>, storage: S, env: HostEnv) {
    let ids = UuidV7Ids;
    let trusted_proxies = match std::env::var("HYPHA_TRUSTED_PROXIES") {
        Ok(list) => match TrustedProxies::parse(&list) {
            Ok(proxies) => proxies,
            Err(e) => {
                tracing::error!("HYPHA_TRUSTED_PROXIES: {e}");
                std::process::exit(1);
            }
        },
        Err(_) => TrustedProxies::default(),
    };
    // The one setting that silently goes wrong behind a proxy (design note 0009, §8).
    if trusted_proxies.is_empty() {
        tracing::info!(
            "rate limits key clients by their socket address; behind a proxy or load \
             balancer, set HYPHA_TRUSTED_PROXIES or every client shares one limit"
        );
    } else {
        tracing::info!(
            ?trusted_proxies,
            "rate limits read X-Forwarded-For from trusted proxies"
        );
    }
    // One store per deployment (design note 0006, model A): created on first start.
    let name = std::env::var("HYPHA_STORE_NAME").unwrap_or_else(|_| "My store".to_string());
    let store = match storage.directory().await {
        Ok(directory) => ensure_single_store(&Ctx::system(), &directory, &ids, &name)
            .await
            .map_err(|e| e.to_string()),
        Err(_) => Err("cannot reach the store directory".to_string()),
    };
    let store = match store {
        Ok(store) => store,
        Err(e) => {
            tracing::error!("store setup failed: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!(store = %store.id(), name = store.name(), "serving store");

    let media = match media::Media::from_env() {
        Ok(media) => media,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };
    match &media {
        None => tracing::info!("media: off (set HYPHA_S3_ENDPOINT for file storage)"),
        Some(media) => {
            // Development servers start empty; production buckets are made by their owner.
            if std::env::var("HYPHA_S3_CREATE_BUCKET").as_deref() == Ok("true")
                && let Err(e) = media.blobs.ensure_bucket().await
            {
                tracing::error!("HYPHA_S3_CREATE_BUCKET: {e}");
                std::process::exit(1);
            }
            match &media.urls.public_base {
                Some(base) => tracing::info!(%base, "media: public files served from"),
                None => tracing::info!("media: signed URLs (set HYPHA_MEDIA_URL for a CDN)"),
            }
        }
    }

    let outbound = outbound::OutboundPolicy::from_env();
    if outbound.allow_private {
        tracing::warn!(
            "HYPHA_ALLOW_PRIVATE_CALLBACKS: rate apps may call private addresses; \
             development only"
        );
    }
    let state = Arc::new(AppState {
        storage,
        ids,
        clock: SystemClock,
        env,
        background: Default::default(),
        limiter: Default::default(),
        trusted_proxies,
        store: store.id(),
        store_name: store.name().to_string(),
        media,
        outbound,
        http: match outbound::client(outbound) {
            Ok(client) => client,
            Err(e) => {
                tracing::error!("{e}");
                std::process::exit(1);
            }
        },
    });
    if let Err(e) = ensure_owner(&state).await {
        tracing::error!("owner setup failed: {e}");
        std::process::exit(1);
    }
    tracing::warn!("emails (invites, password resets) are logged, not sent: development only");
    let app = router
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::clone(&state));

    let addr = bind_address();
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!("cannot listen on {addr}: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!("Hypha listening on http://{addr}");
    S::spawn_media_sweep(Arc::clone(&state));
    // Drop idle rate-limit keys every minute, so rotating addresses can't grow memory.
    let sweeper = Arc::clone(&state);
    tokio::spawn(async move {
        let mut every = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            every.tick().await;
            sweeper.limiter.forget_idle();
        }
    });
    // `with_connect_info` records each connection's peer address for `ClientIp`.
    let app = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        tracing::error!("server error: {e}");
        std::process::exit(1);
    }
    // Requests are done; let work they started (password reset emails) finish too.
    if !state
        .background
        .drain(std::time::Duration::from_secs(10))
        .await
    {
        tracing::warn!("stopping with background work unfinished");
    }
    tracing::info!("stopped");
}

/// `HYPHA_BIND` (`host:port`) if set. Otherwise `PORT` on every interface, as platforms
/// like Fly.io and Render expect. Otherwise `127.0.0.1:3000` for local development.
fn bind_address() -> String {
    bind_address_from(std::env::var("HYPHA_BIND").ok(), std::env::var("PORT").ok())
}

fn bind_address_from(bind: Option<String>, port: Option<String>) -> String {
    match (bind, port) {
        (Some(bind), _) => bind,
        (None, Some(port)) => format!("0.0.0.0:{port}"),
        (None, None) => "127.0.0.1:3000".to_string(),
    }
}

/// Resolves on Ctrl+C or (on Unix) SIGTERM, which is how orchestrators stop a
/// container. The server then stops accepting connections and lets in-flight requests
/// finish before `main` returns.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("cannot listen for Ctrl+C: {e}");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(e) => {
                tracing::error!("cannot listen for SIGTERM: {e}");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down: finishing in-flight requests");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_64_hex_characters() {
        let key = parse_key(&"0f".repeat(32)).unwrap();
        assert_eq!(key, [0x0f; 32]);
        assert!(parse_key(&"0f".repeat(31)).is_err());
        assert!(parse_key(&format!("{}zz", "0f".repeat(31))).is_err());
    }

    #[test]
    fn bind_address_prefers_hypha_bind_then_port_then_localhost() {
        let s = |v: &str| Some(v.to_string());
        assert_eq!(bind_address_from(s("[::]:8080"), s("9000")), "[::]:8080");
        assert_eq!(bind_address_from(None, s("9000")), "0.0.0.0:9000");
        assert_eq!(bind_address_from(None, None), "127.0.0.1:3000");
    }
}
