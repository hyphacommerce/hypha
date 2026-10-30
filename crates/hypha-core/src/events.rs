//! Domain events: what happened, recorded by the aggregate that changed and written to the
//! outbox by the adapter in the same transaction as the change, so an event exists if and
//! only if its change committed.
//!
//! An aggregate keeps the events it recorded (usually in a `Recorder`) and hands them
//! over through `Recorded`. Repository writes (`insert`, `update`, and staging in a
//! batch) write them along with the aggregate; set-based writes that have no aggregate
//! stage theirs through `StageEvents`. Event IDs, the actor and the time come
//! from the adapter, so aggregates need no ID generator or clock to record.

use serde::Serialize;
use uuid::Uuid;

use crate::{Actor, Page, PageRequest, RepoError, Timestamp, Version};

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

    /// Records `event` unless one of its type about `subject` is pending already: for
    /// "something changed" events, so a write that changes several things is one event.
    pub fn record_once<E: Event>(&mut self, subject: Subject, event: &E) {
        if !self
            .0
            .iter()
            .any(|e| e.event_type == E::TYPE && e.subject == subject)
        {
            self.record(subject, event);
        }
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

/// Ports that stamp the events they write with whoever is acting. A host builds a request's
/// ports before it knows the caller, so it sets the actor once authentication has made the
/// `Ctx`; until then, and for jobs, it's `Actor::System`.
pub trait ActingAs {
    fn act_as(&self, actor: Actor);
}

impl<T: ActingAs + ?Sized> ActingAs for &T {
    fn act_as(&self, actor: Actor) {
        (**self).act_as(actor);
    }
}

/// What a host is told when a write gives the dispatcher work: events committed to the
/// outbox, or a delivery made due again (`OutboxRepo::retry_delivery`). Natively it wakes
/// the dispatcher; on Workers it runs one after the response.
pub type DispatchHook = std::sync::Arc<dyn Fn() + Send + Sync>;

/// Ports that tell the host when a write has given the dispatcher work (`DispatchHook`),
/// once the write has committed, so the dispatcher finds it. Writes that record no
/// events (a cart's, a session's) don't, so the host runs no dispatcher after them.
/// Like `ActingAs`, set by the host when it builds the ports.
pub trait SignalsDispatch {
    fn on_dispatch_work(&self, hook: DispatchHook);
}

impl<T: SignalsDispatch + ?Sized> SignalsDispatch for &T {
    fn on_dispatch_work(&self, hook: DispatchHook) {
        (**self).on_dispatch_work(hook);
    }
}

/// What fan-out routes an event by: its type, and what it's about. Most destinations only
/// look at the type; an app's own lifecycle events (`app.uninstalled`) go only to the app
/// they're about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRoute<'a> {
    pub event_type: &'a str,
    pub subject_kind: &'a str,
    pub subject_id: Uuid,
}

/// An event as the outbox holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvent {
    /// The outbox's order of insertion (not of commit: rows from a transaction that commits
    /// late can appear below rows already read).
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

/// One event on its way to one destination.
#[derive(Debug, Clone, PartialEq)]
pub struct Delivery {
    pub id: Uuid,
    pub event: StoredEvent,
    /// `handler:<name>` (in process) or `app:<key>` (a webhook).
    pub destination: String,
    /// Attempts made before this one.
    pub attempts: u32,
    /// Until when this runner holds it: its outcome is recorded only while the lease is
    /// still this one, and a runner doesn't start it closer to the end than the longest
    /// a destination may take.
    pub leased_until: Timestamp,
}

/// How a leased delivery went, for `record_outcomes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub delivery: Uuid,
    /// The lease it ran under (`Delivery::leased_until`).
    pub lease: Timestamp,
    pub outcome: DeliveryOutcome,
}

impl Outcome {
    pub fn of(delivery: &Delivery, outcome: DeliveryOutcome) -> Self {
        Self {
            delivery: delivery.id,
            lease: delivery.leased_until,
            outcome,
        }
    }
}

/// How a delivery attempt ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Succeeded,
    /// Failed; try again at `at`.
    Retry {
        error: String,
        at: Timestamp,
    },
    /// Failed for the last time: no more attempts.
    Failed {
        error: String,
    },
}

/// Where a delivery stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStatus {
    /// Waiting for its next attempt (the first, or a retry).
    Pending,
    Succeeded,
    /// Failed for good; the admin can retry it.
    Failed,
}

impl DeliveryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryStatus::Pending => "pending",
            DeliveryStatus::Succeeded => "succeeded",
            DeliveryStatus::Failed => "failed",
        }
    }

    /// The status named `s` (`as_str`'s inverse), for adapters and query strings.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(DeliveryStatus::Pending),
            "succeeded" => Some(DeliveryStatus::Succeeded),
            "failed" => Some(DeliveryStatus::Failed),
            _ => None,
        }
    }
}

