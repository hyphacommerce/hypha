//! Hypha's API on Cloudflare Workers (design note 0026): `hypha-http`'s routes, as on the
//! VPS, over Postgres through Hyperdrive, with the runtime's clock, randomness and
//! `fetch`. The database is set up beforehand (W9: `hypha-host-axum setup`, which
//! `hypha deploy` runs): this Worker runs no migrations and creates no store.
//!
//! An isolate builds its `AppState` and router once, on its first event (`Isolate`), and
//! serves every request with them: the configuration, the store, recently authenticated
//! publishable keys, the rate limiter's counts and its bindings. What belongs to one event (its
//! `waitUntil`, whether it wrote) reaches the shared state through `Scoped`, which makes
//! the event current while its future is polled.
//!
//! Jobs run after each write that gave them work (W6: events committed, a delivery
//! retried; the ports say so through `AfterWrites`) and on a cron tick that touches the database only when
//! KV's marks say there's work (W7, `marks`), so an idle serverless database can sleep.
//!
//! The portable parts (`ids`, `limiter`, `marks`, `schedule`) build natively, so their
//! tests run with the workspace's; the rest is Workers only.

pub mod ids;
pub mod limiter;
pub mod marks;
pub mod peer;
pub mod schedule;

#[cfg(target_arch = "wasm32")]
mod email;
#[cfg(target_arch = "wasm32")]
mod handlers;
#[cfg(target_arch = "wasm32")]
mod log;
#[cfg(target_arch = "wasm32")]
mod payments;
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
    use hypha_http::jobs::{self, Ran, Step};
    use hypha_http::rate_limit::{Decision, Rule};
    use hypha_http::state::{AppState, HostEnv, HostParts, SharedState, Storage};
    use hypha_stores::StoreRepo;
    use tower_service::Service;
    use worker::{
        Context, Env, HttpRequest, KvStore, RateLimiter, ScheduleContext, ScheduledEvent, event,
    };

    use crate::email;
    use crate::limiter::IsolateLimiter;
    use crate::marks::{self, After, Marks, Run};
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

    /// The time left before a deadline under which no step starts: one that ran past
    /// `waitUntil`'s end would be cut off before its work or marks are written.
    const STEP_TIME: Duration = Duration::from_secs(5);

    /// What an isolate builds once and keeps.
    struct Isolate {
        state: SharedState<WorkersStorage>,
        app: Router,
        /// The deployment's caller key (`HYPHA_CALLER_KEY`): the admin and storefront
        /// Workers send it, so their shoppers' addresses are believed (`peer`).
        caller_key: Option<String>,
    }

    type Setup = Shared<LocalBoxFuture<'static, Result<Rc<Isolate>, String>>>;

    thread_local! {
        /// The isolate's setup: shared, so concurrent first events wait for one.
        static ISOLATE: RefCell<Option<Setup>> = const { RefCell::new(None) };
        /// The event whose future is being polled (`Scoped`).
        static CURRENT: RefCell<Option<Rc<Event>>> = const { RefCell::new(None) };
        /// The `due` mark this isolate last read or wrote, so runs after writes don't
        /// write the same one again.
        static LAST_DUE: Cell<Option<u64>> = const { Cell::new(None) };
    }

    /// The KV namespace holding the jobs' marks (W7). Without one, every tick runs the
    /// jobs.
    const JOBS_KV: &str = "JOBS";

    /// What belongs to one event: its `waitUntil`, and whether a write in it gave the
    /// dispatcher work (`AfterWrites`: events committed, a delivery retried).
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
        let isolate = setup.clone().await;
        // Only this setup: another event may have started a newer one meanwhile.
        if isolate.is_err() {
            ISOLATE.with_borrow_mut(|slot| {
                if slot.as_ref().is_some_and(|s| s.ptr_eq(&setup)) {
                    *slot = None;
                }
            });
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
        // Built now so a missing key (or `smtp`) stops setup; the next stage (design note
        // 0028) passes it to the state.
        let _mail = email::email_sender(&config.email, |name| env_string(&env, name))?;
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
        let bindings = Arc::new(rate_limit_bindings(&env));
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
                let bindings = Arc::clone(&bindings);
                Box::pin(async move {
                    match &bindings[rule as usize] {
                        Some(binding) if decision == Decision::Allowed => {
                            across_isolates(binding, rule, key).await
                        }
                        _ => decision,
                    }
                })
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
        let app = hypha_http::router!(WorkersStorage).with_state(Arc::clone(&state));
        Ok(Rc::new(Isolate {
            state,
            app,
            caller_key: env_string(&env, "HYPHA_CALLER_KEY"),
        }))
    }

    /// The Rate Limiting bindings (W8), at `rule as usize`; a rule without one counts
    /// in the isolate only, with a warning.
    fn rate_limit_bindings(env: &Env) -> [Option<RateLimiter>; Rule::ALL.len()] {
        Rule::ALL.map(|rule| {
            let name = rule.binding();
            let binding = env.rate_limiter(name).ok();
            if binding.is_none() {
                tracing::warn!(
                    binding = name,
                    "rate limits: no binding, counting per isolate only"
                );
            }
            binding
        })
    }

    /// Asks `rule`'s binding, which counts every isolate's attempts in fixed windows.
    /// Over it, the client waits for the window to pass; a binding that fails lets the
    /// attempt through (the isolate's count has allowed it).
    async fn across_isolates(binding: &RateLimiter, rule: Rule, key: String) -> Decision {
        match binding.limit(key).await {
            Ok(outcome) if outcome.success => Decision::Allowed,
            Ok(_) => Decision::Limited {
                retry_after: hypha_http::rate_limit::BINDING_WINDOW,
            },
            Err(e) => {
                let name = rule.binding();
                tracing::warn!(binding = name, error = %e, "rate limits: the binding failed");
                Decision::Allowed
            }
        }
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
        // From the internet, `CF-Connecting-IP`; from one of our Workers, the caller's
        // `X-Forwarded-For` (`peer`).
        let header = |name: &str| req.headers().get(name).and_then(|v| v.to_str().ok());
        let ours = crate::peer::is_ours(
            isolate.caller_key.as_deref(),
            header(crate::peer::CALLER_HEADER),
        );
        let ip = crate::peer::client_address(
            ours,
            header("cf-connecting-ip"),
            header("x-forwarded-for"),
        );
        req.headers_mut().remove(crate::peer::CALLER_HEADER);
        if let Some(ip) = ip {
            req.extensions_mut()
                .insert(hypha_http::client_ip::PeerAddr(ip));
        }
        let mut app = isolate.app.clone();
        let response = Scoped::new(&event, app.call(req)).await?;
        // A write's events go out after its response (W6), as the VPS's runner wakes. A
        // request whose writes recorded none (a cart's) runs nothing: its run would find
        // nothing and write no marks (`marks::after`), and what others left is the
        // ticks' to find.
        if event.wrote.get() {
            let state = Arc::clone(&isolate.state);
            let deadline = state.clock.now().plus(AFTER_RESPONSE);
            let kv = env.kv(JOBS_KV).ok();
            ctx.wait_until(Scoped::new(&event, async move {
                let jobs = state.config.jobs;
                let mut run = Run::default();
                if jobs.dispatch.enabled {
                    run = run.and(
                        steps("dispatch", &state, Some(deadline), || {
                            jobs::dispatch_by(&state, deadline)
                        })
                        .await,
                    );
                }
                if jobs.search_sync.enabled {
                    run = run.and(
                        steps("search sync", &state, Some(deadline), || {
                            search_step(&state)
                        })
                        .await,
                    );
                }
                if let Some(kv) = kv {
                    mark(&kv, &state, marks::after(run, None), LAST_DUE.get()).await;
                }
            }));
        }
        Ok(response)
    }

    /// The cron tick (every minute, `wrangler.toml`): dispatch and search sync when
    /// KV's marks say there's work (or on the safety net's ticks), the others on the
    /// ticks that start their interval (`schedule::due`).
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
        let kv = env.kv(JOBS_KV).ok();
        Scoped::new(&event, async move {
            let jobs = state.config.jobs;
            let started = millis(state.clock.now());
            let read = match &kv {
                Some(kv) => read_marks(kv).await,
                None => None,
            };
            let waiting = kv.is_none()
                || due(at, marks::NET)
                || read.is_none_or(|marks| marks.waiting(started));
            if waiting {
                let mut run = Run::default();
                if jobs.dispatch.enabled {
                    run = run.and(steps("dispatch", &state, None, || jobs::dispatch(&state)).await);
                }
                if jobs.search_sync.enabled {
                    run = run.and(steps("search sync", &state, None, || search_step(&state)).await);
                }
                if let Some(kv) = &kv {
                    let known = read.and_then(|marks| marks.due);
                    mark(kv, &state, marks::after(run, Some(started)), known).await;
                }
            } else {
                tracing::debug!("jobs: nothing waiting");
            }
            // An interval job that left work goes on at the next tick, not its next interval,
            // as the native runner repeats while there's more (`marks::BACKLOG`).
            let backlog = match &kv {
                Some(kv) => kv.get(marks::BACKLOG).text().await.ok().flatten().is_some(),
                None => false,
            };
            let mut left = false;
            if jobs.prune.enabled && (backlog || due(at, jobs.prune.interval)) {
                left |= steps("prune", &state, None, || jobs::prune(&state))
                    .await
                    .left;
            }
            if jobs.media_sweep.enabled
                && state.media.is_some()
                && (backlog || due(at, jobs.media_sweep.interval))
            {
                left |= steps("media sweep", &state, None, || jobs::media_sweep(&state))
                    .await
                    .left;
            }
            if let Some(kv) = &kv {
                if left {
                    put(kv, marks::BACKLOG, started).await;
                } else if backlog && let Err(e) = kv.delete(marks::BACKLOG).await {
                    tracing::warn!(error = %e, "jobs: cannot clear the backlog mark");
                }
            }
        })
        .await;
    }

    /// Runs `step` while it says there's more, up to `MAX_STEPS` and, with one, until
    /// `deadline`; says whether it found work and whether it left some.
    async fn steps<F: Future<Output = Step>>(
        job: &str,
        state: &AppState<WorkersStorage>,
        deadline: Option<Timestamp>,
        step: impl Fn() -> F,
    ) -> Run {
        let mut run = Run::default();
        for _ in 0..MAX_STEPS {
            if deadline.is_some_and(|by| state.clock.now().plus(STEP_TIME) > by) {
                break;
            }
            match step().await {
                Ok(ran) => {
                    run.worked |= !ran.idle;
                    run.retry_at = marks::earliest(run.retry_at, ran.retry_at.map(millis));
                    if !ran.more {
                        return run;
                    }
                }
                Err(e) => {
                    tracing::warn!(job, error = %e, "job step failed");
                    run.left = true;
                    return run;
                }
            }
        }
        // Stopped with more to do (or with no time to find out).
        run.left = true;
        run
    }

    /// A search sync step that says there's more while marks are queued after it
    /// worked. A mark marked again while this run held it waits for the next run, which
    /// natively is the next timer but here may be the safety net's tick (W7); marks
    /// another run holds are that run's to finish, so the next step finds none and stops.
    async fn search_step(state: &AppState<WorkersStorage>) -> Step {
        let ran = jobs::search_sync(state).await?;
        if ran.more || ran.idle {
            return Ok(ran);
        }
        let queued = jobs::search_queued(state).await?;
        Ok(Ran {
            more: queued > 0,
            ..ran
        })
    }

    fn millis(at: Timestamp) -> u64 {
        u64::try_from(at.unix_micros() / 1000).unwrap_or(0)
    }

    /// The marks, or `None` when KV can't be read (the tick then runs the jobs).
    async fn read_marks(kv: &KvStore) -> Option<Marks> {
        let (written, checked, due) = futures_util::future::join3(
            kv.get(marks::WRITTEN).text(),
            kv.get(marks::CHECKED).text(),
            kv.get(marks::DUE).text(),
        )
        .await;
        match (written, checked, due) {
            (Ok(written), Ok(checked), Ok(due)) => {
                let marks = Marks::parse(written.as_deref(), checked.as_deref(), due.as_deref());
                LAST_DUE.set(marks.due);
                Some(marks)
            }
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                tracing::warn!(error = %e, "jobs: cannot read the marks; running the jobs");
                None
            }
        }
    }

    /// Writes what a run says to (`marks::after`); `known` is the `due` mark KV holds,
    /// as far as this isolate knows, so the same one isn't written again.
    async fn mark(
        kv: &KvStore,
        state: &AppState<WorkersStorage>,
        after: After,
        known: Option<u64>,
    ) {
        if after.written {
            put(kv, marks::WRITTEN, millis(state.clock.now())).await;
        }
        if let Some(at) = after.checked {
            put(kv, marks::CHECKED, at).await;
        }
        if after.due {
            let due = match jobs::next_due(state).await {
                Ok(due) => marks::earliest(due.map(millis), after.retry_at),
                Err(e) => {
                    tracing::warn!(error = %e, "jobs: cannot read when work is next due");
                    after.retry_at
                }
            };
            if let Some(due) = due.filter(|due| Some(*due) != known) {
                put(kv, marks::DUE, due).await;
                LAST_DUE.set(Some(due));
            }
        }
    }

    /// Writes one mark; a failure is logged (the safety net's tick catches up).
    async fn put(kv: &KvStore, key: &str, at: u64) {
        let put = match kv.put(key, at.to_string()) {
            Ok(put) => put.execute().await,
            Err(e) => Err(e),
        };
        if let Err(e) = put {
            tracing::warn!(key, error = %e, "jobs: cannot write a mark");
        }
    }
}
