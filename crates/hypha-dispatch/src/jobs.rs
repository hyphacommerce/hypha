//! The dispatcher's jobs (design note 0016, sections 5 and 7). Each is a plain function
//! of ports and a clock that does a bounded amount of work and says whether there's
//! more (`Progress`), so a runner loops while there is and sleeps when there isn't.

use hypha_auth::{HasSessions, SessionRepo};
use hypha_core::{
    Clock, Delivery, DeliveryOutcome, HasBatch, HasIdempotency, HasOutbox, IdempotencyRepo,
    OutboxRepo, RepoError, StageEvents,
};
use hypha_inventory::HasInventoryItems;
use hypha_markets::{HasPriceLists, StageFixedPrices};
use uuid::Uuid;

use crate::{handlers::run_handler, schedule::next_attempt};

/// Everything the dispatcher and its handlers read and write. Anything with all of them
/// is `DispatchPorts` (the blanket impl below).
pub trait DispatchPorts:
    HasOutbox + HasPriceLists + HasInventoryItems + HasBatch<Batch: StageFixedPrices + StageEvents>
{
}

impl<T> DispatchPorts for T where
    T: HasOutbox
        + HasPriceLists
        + HasInventoryItems
        + HasBatch<Batch: StageFixedPrices + StageEvents>
        + ?Sized
{
}

/// How much a job did, and whether it stopped at its limit with more to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u32,
    pub more: bool,
}

impl Progress {
    fn of(done: u32, limit: u32) -> Self {
        Self {
            done,
            more: done >= limit,
        }
    }
}

/// Events fanned out, or deliveries run, per call.
pub const BATCH: u32 = 100;
/// How long a delivery is leased for: past this, another runner may take it.
const LEASE_SECS: i64 = 60;
/// How long events (and their deliveries) are kept (Q7).
const RETENTION_SECS: i64 = 30 * 24 * 3600;
/// How long an idempotency key is honoured (design note 0007, section 5).
const IDEMPOTENCY_SECS: i64 = 24 * 3600;
/// How long a session is kept after it ended (expired or revoked).
const ENDED_SESSION_SECS: i64 = 30 * 24 * 3600;

/// Fans out events not fanned out yet: a pending delivery for each destination that
/// wants them (`handlers::destinations`).
pub async fn fan_out<S: HasOutbox>(ports: &S, clock: &impl Clock) -> Result<Progress, RepoError> {
    let done = ports
        .outbox()
        .fan_out(BATCH, clock.now(), &crate::destinations)
        .await?;
    Ok(Progress::of(done, BATCH))
}

/// Leases the due deliveries, for `run`. Short: get ports for it, then let them go.
pub async fn lease_due<S: HasOutbox>(
    ports: &S,
    clock: &impl Clock,
) -> Result<Vec<Delivery>, RepoError> {
    let now = clock.now();
    ports
        .outbox()
        .lease_due(now, now.plus_secs(LEASE_SECS), BATCH)
        .await
}

/// Runs one delivery and says how it went: a failure is tried again on the schedule
/// (`next_attempt`), or fails for good after the last attempt.
pub async fn run<S: DispatchPorts>(
    ports: &S,
    clock: &impl Clock,
    delivery: &Delivery,
) -> DeliveryOutcome {
    let result = match delivery.destination.split_once(':') {
        Some(("handler", name)) => run_handler(ports, name, &delivery.event).await,
        // Webhooks come with apps (step 5 of 0016).
        _ => Err(format!("no way to deliver to {:?}", delivery.destination)),
    };
    match result {
        Ok(()) => DeliveryOutcome::Succeeded,
        Err(error) => match next_attempt(delivery.attempts + 1, clock.now()) {
            Some(at) => DeliveryOutcome::Retry { error, at },
            None => DeliveryOutcome::Failed { error },
        },
    }
}

/// Records how the deliveries `run` went. Short, like `lease_due`.
pub async fn record<S: HasOutbox>(
    ports: &S,
    outcomes: &[(Uuid, DeliveryOutcome)],
) -> Result<(), RepoError> {
    ports.outbox().record_outcomes(outcomes).await
}

/// `lease_due`, `run` for each and `record`, all with `ports`: for tests and hosts that
/// get a connection per invocation anyway. A host with a pool gets ports for each step
/// instead, so none is held while a handler or webhook runs (EV9).
pub async fn deliver_due<S: DispatchPorts>(
    ports: &S,
    clock: &impl Clock,
) -> Result<Progress, RepoError> {
    let leased = lease_due(ports, clock).await?;
    let mut outcomes = Vec::with_capacity(leased.len());
    for delivery in &leased {
        outcomes.push((delivery.id, run(ports, clock, delivery).await));
    }
    record(ports, &outcomes).await?;
    Ok(Progress::of(leased.len() as u32, BATCH))
}

/// Deletes events (and their deliveries) older than 30 days (Q7).
pub async fn prune_events<S: HasOutbox>(
    ports: &S,
    clock: &impl Clock,
) -> Result<Progress, RepoError> {
    let before = clock.now().plus_secs(-RETENTION_SECS);
    let done = ports.outbox().prune_events(before, BATCH).await?;
    Ok(Progress::of(done, BATCH))
}

/// Deletes idempotency records older than a day: past that, a retry with the same key
/// is a new request (design note 0007, section 5).
pub async fn prune_idempotency_keys<S: HasIdempotency>(
    ports: &S,
    clock: &impl Clock,
) -> Result<Progress, RepoError> {
    let before = clock.now().plus_secs(-IDEMPOTENCY_SECS);
    let done = ports.idempotency().prune(before, BATCH).await?;
    Ok(Progress::of(done, BATCH))
}

/// Deletes sessions that ended (expired or revoked) over 30 days ago.
pub async fn prune_sessions<S: HasSessions>(
    ports: &S,
    clock: &impl Clock,
) -> Result<Progress, RepoError> {
    let before = clock.now().plus_secs(-ENDED_SESSION_SECS);
    let done = ports.sessions().prune(before, BATCH).await?;
    Ok(Progress::of(done, BATCH))
}
