use std::sync::Arc;

use axum::Router;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use hypha_config::Config;
use hypha_core::Ctx;
use hypha_inventory::{HasLocations, ensure_default_location};
use hypha_stores::ensure_single_store;
use hypha_users::{HasUsers, NewOwner, UserRepo, bootstrap_owner};

use hypha_http::host::HostLimiter;
use hypha_http::rate_limit::{RateLimiter, Rule};
use hypha_http::state::{AppState, HostEnv, HostParts, Runtime, SharedState, Storage};

mod calls;
mod handlers;
#[cfg(test)]
mod handlers_test;
mod limiter;
mod platform;
#[cfg(test)]
mod route_tests;
mod runner;
mod storage;

use limiter::MemoryRateLimiter;
use platform::Background;
use storage::PgStorage;

/// `hypha-host-axum openapi [DIR]` writes the OpenAPI documents to `DIR` (default
/// `openapi`) and exits (design note 0010, S6). Run it after changing routes or DTOs;
/// a test fails while the committed files are out of date.
fn write_openapi(dir: &str) -> std::io::Result<()> {
    use hypha_http::routes::openapi::{Doc, document_json};
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
/// while the committed one is stale); `config rate-limits` prints a Worker's Rate
/// Limiting bindings under the quotas in effect, as JSON (`{"slots": 100, "bindings":
/// [{"rule": "login_ip", "binding": "RL_LOGIN_IP", "slot": 0, "limit": 12, "period":
/// 60}, …]}`), which `hypha deploy` copies into `wrangler.toml` (design note 0026, W8).
fn config_command(args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) == Some("rate-limits") {
        use hypha_http::rate_limit::{BINDING_SLOTS, BINDING_WINDOW};
        let (config, _) = load_config()?;
        let limits = &config.rate_limits;
        let bindings: Vec<serde_json::Value> = Rule::ALL
            .into_iter()
            .map(|rule| {
                serde_json::json!({
                    "rule": rule.config_key(),
                    "binding": rule.binding(),
                    "slot": rule.binding_slot(),
                    "limit": rule.binding_limit(limits),
                    "period": BINDING_WINDOW.as_secs(),
                })
            })
            .collect();
        let out = serde_json::json!({ "slots": BINDING_SLOTS, "bindings": bindings });
        println!("{out}");
        return Ok(());
    }
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
        Some("setup") | None => {}
        Some(other) => {
            eprintln!("unknown command {other:?}: expected setup, config or openapi");
            std::process::exit(1);
        }
    }
    // `setup` prepares the database as a start would, then exits (design note 0026,
    // W9): for hosts that can't, such as a Worker.
    let setup_only = args.get(1).is_some();

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
        .map(|k| hypha_http::state::parse_secret_key(&k))
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
    let runtime = platform::runtime();
    let storage = match PgStorage::connect(&url, &config.database, runtime.clock.clone()).await {
        Ok(storage) => storage,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };
    tracing::info!("storage: Postgres (migrations applied)");
    let env = HostEnv::new(
        secret_key,
        config.server.admin_url.clone(),
        config.auth,
        runtime.clone(),
    );
    let prepared = prepare(storage, env, runtime, config).await;
    if setup_only {
        tracing::info!("setup done: migrations, the store, the owner, the location, search");
        return;
    }
    serve(prepared).await;
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
                 the owner, then restart (or run setup)"
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

/// An empty engine (a new database, or a new engine) is filled by a full rebuild: the
/// sync job works it like any other mark (design note 0019, section 5).
async fn ensure_search_index<S: Storage>(state: &AppState<S>) -> Result<(), String> {
    use hypha_search_index::SearchIndex;
    let ports = state
        .public_ports()
        .await
        .map_err(|_| "cannot reach storage")?;
    let documents = state
        .storage
        .search(&ports)
        .count()
        .await
        .map_err(|e| e.to_string())?;
    if documents == 0 {
        hypha_search_index::queue_all(&ports)
            .await
            .map_err(|e| e.to_string())?;
        tracing::info!("search: the index is empty, rebuilding it");
    }
    Ok(())
}

