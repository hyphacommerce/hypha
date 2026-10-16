mod client_ip;
mod error;
mod extract;
mod handlers;
mod jobs;
mod media;
mod outbound;
mod patch;
mod rate_limit;
mod routes;
mod shipping;
mod state;
mod webhooks;

use std::sync::Arc;

use axum::Router;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use hypha_config::Config;
use hypha_core::Ctx;
use hypha_inventory::{HasLocations, ensure_default_location};
use hypha_stores::ensure_single_store;
use hypha_users::{HasUsers, NewOwner, UserRepo, bootstrap_owner};

use crate::rate_limit::MemoryRateLimiter;
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

/// Reads the configuration (design note 0017): the file `HYPHA_CONFIG` names, else
/// `hypha.config.toml` if it's there, under the environment. `Ok` with the file read,
/// if any.
fn load_config() -> Result<(Config, Option<String>), String> {
    let named = std::env::var("HYPHA_CONFIG").ok().filter(|p| !p.is_empty());
    let path = named
        .clone()
        .unwrap_or_else(|| hypha_config::DEFAULT_PATH.to_string());
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        // Only the default file is optional; one named on purpose must be there.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && named.is_none() => None,
        Err(e) => return Err(format!("cannot read {path}: {e}")),
    };
    let source = match &text {
        Some(_) => format!("{path} and the environment"),
        None => "the environment (no hypha.config.toml)".to_string(),
    };
    let config = Config::load(text.as_deref(), |name| std::env::var(name).ok())
        .map_err(|e| format!("invalid configuration, from {source}:\n{e}"))?;
    Ok((config, text.map(|_| path)))
}

/// `hypha-host-axum config` prints the settings in effect; `config schema [PATH]` writes
/// the file's JSON Schema to `PATH` (default `hypha.config.schema.json`; a test fails
/// while the committed one is stale).
fn config_command(args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) == Some("schema") {
        let path = args
            .get(1)
            .map_or("hypha.config.schema.json", String::as_str);
        std::fs::write(path, hypha_config::schema_json())
            .map_err(|e| format!("cannot write {path}: {e}"))?;
        println!("wrote {path}");
        return Ok(());
    }
    let (config, path) = load_config()?;
    let from = path.as_deref().unwrap_or("defaults");
    print!(
        "# The settings in effect: {from}, then the environment.\n\n{}",
        config.to_toml()
    );
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("openapi") => {
            let dir = args.get(2).map_or("openapi", String::as_str);
            if let Err(e) = write_openapi(dir) {
                eprintln!("cannot write the OpenAPI documents: {e}");
                std::process::exit(1);
            }
            return;
        }
        Some("config") => {
            if let Err(e) = config_command(&args[2..]) {
                eprintln!("{e}");
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    let config = match load_config() {
        Ok((config, path)) => {
            match path {
                Some(path) => tracing::info!("configuration: {path}"),
                None => tracing::info!("configuration: defaults (no hypha.config.toml)"),
            }
            config
        }
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };
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
    let storage = match PgStorage::connect(&url, &config.database).await {
        Ok(storage) => storage,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };
    tracing::info!("storage: Postgres (migrations applied)");
    let env = HostEnv::new(secret_key, config.server.admin_url.clone(), config.auth);
    serve(routes::postgres_router(), storage, env, config).await;
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

/// A store with no location gets "Default", so it can fulfil orders before it sets up
/// stock (design note 0018).
async fn ensure_location<S: Storage>(state: &AppState<S>) -> Result<(), String> {
    let ports = state
        .public_ports()
        .await
        .map_err(|_| "cannot reach storage")?;
    match ensure_default_location(&Ctx::system(), ports.locations(), &state.ids).await {
        Ok(Some(location)) => {
            tracing::info!(location = %location.id(), "created the default location")
        }
        Ok(None) => {}
        Err(e) => return Err(e.to_string()),
    }
    Ok(())
}

async fn serve<S: Storage>(
    router: Router<SharedState<S>>,
    storage: S,
    env: HostEnv,
    config: Config,
) {
    let ids = UuidV7Ids;
    let trusted_proxies = &config.server.trusted_proxies;
    // The one setting that silently goes wrong behind a proxy (design note 0009, §8).
    if trusted_proxies.is_empty() {
        tracing::info!(
            "rate limits key clients by their socket address; behind a proxy or load \
             balancer, set server.trusted_proxies or every client shares one limit"
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

    let media = match media::Media::from_config(&config.media) {
        Ok(media) => media,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };
    match &media {
        None => tracing::info!("media: off (set media.s3.endpoint for file storage)"),
        Some(media) => {
            // Development servers start empty; production buckets are made by their owner.
            if config.media.s3.as_ref().is_some_and(|s3| s3.create_bucket)
                && let Err(e) = media.blobs.ensure_bucket().await
            {
                tracing::error!("media.s3.create_bucket: {e}");
                std::process::exit(1);
            }
            match &media.urls.public_base {
                Some(base) => tracing::info!(%base, "media: public files served from"),
                None => tracing::info!("media: signed URLs (set media.public_url for a CDN)"),
            }
        }
    }

    let outbound = outbound::OutboundPolicy {
        allow_private: config.allow_private_callbacks,
    };
    if outbound.allow_private {
        tracing::warn!(
            "outbound.allow_private: apps may be called at private addresses; development only"
        );
    }
    if !config.jobs.dispatch.enabled {
        tracing::info!("jobs: events aren't delivered from this instance (jobs.dispatch)");
    }
    let grace = config.server.shutdown_grace;
    let addr = config.server.bind.clone();
    let state = Arc::new(AppState {
        storage,
        ids,
        clock: SystemClock,
        env,
        background: Default::default(),
        limiter: MemoryRateLimiter::new(&config.rate_limits),
        store: store.id(),
        store_name: store.name().to_string(),
        media,
        http: match outbound::client(outbound) {
            Ok(client) => client,
            Err(e) => {
                tracing::error!("{e}");
                std::process::exit(1);
            }
        },
        config,
    });
    if let Err(e) = ensure_owner(&state).await {
        tracing::error!("owner setup failed: {e}");
        std::process::exit(1);
    }
    if let Err(e) = ensure_location(&state).await {
        tracing::error!("location setup failed: {e}");
        std::process::exit(1);
    }
    tracing::warn!("emails (invites, password resets) are logged, not sent: development only");
    let app = router
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::clone(&state));

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!("cannot listen on {addr}: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!("Hypha listening on http://{addr}");
    let jobs = S::start_jobs(Arc::clone(&state));
    // `with_connect_info` records each connection's peer address for `ClientIp`.
    let app = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        tracing::error!("server error: {e}");
        std::process::exit(1);
    }
    // Requests are done; stop the jobs after the step each is in, and let work the
    // requests started (password reset emails) finish too.
    if !jobs.stop(grace).await {
        tracing::warn!("stopping with background jobs unfinished");
    }
    if !state.background.drain(grace).await {
        tracing::warn!("stopping with background work unfinished");
    }
    tracing::info!("stopped");
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
}
