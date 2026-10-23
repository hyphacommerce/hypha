//! What the host binary does (`host!`'s `main`): the subcommands (`setup`, `config`,
//! `openapi`), then startup (configuration, migrations, the store, the owner, the
//! location, the search index) and serving, for a deployment's spores (`Deployment`).

use std::sync::Arc;

use axum::Router;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use hypha_config::Config;
use hypha_core::Ctx;
use hypha_inventory::{HasLocations, ensure_default_location};
use hypha_stores::ensure_single_store;
use hypha_users::{HasUsers, NewOwner, UserRepo, bootstrap_owner};

use hypha_http::host::{HostLimiter, HostMail};
use hypha_http::mail::{MailHost, MailParts};
use hypha_http::rate_limit::{RateLimiter, Rule};
use hypha_http::state::{AppState, HostEnv, HostParts, Runtime, SharedState, Storage};

use crate::limiter::MemoryRateLimiter;
use crate::platform::Background;
use crate::storage::{PgDatabase, PgStorage};
use hypha_spore_core::SporeSet;

use crate::{Deployment, calls, email, platform, runner};

/// `hypha-host-axum openapi [DIR]` writes the OpenAPI documents to `DIR` (default
/// `openapi`) and exits. Run it after changing routes or DTOs; a test fails while the
/// committed files are out of date. A build with spores documents their routes too, a tag
/// per spore; the committed files are the stock build's.
fn write_openapi<D: Deployment>(dir: &str) -> std::io::Result<()> {
    use hypha_http::routes::openapi::{
        Doc, EXTENSIONS_FILE, document_json_with, extensions_document_json,
    };
    std::fs::create_dir_all(dir)?;
    let documents = Doc::ALL
        .map(|doc| (doc.file_name(), document_json_with(doc, D::spore_rows())))
        .into_iter()
        .chain([(EXTENSIONS_FILE, extensions_document_json())]);
    for (file, json) in documents {
        let path = std::path::Path::new(dir).join(file);
        std::fs::write(&path, json)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// Reads the configuration: the file `HYPHA_CONFIG` names, else `hypha.config.toml` if it's
/// there, under the environment. `Ok` with the file read, if any.
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
/// the file's JSON Schema to `PATH` (default `hypha.config.schema.json`; a test fails while
/// the committed one is stale); `config rate-limits` prints a Worker's Rate Limiting
/// bindings under the quotas in effect, as JSON (`{"slots": 100, "bindings": [{"rule":
/// "login_ip", "binding": "RL_LOGIN_IP", "slot": 0, "limit": 12, "period": 60}, …]}`),
/// which `hypha deploy` copies into `wrangler.toml`.
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

/// The binary: `host!`'s `main`. Runs on a multi-threaded Tokio runtime, as
/// `#[tokio::main]` would.
pub fn run<D: Deployment>() {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("cannot start the Tokio runtime: {e}");
            std::process::exit(1);
        }
    };
    runtime.block_on(main::<D>());
}

