//! Hypha's API on Cloudflare Workers (design note 0026): `hypha-http`'s routes, as on the
//! VPS, over Postgres through Hyperdrive, with the runtime's clock, randomness and
//! `fetch`. The database is set up beforehand (W9: `hypha-host-axum setup`, which
//! `hypha deploy` runs): this Worker runs no migrations and creates no store.
//!
//! An isolate builds its `AppState` and router once, on its first event (`Isolate`), and
//! serves every request with them: the configuration, the store, recently authenticated
//! publishable keys, the rate limiter's counts. What belongs to one event (its
//! `waitUntil`, whether it wrote) reaches the shared state through `Scoped`, which makes
//! the event current while its future is polled.
//!
//! The portable parts (`ids`, `limiter`, `schedule`) build natively, so their tests run
//! with the workspace's; the rest is Workers only.

pub mod ids;
pub mod limiter;
pub mod schedule;

#[cfg(target_arch = "wasm32")]
mod handlers;
#[cfg(target_arch = "wasm32")]
mod log;
#[cfg(target_arch = "wasm32")]
mod platform;
#[cfg(target_arch = "wasm32")]
mod storage;

#[cfg(target_arch = "wasm32")]
mod worker_main {
    use std::cell::{Cell, RefCell};
    use std::pin::Pin;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::task::{Context as TaskContext, Poll};
    use std::time::Duration;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Response, StatusCode};
    use futures_util::future::{FutureExt, LocalBoxFuture, Shared};
    use hypha_config::Config;
    use hypha_core::{Clock, Timestamp};
    use hypha_http::host::{AfterWrites, BoxFuture, HostBackground, HostLimiter};
    use hypha_http::jobs::{self, Step};
    use hypha_http::state::{AppState, HostEnv, HostParts, SharedState, Storage};
    use hypha_stores::StoreRepo;
    use tower_service::Service;
    use worker::{Context, Env, HttpRequest, ScheduleContext, ScheduledEvent, event};

    use crate::limiter::IsolateLimiter;
    use crate::platform;
    use crate::schedule::due;
    use crate::storage::WorkersStorage;

    /// The most steps one job takes in one run, so a backlog can't hold the event past
    /// its limits; the next tick goes on.
    const MAX_STEPS: usize = 20;

    /// How long after a response its jobs may run: `waitUntil` ends about 30 seconds
    /// after the response, and dispatch starts nothing that couldn't finish by then
    /// (`jobs::dispatch_by`).
    const AFTER_RESPONSE: Duration = Duration::from_secs(25);

    /// What an isolate builds once and keeps.
    struct Isolate {
        state: SharedState<WorkersStorage>,
        app: Router,
    }

    type Setup = Shared<LocalBoxFuture<'static, Result<Rc<Isolate>, String>>>;

    thread_local! {
        /// The isolate's setup: shared, so concurrent first events wait for one.
        static ISOLATE: RefCell<Option<Setup>> = const { RefCell::new(None) };
        /// The event whose future is being polled (`Scoped`).
        static CURRENT: RefCell<Option<Rc<Event>>> = const { RefCell::new(None) };
    }

    /// What belongs to one event: its `waitUntil`, and whether it wrote.
    struct Event {
        wait_until: Box<dyn Fn(BoxFuture<()>)>,
        wrote: Cell<bool>,
    }

    impl Event {
        fn new(wait_until: impl Fn(BoxFuture<()>) + 'static) -> Rc<Self> {
            Rc::new(Self {
                wait_until: Box::new(wait_until),
                wrote: Cell::new(false),
            })
        }
    }

    /// Polls `inner` with `event` current, so the shared state's background work and
    /// after-writes hook reach this event. An isolate interleaves events at their
    /// awaits; a poll is never interrupted, so each sees its own.
    struct Scoped<F> {
        event: Rc<Event>,
        inner: Pin<Box<F>>,
    }

    impl<F> Scoped<F> {
        fn new(event: &Rc<Event>, inner: F) -> Self {
            Self {
                event: Rc::clone(event),
                inner: Box::pin(inner),
            }
        }
    }

    impl<F: Future> Future for Scoped<F> {
        type Output = F::Output;

        fn poll(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<F::Output> {
            let this = self.get_mut();
            let outer = CURRENT.replace(Some(Rc::clone(&this.event)));
            let polled = this.inner.as_mut().poll(cx);
            CURRENT.set(outer);
            polled
        }
    }

    /// A Worker variable or secret.
    fn env_string(env: &Env, name: &str) -> Option<String> {
        env.var(name)
            .map(|v| v.to_string())
            .or_else(|_| env.secret(name).map(|v| v.to_string()))
            .ok()
    }

    /// The isolate, set up by its first event. A failed setup is tried again by the
    /// next event.
    async fn isolate(env: &Env) -> Result<Rc<Isolate>, String> {
        let setup = ISOLATE.with_borrow_mut(|slot| {
            slot.get_or_insert_with(|| set_up(env.clone()).boxed_local().shared())
                .clone()
        });
        let isolate = setup.await;
        if isolate.is_err() {
            ISOLATE.set(None);
        }
        isolate
    }

    /// Reads the configuration (design note 0026, W11: `HYPHA_CONFIG` holds the TOML
    /// itself, under the Worker's variables, as `hypha-config` reads the environment
    /// natively), finds the store and builds the state and the router.
    async fn set_up(env: Env) -> Result<Rc<Isolate>, String> {
        let level = env_string(&env, "HYPHA_LOG")
            .and_then(|l| l.parse().ok())
            .unwrap_or(tracing::Level::INFO);
        crate::log::init(level);
        let text = env_string(&env, "HYPHA_CONFIG").filter(|t| !t.trim().is_empty());
        let config = Config::load(text.as_deref(), |name| env_string(&env, name))
            .map_err(|e| format!("invalid configuration:\n{e}"))?;
        let secret_key = env_string(&env, "HYPHA_SECRET_KEY").ok_or(
            "HYPHA_SECRET_KEY is required (`wrangler secret put HYPHA_SECRET_KEY`)".to_string(),
        )?;
        let secret_key = hypha_http::state::parse_secret_key(&secret_key)
            .map_err(|e| format!("HYPHA_SECRET_KEY: {e}"))?;
        let runtime = platform::runtime();
        // An isolate's `env` is the same bindings for every event, so one is kept.
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
                "expected one store, found {}: set the database up with `hypha-host-axum setup` first",
                stores.len()
            ));
        };
        let media = hypha_http::media::Media::from_config(
            &config.media,
            |name| env_string(&env, name),
            || Ok(platform::host_http()),
            runtime.clock.clone(),
        )?;
        let limiter = Arc::new(IsolateLimiter::new(&config.rate_limits));
        let clock = runtime.clock.clone();
        let host = HostParts {
            env: HostEnv::new(
                secret_key,
                config.server.admin_url.clone(),
                config.auth,
                runtime,
            ),
            // Kept alive by the current event; outside one (never, in practice) it's
            // dropped and logged by the caller, like a full slot natively.
            background: HostBackground::new(|work| {
                CURRENT.with_borrow(|event| match event {
                    Some(event) => {
                        (event.wait_until)(work);
                        true
                    }
                    None => false,
                })
            }),
            limiter: HostLimiter::new(move |rule, key| {
                let decision = limiter.check(rule, &key, clock.now());
                Box::pin(async move { decision })
            }),
            after_writes: AfterWrites::new(|| {
                CURRENT.with_borrow(|event| {
                    if let Some(event) = event {
                        event.wrote.set(true);
                    }
                })
            }),
            calls: platform::host_calls(),
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
        let app = hypha_http::router!(WorkersStorage)
            .layer(axum::middleware::from_fn_with_state(
                state.after_writes.clone(),
                hypha_http::writes::after_writes,
            ))
            .with_state(Arc::clone(&state));
        Ok(Rc::new(Isolate { state, app }))
    }

    /// The API's error shape (`{"code", "error", "details"}`) for a Worker that can't
    /// serve: its configuration or database isn't ready. The reason is logged.
    fn unavailable(reason: &str) -> Response<Body> {
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
        let ctx = Rc::new(ctx);
        let event = {
            let ctx = Rc::clone(&ctx);
            Event::new(move |work| ctx.wait_until(work))
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
        let mut app = isolate.app.clone();
        let response = Scoped::new(&event, app.call(req)).await?;
        // A write's events go out after its response (W6), as the VPS's runner wakes.
        if event.wrote.get() {
            let state = Arc::clone(&isolate.state);
            let deadline = state.clock.now().plus(AFTER_RESPONSE);
            ctx.wait_until(Scoped::new(&event, async move {
                let jobs = state.config.jobs;
                if jobs.dispatch.enabled {
                    steps("dispatch", &state, Some(deadline), || {
                        jobs::dispatch_by(&state, deadline)
                    })
                    .await;
                }
                if jobs.search_sync.enabled {
                    steps("search sync", &state, Some(deadline), || {
                        jobs::search_sync(&state)
                    })
                    .await;
                }
            }));
        }
        Ok(response)
    }

    /// The cron tick (every minute, `wrangler.toml`): dispatch and search sync each
    /// time, the others on the ticks that start their interval (`schedule::due`).
    #[event(scheduled)]
    async fn scheduled(tick: ScheduledEvent, env: Env, ctx: ScheduleContext) {
        let isolate = match isolate(&env).await {
            Ok(isolate) => isolate,
            Err(e) => {
                worker::console_error!("{e}");
                return;
            }
        };
        let event = Event::new(move |work| ctx.wait_until(work));
        let state = Arc::clone(&isolate.state);
        let at = tick.schedule() as u64;
        Scoped::new(&event, async move {
            let jobs = state.config.jobs;
            if jobs.dispatch.enabled {
                steps("dispatch", &state, None, || jobs::dispatch(&state)).await;
            }
            if jobs.search_sync.enabled {
                steps("search sync", &state, None, || jobs::search_sync(&state)).await;
            }
            if jobs.prune.enabled && due(at, jobs.prune.interval) {
                steps("prune", &state, None, || jobs::prune(&state)).await;
            }
            if jobs.media_sweep.enabled
                && state.media.is_some()
                && due(at, jobs.media_sweep.interval)
            {
                steps("media sweep", &state, None, || jobs::media_sweep(&state)).await;
            }
        })
        .await;
    }

    /// Runs `step` while it says there's more, up to `MAX_STEPS` and, with one, until
    /// `deadline`.
    async fn steps<F: Future<Output = Step>>(
        job: &str,
        state: &AppState<WorkersStorage>,
        deadline: Option<Timestamp>,
        step: impl Fn() -> F,
    ) {
        for _ in 0..MAX_STEPS {
            match step().await {
                Ok(ran) if ran.more => {}
                result => {
                    report(job, result);
                    return;
                }
            }
            if deadline.is_some_and(|by| state.clock.now() >= by) {
                return;
            }
        }
    }

    fn report(job: &str, result: Step) {
        if let Err(e) = result {
            tracing::warn!(job, error = %e, "job step failed");
        }
    }
}
