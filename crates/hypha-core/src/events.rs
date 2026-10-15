//! Domain events (design note 0016): what happened, recorded by the aggregate that
//! changed and written to the outbox by the adapter in the same transaction as the
//! change, so an event exists if and only if its change committed.
//!
//! An aggregate keeps the events it recorded (usually in a `Recorder`) and hands them
//! over through `Recorded`. Repository writes (`insert`, `update`, and staging in a
//! batch) write them along with the aggregate; set-based writes that have no aggregate
//! stage theirs through `StageEvents`. Event IDs, the actor and the time come
//! from the adapter, so aggregates need no ID generator or clock to record.

use serde::Serialize;
use uuid::Uuid;

use crate::{Actor, RepoError, Timestamp, Version};

/// A domain event's payload: a contract type in its domain crate's `events.rs`, plain
/// serializable data (IDs, strings, `Money`), never the aggregate itself. Its name and
/// version are the webhook contract: renaming or reshaping a field is a new `VERSION`.
pub trait Event: Serialize {
    /// Dotted and stable: `"product.variant_removed"`.
    const TYPE: &'static str;
    const VERSION: u16;
}

/// What an event is about: an aggregate (or other record) by kind and ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subject {
    /// The aggregate's kind, as event types name it: `"product"`, `"price_list"`.
    pub kind: &'static str,
    pub id: Uuid,
}

impl Subject {
    pub fn new(kind: &'static str, id: Uuid) -> Self {
        Self { kind, id }
    }
}

/// An event recorded but not yet written: the adapter adds its ID, actor and commit time.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingEvent {
    pub event_type: &'static str,
    pub version: u16,
    pub subject: Subject,
    pub data: serde_json::Value,
}

impl PendingEvent {
    pub fn new<E: Event>(subject: Subject, event: &E) -> Self {
        Self {
            event_type: E::TYPE,
            version: E::VERSION,
            subject,
            // Payloads are plain structs of IDs, strings and numbers, with string keys:
            // serializing one can't fail (a documented invariant of `Event`).
            data: serde_json::to_value(event)
                .unwrap_or_else(|e| unreachable!("event {} can't be serialized: {e}", E::TYPE)),
        }
    }
}

/// The events an aggregate recorded since it was loaded. It compares as always equal
/// and is skipped in snapshots, so an aggregate restored from storage equals the one
/// that was written (`restore(snapshot) == aggregate` round trips still hold).
#[derive(Debug, Clone, Default)]
pub struct Recorder(Vec<PendingEvent>);

impl Recorder {
    pub fn record<E: Event>(&mut self, subject: Subject, event: &E) {
        self.0.push(PendingEvent::new(subject, event));
    }

    pub fn pending(&self) -> &[PendingEvent] {
        &self.0
    }

    /// Hands the events over to be written, leaving none.
    pub fn take(&mut self) -> Vec<PendingEvent> {
        std::mem::take(&mut self.0)
    }
}

impl PartialEq for Recorder {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Recorder {}

/// An aggregate that records events. Writing it (`insert`, `update`, staging in a
/// batch) takes them and writes them in the same transaction. A write that fails drops
/// them, as callers drop the aggregate after any failed write and load it again.
///
/// Take-only rather than a borrowed slice, so an aggregate can coalesce what it records
/// (a product's edits in one request are one `product.updated` listing every field).
pub trait Recorded {
    fn take_events(&mut self) -> Vec<PendingEvent>;
}

/// Events a command records itself, for writes that have no aggregate to record them
/// (catalog membership, fixed prices): staged in the same batch as the write.
pub trait StageEvents {
    fn record_events(&mut self, events: Vec<PendingEvent>);
}

/// Ports that stamp the events they write with whoever is acting (design note 0016,
/// Q1). A host builds a request's ports before it knows the caller, so it sets the
/// actor once authentication has made the `Ctx`; until then, and for jobs, it's
/// `Actor::System`.
pub trait ActingAs {
    fn act_as(&self, actor: Actor);
}

impl<T: ActingAs + ?Sized> ActingAs for &T {
    fn act_as(&self, actor: Actor) {
        (**self).act_as(actor)
    }
}

/// An event as the outbox holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvent {
    /// The outbox's order of insertion (not of commit: rows from a transaction that
    /// commits late can appear below rows already read, design note 0016, EV6).
    pub seq: i64,
    pub id: Uuid,
    pub event_type: String,
    pub version: u16,
    pub subject_kind: String,
    pub subject_id: Uuid,
    /// The aggregate's version after the write that recorded the event; `None` for
    /// events staged through `StageEvents`.
    pub subject_version: Option<Version>,
    pub actor: Actor,
    pub occurred_at: Timestamp,
    pub data: serde_json::Value,
}

/// The outbox, as tests and the admin read it. Delivery (claiming, leases) is the
/// dispatcher's port, in a later step.
#[allow(async_fn_in_trait)]
pub trait OutboxRepo {
    /// Events with `seq` greater than `after`, oldest first, at most `limit`. For tests
    /// and debugging only: not a feed, since rows can appear out of `seq` order.
    async fn events_after(&self, after: i64, limit: u32) -> Result<Vec<StoredEvent>, RepoError>;
}

pub trait HasOutbox {
    type Outbox: OutboxRepo;
    fn outbox(&self) -> &Self::Outbox;
}

impl<T: HasOutbox + ?Sized> HasOutbox for &T {
    type Outbox = T::Outbox;
    fn outbox(&self) -> &Self::Outbox {
        (**self).outbox()
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;

    #[derive(Serialize)]
    struct Renamed {
        title: String,
    }

    impl Event for Renamed {
        const TYPE: &'static str = "thing.renamed";
        const VERSION: u16 = 1;
    }

    fn subject() -> Subject {
        Subject::new("thing", Uuid::from_u128(7))
    }

    #[test]
    fn a_recorded_event_carries_its_type_version_and_data() {
        let mut recorder = Recorder::default();
        recorder.record(
            subject(),
            &Renamed {
                title: "Mug".to_string(),
            },
        );
        assert_eq!(
            recorder.pending(),
            [PendingEvent {
                event_type: "thing.renamed",
                version: 1,
                subject: subject(),
                data: serde_json::json!({"title": "Mug"}),
            }]
        );
    }

    #[test]
    fn taking_the_events_leaves_none() {
        let mut recorder = Recorder::default();
        recorder.record(
            subject(),
            &Renamed {
                title: "Mug".to_string(),
            },
        );
        assert_eq!(recorder.take().len(), 1);
        assert!(recorder.pending().is_empty());
    }

    #[test]
    fn recorders_never_make_aggregates_unequal() {
        let mut busy = Recorder::default();
        busy.record(
            subject(),
            &Renamed {
                title: "Mug".to_string(),
            },
        );
        assert_eq!(busy, Recorder::default());
    }
}