async fn main<D: Deployment>() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("openapi") => {
            let dir = args.get(2).map_or("openapi", String::as_str);
            if let Err(e) = write_openapi::<D>(dir) {
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
    // `setup` prepares the database as a start would, then exits: for hosts that can't,
    // such as a Worker.
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
    // Built now so a missing key or password stops startup.
    let mail_sender = match email::email_sender(&config.email, |name| std::env::var(name).ok()) {
        Ok(mail) => mail,
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
    // A spore's name is its key, its migration module and its tables' prefix: a bad one
    // stops the build here, before anything is migrated.
    let mut problems = hypha_spore_core::check::<D::Spores>();
    problems.extend(hypha_spore_core::check_config::<D::Spores>(&config.spores));
    problems.extend(crate::check_routes::<D>());
    if !problems.is_empty() {
        tracing::error!("this build's spores:\n{}", problems.join("\n"));
        std::process::exit(1);
    }
    let runtime = platform::runtime();
    let migrations = crate::migrations::<D>();
    let db = match PgDatabase::connect(&url, &config.database, runtime.clock.clone(), &migrations)
        .await
    {
        Ok(db) => db,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };
    tracing::info!("storage: Postgres (migrations applied)");
    let env = HostEnv::new(secret_key, config.auth, runtime.clone())
        .with_accounts(config.accounts)
        .with_staff(config.staff.clone());
    let prepared = prepare::<D>(db, env, mail_sender, runtime, config).await;
    if setup_only {
        tracing::info!("setup done: migrations, the store, the owner, the location, search");
        return;
    }
    serve(prepared).await;
}

/// With no staff user yet, `HYPHA_OWNER_EMAIL` and `HYPHA_OWNER_PASSWORD` (and optionally
/// `HYPHA_OWNER_NAME`) create the owner. Ignored once anyone exists.
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
/// stock.
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

/// An empty engine (a new database, or a new engine) is filled by a full rebuild: the sync
/// job works it like any other mark.
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

/// Each spore this host is built with gets its app, as the system: created with its scopes
/// the first time, its title and scopes brought up to date after; then its custom fields
/// are synced (`Spore::custom_fields`).
pub(crate) async fn register_spores<D: Deployment>(
    state: &AppState<PgStorage<D>>,
) -> Result<(), String> {
    use hypha_apps::HasApps;
    let spores = <D::Spores as SporeSet>::registrations();
    if spores.is_empty() {
        return Ok(());
    }
    let ports = state
        .public_ports()
        .await
        .map_err(|_| "cannot reach storage")?;
    for spore in spores {
        let app =
            hypha_apps::register_spore(&Ctx::system(), ports.apps(), &state.env, &state.env, spore)
                .await
                .map_err(|e| format!("spore {:?}: {e}", spore.name))?;
        tracing::info!(app = %app.key(), enabled = app.enabled(), "spore");
    }
    // Their custom fields, in the namespaces of the apps just registered: a changed type
    // stops the start, naming the spore and the field.
    hypha_spore_core::sync_custom_fields::<D::Spores, _>(&ports, &state.env).await
}

/// Handler names and provider keys are how deliveries and payment methods find them, so
/// two alike (a spore's and core's, or two spores') would be ambiguous, and a spore's
/// provider under a key that isn't its own would never be reached: the host refuses
/// (`hypha_spore_core::check_built`, which the Worker runs too).
fn check_unique<D: Deployment>(storage: &PgStorage<D>) -> Result<(), String> {
    use hypha_core::EventHandlers;
    use hypha_payments::PaymentProviders;
    let names = EventHandlers::<crate::PgPorts>::names(storage.handlers());
    let providers = storage.payment_providers();
    let problems =
        hypha_spore_core::check_built::<D::Spores>(&names, &providers.keys(), &providers.0.1);
    match problems.is_empty() {
        true => Ok(()),
        false => Err(problems.join("\n")),
    }
}

/// What a start prepares before serving: the state, with the store, the owner, the
/// default location and the search index set up, and the host pieces `serve` runs.
pub(crate) struct Prepared<D: Deployment> {
    pub(crate) state: SharedState<PgStorage<D>>,
    background: Background,
    limiter: Arc<MemoryRateLimiter>,
    wake: runner::Wake,
}

/// Everything a start does before listening, which `setup` does alone: the store (one per
/// deployment), the media bucket if asked for, the owner, the default location and the
/// search index; then the handlers, whose `Mailer` needs the store, and each spore's app.
/// Migrations ran in `PgDatabase::connect`. Exits on failure.
pub(crate) async fn prepare<D: Deployment>(
    db: PgDatabase,
    env: HostEnv,
    mail_sender: HostMail,
    runtime: Runtime,
    config: Config,
) -> Prepared<D> {
    let ids = runtime.ids.clone();
    // One store per deployment (model A): created on first start.
    let name = std::env::var("HYPHA_STORE_NAME").unwrap_or_else(|_| "My store".to_string());
    let store = match db.store().await {
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
        Ok(media) => media.map(Arc::new),
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
    let mail = Arc::new(MailHost::new(
        MailParts {
            sender: mail_sender,
            from: config.email.from.clone(),
            store: store.id(),
            admin_url: config.server.admin_url.clone(),
            order_tokens: env.order_tokens().clone(),
            media: media.clone(),
            queued: wake.mail_queued(),
        },
        &runtime,
    ));
    // Each spore is built with the host's capabilities and its `[spores.<name>]` section;
    // its HTTP goes through the client apps are called with.
    let spores = hypha_http::spores::spore_host(&env, calls::host_calls(client.clone()), outbound);
    let storage = match PgStorage::<D>::new(db, Arc::clone(&mail), &spores, &config.spores) {
        Ok(storage) => storage,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = check_unique(&storage) {
        tracing::error!("{e}");
        std::process::exit(1);
    }
    let host = HostParts {
        env,
        background: background.host(),
        limiter: host_limiter(Arc::clone(&limiter)),
        after_writes: wake.after_writes(),
        calls: calls::host_calls(client),
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
    // What agents' calls and approved proposals are sent through, in process.
    state.install(D::router(state.config.server.max_body), D::spore_rows());
    if let Err(e) = register_spores(&state).await {
        tracing::error!("spore setup failed: {e}");
        std::process::exit(1);
    }
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

async fn serve<D: Deployment>(prepared: Prepared<D>) {
    let Prepared {
        state,
        background,
        limiter,
        wake,
    } = prepared;
    let config = &state.config;
    let trusted_proxies = &config.server.trusted_proxies;
    // The one setting that silently goes wrong behind a proxy.
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
    let router: Router<_> = D::router(config.server.max_body);
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
pub(crate) async fn record_peer(
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
pub(crate) fn host_limiter(limiter: Arc<MemoryRateLimiter>) -> HostLimiter {
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
