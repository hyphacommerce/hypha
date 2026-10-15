use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use hypha_core::{
    ActingAs, Actor, Clock, Delivery, DeliveryOutcome, OutboxRepo, PendingEvent, RepoError,
    StoredEvent, Timestamp, Version,
};
use uuid::Uuid;

use crate::FixedClock;

/// The outbox in memory (design note 0016). A handle: clones share the same events, so
/// `InMemoryPorts` gives one to every repo that writes events and to its batches, and
/// tests read them all here. A repo made on its own has a private one.
///
/// Event IDs are `Uuid::from_u128(seq)` and `occurred_at` is the shared clock's time
/// (below): deterministic, like the database's transaction time.
///
/// It also carries the clock the repos stamp what they write with (design note 0016,
/// Q3), `FixedClock::DEFAULT` unless a test moves it (`clock()`): the in-memory store's
/// shared state, like `PgStore`'s actor and clock.
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
    status: Status,
    attempts: u32,
    next_attempt_at: Timestamp,
    leased_until: Option<Timestamp>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    Succeeded,
    Failed,
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
                let status = match d.status {
                    Status::Pending => "pending",
                    Status::Succeeded => "succeeded",
                    Status::Failed => "failed",
                };
                (
                    d.destination.clone(),
                    status,
                    d.attempts,
                    d.last_error.clone(),
                )
            })
            .collect()
    }

    /// Just their types, for assertions that read like a story.
    pub fn types(&self) -> Vec<String> {
        self.events().into_iter().map(|e| e.event_type).collect()
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
        destinations: &(dyn Fn(&str) -> Vec<String> + Sync),
    ) -> Result<u32, RepoError> {
        let mut state = self.inner.lock().unwrap();
        let claimed: Vec<(i64, String)> = state
            .events
            .iter()
            .filter(|e| !state.fanned_out.contains(&e.seq))
            .take(limit as usize)
            .map(|e| (e.seq, e.event_type.clone()))
            .collect();
        for (seq, event_type) in &claimed {
            for destination in destinations(event_type) {
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
                        status: Status::Pending,
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
                d.status == Status::Pending
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
            });
        }
        Ok(leased)
    }

    async fn record_outcomes(&self, outcomes: &[(Uuid, DeliveryOutcome)]) -> Result<(), RepoError> {
        let mut state = self.inner.lock().unwrap();
        for (id, outcome) in outcomes {
            let Some(row) = state.deliveries.iter_mut().find(|d| d.id == *id) else {
                continue;
            };
            row.attempts += 1;
            row.leased_until = None;
            match outcome {
                DeliveryOutcome::Succeeded => {
                    row.status = Status::Succeeded;
                    row.last_error = None;
                }
                DeliveryOutcome::Retry { error, at } => {
                    row.last_error = Some(error.clone());
                    row.next_attempt_at = *at;
                }
                DeliveryOutcome::Failed { error } => {
                    row.status = Status::Failed;
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
}
