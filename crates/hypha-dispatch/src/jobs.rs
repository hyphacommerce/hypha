//! The dispatcher's jobs. Each is a plain function of ports and a clock that does a bounded
//! amount of work and says whether there's more (`Progress`), so a runner loops while there
//! is and sleeps when there isn't.

use hypha_apps::{App, AppKey, AppRepo, HasApps, Webhook};
use hypha_auth::{HasSessions, SessionRepo};
use hypha_core::{
    Clock, Delivery, DeliveryOutcome, EventHandlers, EventRoute, HasIdempotency, HasOutbox,
    IdempotencyRepo, OutboxRepo, Outcome, Progress, RepoError,
};

use core::time::Duration;

use crate::settings::DispatchSettings;

/// What the dispatcher itself reads and writes: the outbox and the apps. Handlers bring
/// their own needs (each `EventHandler<P>` bounds `P`), so this doesn't grow with them.
pub trait DispatchPorts: HasOutbox + HasApps {}

impl<T> DispatchPorts for T where T: HasOutbox + HasApps + ?Sized {}

/// A handler's destination name in deliveries.
const HANDLER: &str = "handler:";

/// Sends webhooks: the host's HTTP client, signing with the app's secret. A port, so the
/// dispatcher stays free of HTTP and runtimes.
#[allow(async_fn_in_trait)]
pub trait WebhookSender {
    /// Sends `delivery`'s event to `app`'s webhook (it has one). `Err` with why, which
    /// the admin shows the merchant: say what happened (a status, a timeout), not how
    /// the network looked.
    async fn send(&self, app: &App, delivery: &Delivery) -> Result<(), String>;
}

/// The longest one delivery may take: the slowest webhook timeout allowed, with room for
/// loading the app and connecting. A runner doesn't start a delivery with less of its
/// lease left (`has_time`), so a lease never runs out while its delivery is sending.
pub const LONGEST_RUN: Duration = Duration::from_secs(Webhook::MAX_TIMEOUT_MS as u64 / 1000 + 15);

/// Fans out events not fanned out yet: a pending delivery for each destination that
/// wants them, the `handlers` (`handler:<name>`) and every app that wants the event
/// (`app:<key>`, `App::wants_event`: subscribed to its type and granted what it's
/// about, or the app an `app.*` event is about) as the apps are now. `handlers` are `Sync` because
/// the outbox may hold their routing across an await (a host's futures must be `Send`).
/// With no event waiting (`has_unfanned`), that one read is all: the apps aren't read.
pub async fn fan_out<S: HasOutbox + HasApps>(
    ports: &S,
    clock: &impl Clock,
    settings: &DispatchSettings,
    handlers: &(impl EventHandlers<S> + Sync),
) -> Result<Progress, RepoError> {
    if !ports.outbox().has_unfanned().await? {
        return Ok(Progress::of(0, settings.batch));
    }
    let apps = ports.apps().list().await?;
    let destinations = |route: &EventRoute<'_>| {
        let mut to: Vec<String> = handlers
            .wanting(route.event_type)
            .into_iter()
            .map(|name| format!("{HANDLER}{name}"))
            .collect();
        to.extend(
            apps.iter()
                .filter(|app| app.wants_event(route))
                .map(|app| app.key().destination()),
        );
        to
    };
    let done = ports
        .outbox()
        .fan_out(settings.batch, clock.now(), &destinations)
        .await?;
    Ok(Progress::of(done, settings.batch))
}

/// Leases the due deliveries, for `run`. Short: get ports for it, then let them go.
pub async fn lease_due<S: HasOutbox>(
    ports: &S,
    clock: &impl Clock,
    settings: &DispatchSettings,
) -> Result<Vec<Delivery>, RepoError> {
    let now = clock.now();
    ports
        .outbox()
        .lease_due(now, now.plus(settings.lease), settings.batch)
        .await
}

/// Whether `delivery` can still run and be recorded within its lease. The rest of a
/// batch that's run out of time is left: its leases end, and the next lease takes them.
pub fn has_time(delivery: &Delivery, clock: &impl Clock) -> bool {
    clock.now().plus(LONGEST_RUN) <= delivery.leased_until
}

/// Why an attempt failed, and whether trying again could help.
enum Failure {
    /// Tried again on the schedule.
    Retry(String),
    /// Nothing to send to (the app is gone or has no webhook any more): fails for good
    /// at once. The admin can retry it once the app is back.
    Final(String),
}

