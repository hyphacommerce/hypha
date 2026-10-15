use std::sync::{Arc, Mutex};

use hypha_core::{
    ActingAs, Actor, Clock, OutboxRepo, PendingEvent, RepoError, StoredEvent, Timestamp, Version,
};
use uuid::Uuid;

use crate::FixedClock;

/// The outbox in memory (design note 0016). A handle: clones share the same events, so
/// `InMemoryPorts` gives one to every repo that writes events and to its batches, and
/// tests read them all here. A repo made on its own has a private one.
///
/// Event IDs are `Uuid::from_u128(seq)` and `occurred_at` is `seq` microseconds after
/// the epoch: deterministic, and in order, like the database's.
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
    events: Vec<StoredEvent>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            actor: Actor::System,
            events: Vec::new(),
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
            let seq = state.events.len() as i64 + 1;
            state.events.push(StoredEvent {
                seq,
                id: Uuid::from_u128(seq as u128),
                event_type: event.event_type.to_string(),
                version: event.version,
                subject_kind: event.subject.kind.to_string(),
                subject_id: event.subject.id,
                subject_version: version,
                actor,
                occurred_at: Timestamp::from_unix_micros(seq),
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
}
