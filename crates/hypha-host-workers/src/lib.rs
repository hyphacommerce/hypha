//! Hypha's API on Cloudflare Workers (design note 0026): `hypha-http`'s routes, as on the
//! VPS, over Postgres through Hyperdrive, with the runtime's clock, randomness and
//! `fetch`. The database is set up beforehand (W9: the native host's startup, later
//! `hypha … setup` from the CLI): this Worker runs no migrations and creates no store.
//!
//! What an isolate keeps between requests (`Isolate`): the configuration, the secret
//! key, the store, recently authenticated publishable keys, the rate limiter's counts
//! and the routes. Each request gets its own `AppState` around them, since its work
//! after the response is kept alive by its own event (`waitUntil`).
//!
//! The portable parts (`ids`, `limiter`) build natively, so their tests run with the
//! workspace's; the rest is Workers only.

pub mod ids;
pub mod limiter;

#[cfg(target_arch = "wasm32")]
mod log;
#[cfg(target_arch = "wasm32")]
mod platform;
#[cfg(target_arch = "wasm32")]
mod storage;

#[cfg(target_arch = "wasm32")]
mod worker_main {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Response, StatusCode};
    use hypha_config::Config;
    use hypha_core::StoreId;
    use hypha_http::host::{AfterWrites, BoxFuture, HostLimiter};
    use hypha_http::jobs::{self, Step};
    use hypha_http::key_cache::KeyCache;
    use hypha_http::state::{AppState, HostEnv, HostParts, Runtime, SharedState, Storage};
    use hypha_stores::StoreRepo;
    use tower_service::Service;
    use worker::send::SendWrapper;
    use worker::{Context, Env, HttpRequest, ScheduleContext, ScheduledEvent, event};

    use crate::limiter::IsolateLimiter;
    use crate::platform;
    use crate::storage::WorkersStorage;

    /// The cron that also runs the hourly jobs (`prune`, `media_sweep`); every other
    /// trigger runs dispatch and search sync (`wrangler.toml`).
    const HOURLY: &str = "0 * * * *";

    /// The most steps one job takes in one run, so a backlog can't hold the event past
    /// its limits; the next trigger goes on.
    const MAX_STEPS: usize = 20;

    /// What an isolate reads once and keeps.
    struct Isolate {
        config: Config,
        secret_key: [u8; 32],
        runtime: Runtime,
        store: StoreId,
        store_name: String,
        publishable_keys: KeyCache,
        limiter: Arc<IsolateLimiter>,
        routes: Router<SharedState<WorkersStorage>>,
    }

    thread_local! {
        static ISOLATE: RefCell<Option<Rc<Isolate>>> = const { RefCell::new(None) };
    }

    /// A Worker variable or secret.
    fn env_string(env: &Env, name: &str) -> Option<String> {
        env.var(name)
            .map(|v| v.to_string())
            .or_else(|_| env.secret(name).map(|v| v.to_string()))
            .ok()
    }

    /// The isolate's state, read on its first request (design note 0026, W11): the
    /// configuration from `HYPHA_CONFIG` (the TOML itself) under the Worker's variables,
    /// as `hypha-config` reads the environment natively.
    async fn isolate(env: &Env) -> Result<Rc<Isolate>, String> {
        if let Some(isolate) = ISOLATE.with_borrow(Clone::clone) {
            return Ok(isolate);
        }
        let level = env_string(env, "HYPHA_LOG")
            .and_then(|l| l.parse().ok())
            .unwrap_or(tracing::Level::INFO);
        crate::log::init(level);
        let text = env_string(env, "HYPHA_CONFIG").filter(|t| !t.trim().is_empty());
        let config = Config::load(text.as_deref(), |name| env_string(env, name))
            .map_err(|e| format!("invalid configuration:\n{e}"))?;
        let secret_key = env_string(env, "HYPHA_SECRET_KEY").ok_or(
            "HYPHA_SECRET_KEY is required (`wrangler secret put HYPHA_SECRET_KEY`)".to_string(),
        )?;
        let secret_key = hypha_http::state::parse_secret_key(&secret_key)
            .map_err(|e| format!("HYPHA_SECRET_KEY: {e}"))?;
        let runtime = platform::runtime();
        let storage = WorkersStorage::new(env.clone(), runtime.clock.clone());
        // One store per deployment (design note 0006, model A), made by setup.
        let stores = storage
            .directory()
            .await
            .map_err(|_| "cannot reach the database".to_string())?
            .list_all()
            .await
            .map_err(|e| e.to_string())?;
        let [store] = stores.as_slice() else {
            return Err(format!(
                "expected one store, found {}: set the database up with the host's setup first",
                stores.len()
            ));
        };
        let isolate = Rc::new(Isolate {
            limiter: Arc::new(IsolateLimiter::new(&config.rate_limits)),
            config,
            secret_key,
            runtime,
            store: store.id(),
            store_name: store.name().to_string(),
            publishable_keys: KeyCache::default(),
            routes: hypha_http::router!(WorkersStorage),
        });
        ISOLATE.set(Some(Rc::clone(&isolate)));
        Ok(isolate)
    }

    impl Isolate {
        /// A request's state: the isolate's parts, with `wait_until` for its work after
        /// the response and `after_writes` called after a successful write.
        fn state(
            &self,
            env: &Env,
            wait_until: impl Fn(BoxFuture<()>) + Send + Sync + 'static,
            after_writes: AfterWrites,
        ) -> Result<AppState<WorkersStorage>, String> {
            let media = hypha_http::media::Media::from_config(
                &self.config.media,
                |name| env_string(env, name),
                || Ok(platform::host_http()),
                self.runtime.clock.clone(),
            )?;
            let limiter = Arc::clone(&self.limiter);
            let clock = self.runtime.clock.clone();
            let host = HostParts {
                env: HostEnv::new(
                    self.secret_key,
                    self.config.server.admin_url.clone(),
                    self.config.auth,
                    self.runtime.clone(),
                ),
                background: platform::background(wait_until),
                limiter: HostLimiter::new(move |rule, key| {
                    use hypha_core::Clock;
                    let decision = limiter.check(rule, &key, clock.now());
                    Box::pin(async move { decision })
                }),
                after_writes,
                calls: platform::host_calls(),
                media,
                publishable_keys: self.publishable_keys.clone(),
            };
            Ok(AppState::new(
                WorkersStorage::new(env.clone(), self.runtime.clock.clone()),
                self.store,
                self.store_name.clone(),
                host,
                self.config.clone(),
            ))
        }
    }

    /// The API's error shape (`{"code", "error", "details"}`) for a Worker that can't
    /// serve: its configuration or database isn't ready. The reason is logged.
    fn unavailable(reason: &str) -> Response<Body> {
        tracing::error!("{reason}");
        worker::console_error!("{reason}");
        let body = r#"{"code":"unavailable","error":"the API isn't available; see its logs","details":null}"#;
        Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap_or_default()
    }

    #[event(fetch)]
    async fn fetch(mut req: HttpRequest, env: Env, ctx: Context) -> worker::Result<Response<Body>> {
        let isolate = match isolate(&env).await {
            Ok(isolate) => isolate,
            Err(e) => return Ok(unavailable(&e)),
        };
        let ctx = Arc::new(ctx);
        let wrote = Arc::new(AtomicBool::new(false));
        let after_writes = {
            let wrote = Arc::clone(&wrote);
            AfterWrites::new(move || wrote.store(true, Ordering::Relaxed))
        };
        let wait = Arc::clone(&ctx);
        let state = match isolate.state(&env, move |work| wait.wait_until(work), after_writes) {
            Ok(state) => Arc::new(state),
            Err(e) => return Ok(unavailable(&e)),
        };
        // Cloudflare sets `CF-Connecting-IP` to the client's address; clients can't.
        if let Some(ip) = req
            .headers()
            .get("cf-connecting-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
        {
            req.extensions_mut()
                .insert(hypha_http::client_ip::PeerAddr(ip));
        }
        let mut app = isolate
            .routes
            .clone()
            .layer(axum::middleware::from_fn_with_state(
                state.after_writes.clone(),
                hypha_http::writes::after_writes,
            ))
            .with_state(Arc::clone(&state));
        let response = app.call(req).await?;
        // A write's events go out after its response (W6), as the VPS's runner wakes.
        if wrote.load(Ordering::Relaxed) {
            ctx.wait_until(async move {
                if state.config.jobs.dispatch.enabled {
                    steps("dispatch", || jobs::dispatch(&state)).await;
                }
                if state.config.jobs.search_sync.enabled {
                    steps("search sync", || jobs::search_sync(&state)).await;
                }
            });
        }
        Ok(response)
    }

    #[event(scheduled)]
    async fn scheduled(event: ScheduledEvent, env: Env, ctx: ScheduleContext) {
        let isolate = match isolate(&env).await {
            Ok(isolate) => isolate,
            Err(e) => {
                worker::console_error!("{e}");
                return;
            }
        };
        let ctx = Arc::new(SendWrapper::new(ctx));
        let state = match isolate.state(
            &env,
            move |work| ctx.0.wait_until(work),
            AfterWrites::none(),
        ) {
            Ok(state) => state,
            Err(e) => {
                worker::console_error!("{e}");
                return;
            }
        };
        let jobs = state.config.jobs;
        if event.cron() == HOURLY {
            if jobs.prune.enabled {
                steps("prune", || jobs::prune(&state)).await;
            }
            if jobs.media_sweep.enabled && state.media.is_some() {
                steps("media sweep", || jobs::media_sweep(&state)).await;
            }
            return;
        }
        if jobs.dispatch.enabled {
            steps("dispatch", || jobs::dispatch(&state)).await;
        }
        if jobs.search_sync.enabled {
            steps("search sync", || jobs::search_sync(&state)).await;
        }
    }

    /// Runs `step` while it says there's more, up to `MAX_STEPS`.
    async fn steps<F: Future<Output = Step>>(job: &str, step: impl Fn() -> F) {
        for _ in 0..MAX_STEPS {
            match step().await {
                Ok(ran) if ran.more => {}
                result => {
                    report(job, result);
                    return;
                }
            }
        }
    }

    fn report(job: &str, result: Step) {
        if let Err(e) = result {
            tracing::warn!(job, error = %e, "job step failed");
        }
    }
}