/// A delivery as the admin lists it: which event, where it stands, what went wrong last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryRecord {
    pub id: Uuid,
    /// The event's `seq`: a destination has one delivery per event, so its deliveries
    /// are listed by this, newest first, and pages continue below it.
    pub event_seq: i64,
    pub event_id: Uuid,
    pub event_type: String,
    pub occurred_at: Timestamp,
    pub destination: String,
    pub status: DeliveryStatus,
    /// Attempts made so far.
    pub attempts: u32,
    /// When it runs next, while pending; when it last ran or was due, otherwise.
    pub next_attempt_at: Timestamp,
    pub last_error: Option<String>,
}

/// The outbox: tests and the admin read it; the dispatcher fans events out to deliveries,
/// leases the due ones and records how they went. Each method is one short transaction, so
/// nothing holds a connection while a destination runs.
#[allow(async_fn_in_trait)]
pub trait OutboxRepo {
    /// Events with `seq` greater than `after`, oldest first, at most `limit`. For tests
    /// and debugging only: not a feed, since rows can appear out of `seq` order.
    async fn events_after(&self, after: i64, limit: u32) -> Result<Vec<StoredEvent>, RepoError>;

    /// Claims up to `limit` events not fanned out yet (oldest first, skipping ones
    /// another claim holds), adds a pending delivery due at `now` for each destination
    /// `destinations` names for the event (`EventRoute`: its type and subject), and marks
    /// them fanned out. Returns how many events it fanned out. Running it again for an
    /// event adds nothing.
    async fn fan_out(
        &self,
        limit: u32,
        now: Timestamp,
        destinations: &(dyn Fn(&EventRoute<'_>) -> Vec<String> + Sync),
    ) -> Result<u32, RepoError>;

    /// Whether an event is waiting to be fanned out (`fan_out` would find one to claim,
    /// unless another claim holds it): one cheap read, so a dispatcher with nothing new
    /// reads nothing else (the apps it would fan out to).
    async fn has_unfanned(&self) -> Result<bool, RepoError>;

    /// Up to `limit` pending deliveries due by `now` whose lease is free (none, or
    /// expired), oldest due first, leased until `lease_until`: another call won't hand
    /// them out before then. A runner that dies leaves the lease to expire, and the
    /// delivery runs again (at least once, EV7).
    async fn lease_due(
        &self,
        now: Timestamp,
        lease_until: Timestamp,
        limit: u32,
    ) -> Result<Vec<Delivery>, RepoError>;

    /// Records outcomes: each delivery's attempts go up by one and its lease is freed;
    /// `Succeeded` and `Failed` are final, `Retry` is due again at its time. A delivery
    /// whose lease isn't `lease` any more (leased again, or retried from the admin) is
    /// skipped, as is an unknown one (pruned meanwhile).
    async fn record_outcomes(&self, outcomes: &[Outcome]) -> Result<(), RepoError>;

    /// When the dispatcher next has work: now-or-earlier for an event not fanned out yet
    /// (when it occurred), else the earliest a pending delivery can be leased (its next
    /// attempt, or its lease's end if that's later). `None` when nothing is pending. A host
    /// that runs the jobs only when there's work reads it.
    async fn next_due(&self) -> Result<Option<Timestamp>, RepoError>;

    /// Deletes up to `limit` events (and their deliveries) that occurred before
    /// `before` and were fanned out. Returns how many.
    async fn prune_events(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError>;

    /// The event with ID `id`, if it is still kept.
    async fn get_event(&self, id: Uuid) -> Result<Option<StoredEvent>, RepoError>;

    /// `destination`'s deliveries, by their event's `seq` newest first, only those in
    /// `status` when given. `page.after` is the last `event_seq` of the previous page.
    async fn deliveries_to(
        &self,
        destination: &str,
        status: Option<DeliveryStatus>,
        page: PageRequest<i64>,
    ) -> Result<Page<DeliveryRecord>, RepoError>;

    /// Makes `destination`'s delivery `id` due at `now`: a failed or succeeded one goes
    /// back to pending, a pending one stops waiting for its backoff. Its attempts are kept,
    /// so a delivery that had used them all gets one more. One whose lease hasn't ended
    /// (running right now) is left alone; one whose lease has ended is free (runners don't
    /// start a delivery they can't finish within its lease, and an outcome under an old
    /// lease isn't recorded). `None` when there's no such delivery to `destination`;
    /// otherwise the delivery as it is after.
    async fn retry_delivery(
        &self,
        destination: &str,
        id: Uuid,
        now: Timestamp,
    ) -> Result<Option<DeliveryRecord>, RepoError>;
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
    fn recording_once_keeps_the_first_of_a_type_per_subject() {
        let mut recorder = Recorder::default();
        let renamed = |title: &str| Renamed {
            title: title.to_string(),
        };
        recorder.record_once(subject(), &renamed("a"));
        recorder.record_once(subject(), &renamed("b"));
        let other = Subject::new("thing", Uuid::from_u128(8));
        recorder.record_once(other, &renamed("c"));
        let titles: Vec<&serde_json::Value> = recorder
            .pending()
            .iter()
            .map(|e| &e.data["title"])
            .collect();
        assert_eq!(titles, ["a", "c"]);
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