/// What a start prepares before serving: the state, with the store, the owner, the
/// default location and the search index set up, and the host pieces `serve` runs.
struct Prepared {
    state: SharedState<PgStorage>,
    background: Background,
    limiter: Arc<MemoryRateLimiter>,
    wake: runner::Wake,
}

/// Everything a start does before listening, which `setup` does alone: the store (one per
/// deployment), the media bucket if asked for, the owner, the default location and the
/// search index. Migrations ran in `PgStorage::connect`. Exits on failure.
async fn prepare(storage: PgStorage, env: HostEnv, runtime: Runtime, config: Config) -> Prepared {
    let ids = runtime.ids.clone();
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
    tracing::info!(store = %store.id(), name = store.name(), "store");

    let media = match hypha_http::media::Media::from_config(
        &config.media,
        |name| std::env::var(name).ok(),
        platform::host_http,
        runtime.clock.clone(),
    ) {
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

    let outbound = hypha_http::outbound::OutboundPolicy {
        allow_private: config.allow_private_callbacks,
    };
    if outbound.allow_private {
        tracing::warn!(
            "outbound.allow_private: apps may be called at private addresses; development only"
        );
    }
    let background = Background::default();
    let limiter = Arc::new(MemoryRateLimiter::new(&config.rate_limits));
    let wake = runner::Wake::default();
    let client = match calls::client(outbound) {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };
    let host = HostParts {
        env,
        background: background.host(),
        limiter: host_limiter(Arc::clone(&limiter)),
        after_writes: wake.after_writes(),
        calls: calls::host_calls(client),
        media,
        publishable_keys: Default::default(),
    };
    let state = Arc::new(AppState::new(
        storage,
        store.id(),
        store.name().to_string(),
        host,
        config,
    ));
    if let Err(e) = ensure_owner(&state).await {
        tracing::error!("owner setup failed: {e}");
        std::process::exit(1);
    }
    if let Err(e) = ensure_location(&state).await {
        tracing::error!("location setup failed: {e}");
        std::process::exit(1);
    }
    if let Err(e) = ensure_search_index(&state).await {
        tracing::error!("search setup failed: {e}");
        std::process::exit(1);
    }
    Prepared {
        state,
        background,
        limiter,
        wake,
    }
}

async fn serve(prepared: Prepared) {
    let Prepared {
        state,
        background,
        limiter,
        wake,
    } = prepared;
    let config = &state.config;
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
    if !config.jobs.dispatch.enabled {
        tracing::info!("jobs: events aren't delivered from this instance (jobs.dispatch)");
    }
    let grace = config.server.shutdown_grace;
    let addr = config.server.bind.clone();
    tracing::warn!("emails (invites, password resets) are logged, not sent: development only");
    let router: Router<_> = hypha_http::router!(PgStorage);
    let app = router
        .layer(axum::middleware::from_fn(record_peer))
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
    let jobs = runner::start(Arc::clone(&state), wake, limiter);
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
    if !background.drain(grace).await {
        tracing::warn!("stopping with background work unfinished");
    }
    tracing::info!("stopped");
}

/// Records each request's peer address (`hypha_http::client_ip::PeerAddr`) from the
/// socket (`into_make_service_with_connect_info`; tests set it with `MockConnectInfo`).
async fn record_peer(
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    request
        .extensions_mut()
        .insert(hypha_http::client_ip::PeerAddr(peer.ip()));
    next.run(request).await
}

/// The in-memory limiter as `AppState`'s `HostLimiter`.
fn host_limiter(limiter: Arc<MemoryRateLimiter>) -> HostLimiter {
    HostLimiter::new(move |rule, key| {
        let limiter = Arc::clone(&limiter);
        Box::pin(async move { limiter.check(rule, &key).await })
    })
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