/// What a delivery needs next, as `prepare` finds it.
pub enum Prepared {
    /// An in-process handler: `handle` it, with ports.
    Handler,
    /// A webhook to this app: `send` it, no ports needed (don't hold a connection over
    /// the HTTP call, EV9).
    Webhook(Box<App>),
    /// Decided already (the app is gone, say).
    Done(DeliveryOutcome),
}

/// Looks at `delivery`'s destination: a handler, an app to send to as the app is now,
/// or an outcome already.
pub async fn prepare<S: HasApps>(
    ports: &S,
    clock: &impl Clock,
    settings: &DispatchSettings,
    delivery: &Delivery,
) -> Prepared {
    match delivery.destination.split_once(':') {
        Some((kind, _)) if format!("{kind}:") == HANDLER => Prepared::Handler,
        Some(("app", key)) => match load_app(ports, key, delivery).await {
            Ok(app) => Prepared::Webhook(Box::new(app)),
            Err(failure) => Prepared::Done(outcome(Err(failure), clock, settings, delivery)),
        },
        _ => Prepared::Done(outcome(
            Err(Failure::Retry(format!(
                "no way to deliver to {:?}",
                delivery.destination
            ))),
            clock,
            settings,
            delivery,
        )),
    }
}

/// Runs a handler delivery (`Prepared::Handler`) with the one of `handlers` it names.
/// One the set doesn't have (removed from the build) is retried on the schedule, so
/// putting it back in time still delivers.
pub async fn handle<S: ?Sized>(
    ports: &S,
    clock: &impl Clock,
    settings: &DispatchSettings,
    handlers: &impl EventHandlers<S>,
    delivery: &Delivery,
) -> DeliveryOutcome {
    let name = delivery
        .destination
        .strip_prefix(HANDLER)
        .unwrap_or_default();
    let result = match handlers.run(name, ports, &delivery.event).await {
        Some(result) => result.map_err(Failure::Retry),
        None => Err(Failure::Retry(format!("no handler {name:?} in this build"))),
    };
    outcome(result, clock, settings, delivery)
}

/// Sends a webhook delivery (`Prepared::Webhook`).
pub async fn send(
    webhooks: &impl WebhookSender,
    clock: &impl Clock,
    settings: &DispatchSettings,
    app: &App,
    delivery: &Delivery,
) -> DeliveryOutcome {
    let result = webhooks.send(app, delivery).await.map_err(Failure::Retry);
    outcome(result, clock, settings, delivery)
}

/// Runs one delivery with one set of ports and says how it went: a failure is tried
/// again on the schedule (`RetrySchedule::next_attempt`), or fails for good after the
/// last attempt.
/// A host with a pool uses `prepare`, `handle` and `send` instead, to let its connection
/// go before a webhook is sent.
pub async fn run<S: DispatchPorts>(
    ports: &S,
    clock: &impl Clock,
    settings: &DispatchSettings,
    handlers: &impl EventHandlers<S>,
    webhooks: &impl WebhookSender,
    delivery: &Delivery,
) -> DeliveryOutcome {
    match prepare(ports, clock, settings, delivery).await {
        Prepared::Handler => handle(ports, clock, settings, handlers, delivery).await,
        Prepared::Webhook(app) => send(webhooks, clock, settings, &app, delivery).await,
        Prepared::Done(outcome) => outcome,
    }
}

fn outcome(
    result: Result<(), Failure>,
    clock: &impl Clock,
    settings: &DispatchSettings,
    delivery: &Delivery,
) -> DeliveryOutcome {
    match result {
        Ok(()) => DeliveryOutcome::Succeeded,
        Err(Failure::Final(error)) => DeliveryOutcome::Failed { error },
        Err(Failure::Retry(error)) => match settings
            .retry
            .next_attempt(delivery.attempts + 1, clock.now())
        {
            Some(at) => DeliveryOutcome::Retry { error, at },
            None => DeliveryOutcome::Failed { error },
        },
    }
}

