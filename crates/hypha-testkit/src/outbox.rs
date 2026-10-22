use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use hypha_core::{
    ActingAs, Actor, Clock, Delivery, DeliveryOutcome, DeliveryRecord, DeliveryStatus, EventRoute,
    OutboxRepo, Outcome, Page, PageRequest, PendingEvent, RepoError, StoredEvent, Timestamp,
    Version,
};
use uuid::Uuid;

use crate::FixedClock;

/// The outbox in memory. A handle: clones share the same events, so `InMemoryPorts` gives
/// one to every repo that writes events and to its batches, and tests read them all here. A
/// repo made on its own has a private one.
///
/// Event IDs are `Uuid::from_u128(seq)` and `occurred_at` is the shared clock's time
/// (below): deterministic, like the database's transaction time.
///
/// It also carries the clock the repos stamp what they write with, `FixedClock::DEFAULT`
/// unless a test moves it (`clock()`): the in-memory store's shared state, like `PgStore`'s
/// actor and clock.
#[derive(Clone, Default)]
pub struct InMemoryOutbox {
    inner: Arc<Mutex<State>>,
    clock: Arc<FixedClock>,
}

struct State {
    actor: Actor,
    /// The last `seq` handed out (events can be pruned, so not `events.len()`).
    last_seq: i64,
    events: Vec<StoredEvent>,
    /// Events already fanned out, by `seq`.
    fanned_out: HashSet<i64>,
    deliveries: Vec<DeliveryRow>,
}

struct DeliveryRow {
    id: Uuid,
    seq: i64,
    destination: String,
    status: DeliveryStatus,
    attempts: u32,
    next_attempt_at: Timestamp,
    leased_until: Option<Timestamp>,
    last_error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            actor: Actor::System,
            last_seq: 0,
            events: Vec::new(),
            fanned_out: HashSet::new(),
            deliveries: Vec::new(),
        }
    }
}

impl InMemoryOutbox {
    /// Writes `events` as the current actor; `version` is the one the write gave the
    /// aggregate they're about.
    pub(crate) fn write(&self, events: Vec<PendingEvent>, version: Option<Version>) {
        let mut state = self.inner.lock().unwrap();
        let actor = state.actor;
        for event in events {
            state.last_seq += 1;
            let seq = state.last_seq;
            state.events.push(StoredEvent {
                seq,
                id: Uuid::from_u128(seq as u128),
                event_type: event.event_type.to_string(),
                version: event.version,
                subject_kind: event.subject.kind.to_string(),
                subject_id: event.subject.id,
                subject_version: version,
                actor,
                occurred_at: self.clock.now(),
                data: event.data,
            });
        }
    }

    /// The time writes are stamped with now.
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// The clock writes are stamped with, to move in tests.
    pub fn clock(&self) -> &FixedClock {
        &self.clock
    }

    /// Every event written so far, oldest first.
    pub fn events(&self) -> Vec<StoredEvent> {
        self.inner.lock().unwrap().events.clone()
    }

    /// Each delivery's destination, status (`pending`, `succeeded`, `failed`), attempts
    /// and last error, in the order they were made: for tests.
    pub fn deliveries(&self) -> Vec<(String, &'static str, u32, Option<String>)> {
        let state = self.inner.lock().unwrap();
        state
            .deliveries
            .iter()
            .map(|d| {
                (
                    d.destination.clone(),
                    d.status.as_str(),
                    d.attempts,
                    d.last_error.clone(),
                )
            })
            .collect()
    }

    /// Deletes every delivery to `destination` (an app that's deleted).
    pub(crate) fn drop_deliveries(&self, destination: &str) {
        let mut state = self.inner.lock().unwrap();
        state.deliveries.retain(|d| d.destination != destination);
    }

    /// Just their types, for assertions that read like a story.
    pub fn types(&self) -> Vec<String> {
        self.events().into_iter().map(|e| e.event_type).collect()
    }
}

impl State {
    /// `row` as the admin lists it; `None` if its event is gone.
    fn record(&self, row: &DeliveryRow) -> Option<DeliveryRecord> {
        let event = self.events.iter().find(|e| e.seq == row.seq)?;
        Some(DeliveryRecord {
            id: row.id,
            event_seq: row.seq,
            event_id: event.id,
            event_type: event.event_type.clone(),
            occurred_at: event.occurred_at,
            destination: row.destination.clone(),
            status: row.status,
            attempts: row.attempts,
            next_attempt_at: row.next_attempt_at,
            last_error: row.last_error.clone(),
        })
    }
}

impl ActingAs for InMemoryOutbox {
    fn act_as(&self, actor: Actor) {
        self.inner.lock().unwrap().actor = actor;
    }
}

impl OutboxRepo for InMemoryOutbox {
    async fn events_after(&self, after: i64, limit: u32) -> Result<Vec<StoredEvent>, RepoError> {
        Ok(self
            .events()
            .into_iter()
            .filter(|e| e.seq > after)
            .take(limit as usize)
            .collect())
    }