/// App `key` as it is now, if it can take `delivery` now. A disabled app's deliveries wait
/// on the retry schedule, so pausing an app for a while keeps its backlog. One that may no
/// longer receive the event (uninstalled, its grant narrowed so it no longer reads what the
/// event is about, unsubscribed from the type: `App::wants_event`, as fan-out asked when
/// the delivery was made) fails for good at once, with why: a delivery queued or retrying
/// from before the change must not reach it. What the event's payload shows is decided when
/// it's sent, by the grant then (the host's `WebhookSender` redacts).
async fn load_app<S: HasApps>(ports: &S, key: &str, delivery: &Delivery) -> Result<App, Failure> {
    let gone = || Failure::Final(format!("there is no app {key}"));
    let key = AppKey::new(key).map_err(|_| gone())?;
    let app = ports
        .apps()
        .get(&key)
        .await
        .map_err(|e| Failure::Retry(e.to_string()))?
        .ok_or_else(gone)?;
    if !app.enabled() {
        return Err(Failure::Retry(format!("app {key} is disabled")));
    }
    if app.webhook().is_none() {
        return Err(Failure::Final(format!("app {key} has no webhook")));
    }
    let event = &delivery.event;
    let route = EventRoute {
        event_type: &event.event_type,
        subject_kind: &event.subject_kind,
        subject_id: event.subject_id,
    };
    if !app.wants_event(&route) {
        return Err(Failure::Final(no_longer_wanted(&app, &route)));
    }
    Ok(app)
}

/// Why `app` may no longer receive `route`'s event, for the admin's delivery list.
fn no_longer_wanted(app: &App, route: &EventRoute<'_>) -> String {
    let key = app.key();
    let event = route.event_type;
    if app.uninstalled_at().is_some() {
        format!("app {key} is uninstalled: it no longer receives {event}")
    } else if !app.wants(event) {
        format!("app {key} is no longer subscribed to {event}")
    } else {
        format!("app {key}'s grant no longer covers {event}")
    }
}

/// Records how the deliveries `run` went. Short, like `lease_due`.
pub async fn record<S: HasOutbox>(ports: &S, outcomes: &[Outcome]) -> Result<(), RepoError> {
    ports.outbox().record_outcomes(outcomes).await
}

/// `lease_due`, `run` for each and `record`, all with `ports`: for tests and hosts that get
/// a connection per invocation anyway. A host with a pool gets ports for each step instead,
/// so none is held while a handler or webhook runs.
pub async fn deliver_due<S: DispatchPorts>(
    ports: &S,
    clock: &impl Clock,
    settings: &DispatchSettings,
    handlers: &impl EventHandlers<S>,
    webhooks: &impl WebhookSender,
) -> Result<Progress, RepoError> {
    let leased = lease_due(ports, clock, settings).await?;
    let mut outcomes = Vec::with_capacity(leased.len());
    for delivery in leased.iter().take_while(|d| has_time(d, clock)) {
        let outcome = run(ports, clock, settings, handlers, webhooks, delivery).await;
        outcomes.push(Outcome::of(delivery, outcome));
    }
    record(ports, &outcomes).await?;
    Ok(Progress::of(
        u32::try_from(leased.len()).unwrap_or(u32::MAX),
        settings.batch,
    ))
}

/// Deletes events (and their deliveries) older than `keep_events`.
pub async fn prune_events<S: HasOutbox>(
    ports: &S,
    clock: &impl Clock,
    settings: &DispatchSettings,
) -> Result<Progress, RepoError> {
    let before = clock.now().minus(settings.keep_events);
    let done = ports.outbox().prune_events(before, settings.batch).await?;
    Ok(Progress::of(done, settings.batch))
}

/// Deletes idempotency records older than `keep_idempotency_keys`: past that, a retry with
/// the same key is a new request.
pub async fn prune_idempotency_keys<S: HasIdempotency>(
    ports: &S,
    clock: &impl Clock,
    settings: &DispatchSettings,
) -> Result<Progress, RepoError> {
    let before = clock.now().minus(settings.keep_idempotency_keys);
    let done = ports.idempotency().prune(before, settings.batch).await?;
    Ok(Progress::of(done, settings.batch))
}

/// Deletes sessions that ended (expired or revoked) over `keep_ended_sessions` ago.
pub async fn prune_sessions<S: HasSessions>(
    ports: &S,
    clock: &impl Clock,
    settings: &DispatchSettings,
) -> Result<Progress, RepoError> {
    let before = clock.now().minus(settings.keep_ended_sessions);
    let done = ports.sessions().prune(before, settings.batch).await?;
    Ok(Progress::of(done, settings.batch))
}