    async fn fan_out(
        &self,
        limit: u32,
        now: Timestamp,
        destinations: &(dyn Fn(&EventRoute<'_>) -> Vec<String> + Sync),
    ) -> Result<u32, RepoError> {
        let mut state = self.inner.lock().unwrap();
        let claimed: Vec<(i64, String, String, Uuid)> = state
            .events
            .iter()
            .filter(|e| !state.fanned_out.contains(&e.seq))
            .take(limit as usize)
            .map(|e| {
                (
                    e.seq,
                    e.event_type.clone(),
                    e.subject_kind.clone(),
                    e.subject_id,
                )
            })
            .collect();
        for (seq, event_type, subject_kind, subject_id) in &claimed {
            let route = EventRoute {
                event_type,
                subject_kind,
                subject_id: *subject_id,
            };
            for destination in destinations(&route) {
                let exists = state
                    .deliveries
                    .iter()
                    .any(|d| d.seq == *seq && d.destination == destination);
                if !exists {
                    let id = Uuid::from_u128(1_000_000 + state.deliveries.len() as u128);
                    state.deliveries.push(DeliveryRow {
                        id,
                        seq: *seq,
                        destination,
                        status: DeliveryStatus::Pending,
                        attempts: 0,
                        next_attempt_at: now,
                        leased_until: None,
                        last_error: None,
                    });
                }
            }
            state.fanned_out.insert(*seq);
        }
        Ok(claimed.len() as u32)
    }

    async fn lease_due(
        &self,
        now: Timestamp,
        lease_until: Timestamp,
        limit: u32,
    ) -> Result<Vec<Delivery>, RepoError> {
        let mut state = self.inner.lock().unwrap();
        let mut due: Vec<usize> = state
            .deliveries
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.status == DeliveryStatus::Pending
                    && d.next_attempt_at <= now
                    && d.leased_until.is_none_or(|until| until <= now)
            })
            .map(|(i, _)| i)
            .collect();
        due.sort_by_key(|i| state.deliveries[*i].next_attempt_at);
        due.truncate(limit as usize);
        let mut leased = Vec::with_capacity(due.len());
        for i in due {
            state.deliveries[i].leased_until = Some(lease_until);
            let row = &state.deliveries[i];
            let Some(event) = state.events.iter().find(|e| e.seq == row.seq) else {
                continue;
            };
            leased.push(Delivery {
                id: row.id,
                event: event.clone(),
                destination: row.destination.clone(),
                attempts: row.attempts,
                leased_until: lease_until,
            });
        }
        Ok(leased)
    }

    async fn record_outcomes(&self, outcomes: &[Outcome]) -> Result<(), RepoError> {
        let mut state = self.inner.lock().unwrap();
        for Outcome {
            delivery,
            lease,
            outcome,
        } in outcomes
        {
            let Some(row) = state
                .deliveries
                .iter_mut()
                .find(|d| d.id == *delivery && d.leased_until == Some(*lease))
            else {
                continue;
            };
            row.attempts += 1;
            row.leased_until = None;
            match outcome {
                DeliveryOutcome::Succeeded => {
                    row.status = DeliveryStatus::Succeeded;
                    row.last_error = None;
                }
                DeliveryOutcome::Retry { error, at } => {
                    row.last_error = Some(error.clone());
                    row.next_attempt_at = *at;
                }
                DeliveryOutcome::Failed { error } => {
                    row.status = DeliveryStatus::Failed;
                    row.last_error = Some(error.clone());
                }
            }
        }
        Ok(())
    }

    async fn prune_events(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError> {
        let mut state = self.inner.lock().unwrap();
        let pruned: Vec<i64> = state
            .events
            .iter()
            .filter(|e| e.occurred_at < before && state.fanned_out.contains(&e.seq))
            .take(limit as usize)
            .map(|e| e.seq)
            .collect();
        state.events.retain(|e| !pruned.contains(&e.seq));
        state.deliveries.retain(|d| !pruned.contains(&d.seq));
        Ok(pruned.len() as u32)
    }

    async fn has_unfanned(&self) -> Result<bool, RepoError> {
        let state = self.inner.lock().unwrap();
        Ok(state
            .events
            .iter()
            .any(|e| !state.fanned_out.contains(&e.seq)))
    }

    async fn next_due(&self) -> Result<Option<Timestamp>, RepoError> {
        let state = self.inner.lock().unwrap();
        let events = state
            .events
            .iter()
            .filter(|e| !state.fanned_out.contains(&e.seq))
            .map(|e| e.occurred_at);
        let deliveries = state
            .deliveries
            .iter()
            .filter(|d| d.status == DeliveryStatus::Pending)
            .map(|d| {
                d.leased_until
                    .map_or(d.next_attempt_at, |l| l.max(d.next_attempt_at))
            });
        Ok(events.chain(deliveries).min())
    }

    async fn get_event(&self, id: Uuid) -> Result<Option<StoredEvent>, RepoError> {
        Ok(self.events().into_iter().find(|e| e.id == id))
    }

    async fn deliveries_to(
        &self,
        destination: &str,
        status: Option<DeliveryStatus>,
        page: PageRequest<i64>,
    ) -> Result<Page<DeliveryRecord>, RepoError> {
        let state = self.inner.lock().unwrap();
        let mut rows: Vec<DeliveryRecord> = state
            .deliveries
            .iter()
            .filter(|d| d.destination == destination)
            .filter(|d| status.is_none_or(|s| d.status == s))
            .filter(|d| page.after.is_none_or(|after| d.seq < after))
            .filter_map(|d| state.record(d))
            .collect();
        rows.sort_by_key(|d| std::cmp::Reverse(d.event_seq));
        rows.truncate(page.limit.get() as usize + 1);
        Ok(Page::from_overfetch(rows, page.limit))
    }

    async fn retry_delivery(
        &self,
        destination: &str,
        id: Uuid,
        now: Timestamp,
    ) -> Result<Option<DeliveryRecord>, RepoError> {
        let mut state = self.inner.lock().unwrap();
        let Some(i) = state
            .deliveries
            .iter()
            .position(|d| d.id == id && d.destination == destination)
        else {
            return Ok(None);
        };
        let row = &mut state.deliveries[i];
        let running = row.leased_until.is_some_and(|until| until > now);
        if !running {
            row.status = DeliveryStatus::Pending;
            row.next_attempt_at = now;
            row.leased_until = None;
        }
        Ok(state.record(&state.deliveries[i]))
    }
}
