//! A transfer: stock moved from one of the store's locations to another, with a time in
//! transit between leaving and arriving.

use std::{collections::HashSet, fmt};

use hypha_core::{
    LocationId, PendingEvent, Recorded, Stamped, Stamps, Subject, Timestamp, TransferId, VariantId,
    Version, Versioned, optional_text,
};

use crate::{
    InventoryError, RestoreError,
    events::{
        TRANSFER, TransferCancelled, TransferCreated, TransferLineReceived, TransferReceived,
        TransferShipped, TransferUpdated,
    },
};

/// The most lines a transfer holds.
pub const MAX_TRANSFER_LINES: usize = 1000;
/// The most units a line moves: storage keeps the counts as 32-bit integers.
pub const MAX_TRANSFER_QUANTITY: u32 = 1_000_000_000;
/// The longest internal note.
pub const MAX_TRANSFER_NOTE_CHARS: usize = 5000;

/// A transfer's human number (`TR-1001`): unique and increasing, possibly with gaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransferNumber(i64);

impl TransferNumber {
    pub fn new(n: i64) -> Result<Self, InventoryError> {
        if n < 1 {
            return Err(InventoryError::InvalidTransferNumber(n));
        }
        Ok(Self(n))
    }

    pub fn value(self) -> i64 {
        self.0
    }
}

impl fmt::Display for TransferNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TR-{}", self.0)
    }
}

/// Where a transfer is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransferStatus {
    /// Being written: anything may change, and it may be deleted.
    #[default]
    Draft,
    /// Left the origin, not all arrived yet.
    InTransit,
    /// Every unit shipped arrived or was given up as lost.
    Received,
    /// Called off before it shipped.
    Cancelled,
}

impl TransferStatus {
    pub const ALL: [TransferStatus; 4] = [
        TransferStatus::Draft,
        TransferStatus::InTransit,
        TransferStatus::Received,
        TransferStatus::Cancelled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TransferStatus::Draft => "draft",
            TransferStatus::InTransit => "in_transit",
            TransferStatus::Received => "received",
            TransferStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == name)
    }
}

impl fmt::Display for TransferStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A line as given: one variant and how many to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferLineDraft {
    pub variant: VariantId,
    pub quantity: u32,
}

/// One variant on a transfer, and how much of it left and arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferLine {
    variant: VariantId,
    quantity: u32,
    shipped: u32,
    received: u32,
    rejected: u32,
}

impl TransferLine {
    fn new(draft: TransferLineDraft) -> Result<Self, InventoryError> {
        if draft.quantity == 0 {
            return Err(InventoryError::InvalidTransferQuantity(draft.variant));
        }
        if draft.quantity > MAX_TRANSFER_QUANTITY {
            return Err(InventoryError::TransferQuantityTooLarge {
                variant: draft.variant,
                max: MAX_TRANSFER_QUANTITY,
            });
        }
        Ok(Self {
            variant: draft.variant,
            quantity: draft.quantity,
            shipped: 0,
            received: 0,
            rejected: 0,
        })
    }

    pub fn variant(&self) -> VariantId {
        self.variant
    }

    pub fn quantity(&self) -> u32 {
        self.quantity
    }

    pub fn shipped(&self) -> u32 {
        self.shipped
    }

    pub fn received(&self) -> u32 {
        self.received
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// Shipped and not yet received or given up as lost: what's on its way.
    pub fn in_transit(&self) -> u32 {
        self.shipped
            .saturating_sub(self.received)
            .saturating_sub(self.rejected)
    }
}

/// A transfer's editable fields as given: checked as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferDraft {
    pub origin: LocationId,
    pub destination: LocationId,
    /// When the goods are due at the destination.
    pub expected_at: Option<Timestamp>,
    /// A carrier or tracking note.
    pub reference: Option<String>,
    /// For staff only.
    pub note: Option<String>,
    pub lines: Vec<TransferLineDraft>,
}

impl TransferDraft {
    /// Whether it would make a transfer: `Transfer::new`'s checks, before taking a number
    /// for it.
    pub fn check(&self) -> Result<(), InventoryError> {
        check(self.clone()).map(|_| ())
    }
}

/// The checked editable fields.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fields {
    origin: LocationId,
    destination: LocationId,
    expected_at: Option<Timestamp>,
    reference: Option<String>,
    note: Option<String>,
}

/// Checks `draft`; its lines come back new, with no counts.
fn check(draft: TransferDraft) -> Result<(Fields, Vec<TransferLine>), InventoryError> {
    if draft.origin == draft.destination {
        return Err(InventoryError::SameLocation);
    }
    if draft.lines.len() > MAX_TRANSFER_LINES {
        return Err(InventoryError::TooManyTransferLines {
            max: MAX_TRANSFER_LINES,
        });
    }
    let mut seen = HashSet::new();
    let mut lines = Vec::with_capacity(draft.lines.len());
    for line in draft.lines {
        if !seen.insert(line.variant) {
            return Err(InventoryError::DuplicateTransferLine(line.variant));
        }
        lines.push(TransferLine::new(line)?);
    }
    let note = match draft.note.as_deref().map(str::trim) {
        Some(note) if note.chars().count() > MAX_TRANSFER_NOTE_CHARS => {
            return Err(InventoryError::TransferNoteTooLong {
                max: MAX_TRANSFER_NOTE_CHARS,
            });
        }
        Some(note) if !note.is_empty() => Some(note.to_string()),
        _ => None,
    };
    let fields = Fields {
        origin: draft.origin,
        destination: draft.destination,
        expected_at: draft.expected_at,
        reference: optional_text("reference", draft.reference)?,
        note,
    };
    Ok((fields, lines))
}

/// Units of one variant arriving at the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferArrival {
    pub variant: VariantId,
    /// Into stock at the destination.
    pub accepted: u32,
    /// Lost or broken on the way: no stock anywhere.
    pub rejected: u32,
}

/// A line still on its way, for an over-receipt's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InTransit {
    pub variant: VariantId,
    pub in_transit: u32,
}

/// What a list shows of a transfer, without its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferSummary {
    pub id: TransferId,
    pub number: TransferNumber,
    pub origin: LocationId,
    pub destination: LocationId,
    pub status: TransferStatus,
    pub line_count: usize,
    /// Units over every line.
    pub units: u64,
    pub expected_at: Option<Timestamp>,
    pub shipped_at: Option<Timestamp>,
    pub received_at: Option<Timestamp>,
    pub created_at: Option<Timestamp>,
}

/// Stock moving between two of the store's locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    id: TransferId,
    version: Version,
    stamps: Option<Stamps>,
    number: TransferNumber,
    fields: Fields,
    lines: Vec<TransferLine>,
    status: TransferStatus,
    shipped_at: Option<Timestamp>,
    received_at: Option<Timestamp>,
    cancelled_at: Option<Timestamp>,
    changes: Changes,
}

/// What happened since it was loaded, as events. Compares as always equal.
#[derive(Debug, Clone, Default)]
struct Changes {
    created: bool,
    fields: Vec<&'static str>,
    shipped: bool,
    received: Vec<TransferLineReceived>,
    cancelled: bool,
}

impl PartialEq for Changes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changes {}

impl Recorded for Transfer {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let changes = std::mem::take(&mut self.changes);
        let subject = Subject::new(TRANSFER, self.id.as_uuid());
        let (transfer, origin, destination) =
            (self.id, self.fields.origin, self.fields.destination);
        let mut events = Vec::new();
        if changes.created {
            let event = TransferCreated {
                transfer,
                origin,
                destination,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        if !changes.fields.is_empty() && !changes.created {
            let event = TransferUpdated {
                transfer,
                origin,
                destination,
                fields: changes.fields,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        if changes.shipped {
            let event = TransferShipped {
                transfer,
                origin,
                destination,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        if !changes.received.is_empty() {
            let event = TransferReceived {
                transfer,
                origin,
                destination,
                lines: changes.received,
                complete: self.status == TransferStatus::Received,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        if changes.cancelled {
            let event = TransferCancelled {
                transfer,
                origin,
                destination,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        events
    }
}

impl Versioned for Transfer {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Transfer {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Transfer {
    /// A draft numbered `number`.
    pub fn new(
        id: TransferId,
        number: TransferNumber,
        draft: TransferDraft,
    ) -> Result<Self, InventoryError> {
        let (fields, lines) = check(draft)?;
        Ok(Self {
            id,
            version: Version::NEW,
            stamps: None,
            number,
            fields,
            lines,
            status: TransferStatus::Draft,
            shipped_at: None,
            received_at: None,
            cancelled_at: None,
            changes: Changes {
                created: true,
                ..Changes::default()
            },
        })
    }

    /// The editable fields as they are, to change some and `update` with.
    pub fn draft(&self) -> TransferDraft {
        TransferDraft {
            origin: self.fields.origin,
            destination: self.fields.destination,
            expected_at: self.fields.expected_at,
            reference: self.fields.reference.clone(),
            note: self.fields.note.clone(),
            lines: self
                .lines
                .iter()
                .map(|l| TransferLineDraft {
                    variant: l.variant,
                    quantity: l.quantity,
                })
                .collect(),
        }
    }

    /// Replaces the editable fields with `draft`, checked as a whole. Only a draft
    /// changes (`transfer_not_draft`): what's shipped is on its way.
    pub fn update(&mut self, draft: TransferDraft) -> Result<(), InventoryError> {
        self.require(TransferStatus::Draft)?;
        let (fields, lines) = check(draft)?;
        let old = &self.fields;
        let changed = [
            ("origin", old.origin != fields.origin),
            ("destination", old.destination != fields.destination),
            ("expected_at", old.expected_at != fields.expected_at),
            ("reference", old.reference != fields.reference),
            ("note", old.note != fields.note),
            ("lines", self.lines != lines),
        ];
        for (field, differs) in changed {
            if differs && !self.changes.fields.contains(&field) {
                self.changes.fields.push(field);
            }
        }
        self.fields = fields;
        self.lines = lines;
        Ok(())
    }

    /// Sends every line whole: each line's units leave the origin now. Needs a draft
    /// (`transfer_not_draft`) with at least one line (`transfer_empty`). The caller
    /// takes the units off the origin's stock.
    pub fn ship(&mut self, now: Timestamp) -> Result<(), InventoryError> {
        self.require(TransferStatus::Draft)?;
        if self.lines.is_empty() {
            return Err(InventoryError::TransferEmpty);
        }
        for line in &mut self.lines {
            line.shipped = line.quantity;
        }
        self.status = TransferStatus::InTransit;
        self.shipped_at = Some(now);
        self.changes.shipped = true;
        Ok(())
    }

    /// Records units arriving at the destination, accepted into stock or rejected (lost
    /// on the way). Needs a transfer in transit (`transfer_not_in_transit`); each variant
    /// must be one of its lines (`unknown_transfer_line`), listed once, with at least one
    /// unit (`empty_receipt_line`), and no more than it has on its way (`over_received`,
    /// with every such line's units in transit): the units are the store's own, so
    /// there's no receiving more. Once every line is accounted for, it's `received`.
    /// All or nothing. The caller puts the accepted units into the destination's stock.
    pub fn receive(
        &mut self,
        arrivals: &[TransferArrival],
        now: Timestamp,
    ) -> Result<(), InventoryError> {
        self.require(TransferStatus::InTransit)?;
        if arrivals.is_empty() {
            return Err(InventoryError::TransferArrivalEmpty);
        }
        let mut seen = HashSet::new();
        let mut over = Vec::new();
        for arrival in arrivals {
            if !seen.insert(arrival.variant) {
                return Err(InventoryError::DuplicateTransferLine(arrival.variant));
            }
            let line = self
                .line(arrival.variant)
                .ok_or(InventoryError::UnknownTransferLine(arrival.variant))?;
            let units = u64::from(arrival.accepted) + u64::from(arrival.rejected);
            if units == 0 {
                return Err(InventoryError::EmptyArrival(arrival.variant));
            }
            if units > u64::from(line.in_transit()) {
                over.push(InTransit {
                    variant: line.variant,
                    in_transit: line.in_transit(),
                });
            }
        }
        if !over.is_empty() {
            return Err(InventoryError::OverReceived(over));
        }
        for arrival in arrivals {
            if let Some(line) = self.lines.iter_mut().find(|l| l.variant == arrival.variant) {
                // Within what was shipped, so both fit.
                line.received += arrival.accepted;
                line.rejected += arrival.rejected;
            }
            self.changes.received.push(TransferLineReceived {
                variant: arrival.variant,
                accepted: arrival.accepted,
                rejected: arrival.rejected,
            });
        }
        if self.lines.iter().all(|l| l.in_transit() == 0) {
            self.status = TransferStatus::Received;
            self.received_at = Some(now);
        }
        Ok(())
    }

    /// Calls off a draft (`transfer_not_draft`): a shipped transfer is on its way, and
    /// bringing it back is receiving it at the origin.
    pub fn cancel(&mut self, now: Timestamp) -> Result<(), InventoryError> {
        self.require(TransferStatus::Draft)?;
        self.status = TransferStatus::Cancelled;
        self.cancelled_at = Some(now);
        self.changes.cancelled = true;
        Ok(())
    }

    /// Fails unless it's a draft (`transfer_not_draft`): only a draft changes or is
    /// deleted.
    pub fn ensure_draft(&self) -> Result<(), InventoryError> {
        self.require(TransferStatus::Draft)
    }

    fn require(&self, status: TransferStatus) -> Result<(), InventoryError> {
        if self.status == status {
            return Ok(());
        }
        Err(match status {
            TransferStatus::InTransit => InventoryError::TransferNotInTransit {
                status: self.status,
            },
            _ => InventoryError::TransferNotDraft {
                status: self.status,
            },
        })
    }

    pub fn id(&self) -> TransferId {
        self.id
    }

    pub fn number(&self) -> TransferNumber {
        self.number
    }

    pub fn origin(&self) -> LocationId {
        self.fields.origin
    }

    pub fn destination(&self) -> LocationId {
        self.fields.destination
    }

    pub fn expected_at(&self) -> Option<Timestamp> {
        self.fields.expected_at
    }

    pub fn reference(&self) -> Option<&str> {
        self.fields.reference.as_deref()
    }

    pub fn note(&self) -> Option<&str> {
        self.fields.note.as_deref()
    }

    pub fn lines(&self) -> &[TransferLine] {
        &self.lines
    }

    pub fn line(&self, variant: VariantId) -> Option<&TransferLine> {
        self.lines.iter().find(|l| l.variant == variant)
    }

    pub fn status(&self) -> TransferStatus {
        self.status
    }

    pub fn shipped_at(&self) -> Option<Timestamp> {
        self.shipped_at
    }

    pub fn received_at(&self) -> Option<Timestamp> {
        self.received_at
    }

    pub fn cancelled_at(&self) -> Option<Timestamp> {
        self.cancelled_at
    }

    pub fn summary(&self) -> TransferSummary {
        TransferSummary {
            id: self.id,
            number: self.number,
            origin: self.fields.origin,
            destination: self.fields.destination,
            status: self.status,
            line_count: self.lines.len(),
            units: self.lines.iter().map(|l| u64::from(l.quantity)).sum(),
            expected_at: self.fields.expected_at,
            shipped_at: self.shipped_at,
            received_at: self.received_at,
            created_at: self.stamps.map(|s| s.created_at),
        }
    }
}

/// Which transfers a list wants: all that match every field set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransferFilter {
    pub status: Option<TransferStatus>,
    pub origin: Option<LocationId>,
    pub destination: Option<LocationId>,
    /// Transfers with a line for it.
    pub variant: Option<VariantId>,
}

impl TransferFilter {
    /// The rule every adapter follows.
    pub fn matches(&self, t: &Transfer) -> bool {
        self.status.is_none_or(|s| t.status == s)
            && self.origin.is_none_or(|o| t.fields.origin == o)
            && self.destination.is_none_or(|d| t.fields.destination == d)
            && self.variant.is_none_or(|v| t.line(v).is_some())
    }
}

/// What transfers in transit still bring of `variant` to `location`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncomingTransfer {
    pub variant: VariantId,
    pub location: LocationId,
    /// Units on their way: more than zero.
    pub quantity: u64,
    /// The soonest a transfer bringing some of them is expected; `None` when none says.
    pub next_expected_at: Option<Timestamp>,
}

/// The rule every adapter follows: over the transfers in transit, each line of one of
/// `variants` with units on their way counts them at the destination, and the soonest
/// `expected_at` among those transfers comes with it. Sorted by variant, then location.
pub fn incoming_transfers<'a>(
    transfers: impl IntoIterator<Item = &'a Transfer>,
    variants: &[VariantId],
) -> Vec<IncomingTransfer> {
    let mut found: std::collections::BTreeMap<(VariantId, LocationId), IncomingTransfer> =
        std::collections::BTreeMap::new();
    for t in transfers {
        if t.status != TransferStatus::InTransit {
            continue;
        }
        let location = t.fields.destination;
        for line in &t.lines {
            let units = line.in_transit();
            if units == 0 || !variants.contains(&line.variant) {
                continue;
            }
            let entry = found
                .entry((line.variant, location))
                .or_insert(IncomingTransfer {
                    variant: line.variant,
                    location,
                    quantity: 0,
                    next_expected_at: None,
                });
            entry.quantity += u64::from(units);
            entry.next_expected_at =
                Timestamp::earliest(entry.next_expected_at, t.fields.expected_at);
        }
    }
    found.into_values().collect()
}

/// A line as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferLineSnapshot {
    pub variant: VariantId,
    pub quantity: u32,
    pub shipped: u32,
    pub received: u32,
    pub rejected: u32,
}

/// Plain-data form of a `Transfer` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferSnapshot {
    pub id: TransferId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub number: TransferNumber,
    pub origin: LocationId,
    pub destination: LocationId,
    pub expected_at: Option<Timestamp>,
    pub reference: Option<String>,
    pub note: Option<String>,
    pub lines: Vec<TransferLineSnapshot>,
    pub status: TransferStatus,
    pub shipped_at: Option<Timestamp>,
    pub received_at: Option<Timestamp>,
    pub cancelled_at: Option<Timestamp>,
}

impl Transfer {
    pub fn snapshot(&self) -> TransferSnapshot {
        TransferSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            number: self.number,
            origin: self.fields.origin,
            destination: self.fields.destination,
            expected_at: self.fields.expected_at,
            reference: self.fields.reference.clone(),
            note: self.fields.note.clone(),
            lines: self
                .lines
                .iter()
                .map(|l| TransferLineSnapshot {
                    variant: l.variant,
                    quantity: l.quantity,
                    shipped: l.shipped,
                    received: l.received,
                    rejected: l.rejected,
                })
                .collect(),
            status: self.status,
            shipped_at: self.shipped_at,
            received_at: self.received_at,
            cancelled_at: self.cancelled_at,
        }
    }

    /// Rebuilds a transfer from storage, its rules checked again: counts within what was
    /// shipped, nothing shipped on a draft.
    pub fn restore(s: TransferSnapshot) -> Result<Transfer, RestoreError> {
        let draft = TransferDraft {
            origin: s.origin,
            destination: s.destination,
            expected_at: s.expected_at,
            reference: s.reference,
            note: s.note,
            lines: s
                .lines
                .iter()
                .map(|l| TransferLineDraft {
                    variant: l.variant,
                    quantity: l.quantity,
                })
                .collect(),
        };
        let (fields, mut lines) = check(draft)?;
        for (line, stored) in lines.iter_mut().zip(&s.lines) {
            let arrived = u64::from(stored.received) + u64::from(stored.rejected);
            let shipped_ok = match s.status {
                TransferStatus::Draft | TransferStatus::Cancelled => stored.shipped == 0,
                TransferStatus::InTransit | TransferStatus::Received => {
                    stored.shipped == stored.quantity
                }
            };
            if !shipped_ok || arrived > u64::from(stored.shipped) {
                return Err(RestoreError::TransferCounts(stored.variant));
            }
            line.shipped = stored.shipped;
            line.received = stored.received;
            line.rejected = stored.rejected;
        }
        Ok(Transfer {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            number: s.number,
            fields,
            lines,
            status: s.status,
            shipped_at: s.shipped_at,
            received_at: s.received_at,
            cancelled_at: s.cancelled_at,
            changes: Changes::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn loc(n: u128) -> LocationId {
        LocationId::from_uuid(Uuid::from_u128(n))
    }

    fn var(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(100 + n))
    }

    fn at(n: i64) -> Timestamp {
        Timestamp::from_unix_micros(n)
    }

    fn draft(lines: &[(u128, u32)]) -> TransferDraft {
        TransferDraft {
            origin: loc(1),
            destination: loc(2),
            expected_at: None,
            reference: None,
            note: None,
            lines: lines
                .iter()
                .map(|&(v, quantity)| TransferLineDraft {
                    variant: var(v),
                    quantity,
                })
                .collect(),
        }
    }

    fn transfer(lines: &[(u128, u32)]) -> Transfer {
        let mut t = Transfer::new(
            TransferId::from_uuid(Uuid::from_u128(9)),
            TransferNumber::new(1001).unwrap(),
            draft(lines),
        )
        .unwrap();
        t.take_events();
        t
    }

    fn shipped(lines: &[(u128, u32)]) -> Transfer {
        let mut t = transfer(lines);
        t.ship(at(1)).unwrap();
        t.take_events();
        t
    }

    fn arrival(v: u128, accepted: u32, rejected: u32) -> TransferArrival {
        TransferArrival {
            variant: var(v),
            accepted,
            rejected,
        }
    }

    fn event_types(t: &mut Transfer) -> Vec<String> {
        t.take_events()
            .into_iter()
            .map(|e| e.event_type.to_string())
            .collect()
    }

    #[test]
    fn numbers_start_at_one_and_show_with_their_prefix() {
        assert_eq!(TransferNumber::new(1001).unwrap().to_string(), "TR-1001");
        assert_eq!(
            TransferNumber::new(0),
            Err(InventoryError::InvalidTransferNumber(0))
        );
    }

    #[test]
    fn statuses_read_back_by_name() {
        for status in TransferStatus::ALL {
            assert_eq!(TransferStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(TransferStatus::parse("shipped"), None);
    }

    #[test]
    fn a_new_transfer_is_a_draft_and_says_so() {
        let mut t = Transfer::new(
            TransferId::from_uuid(Uuid::from_u128(9)),
            TransferNumber::new(1001).unwrap(),
            draft(&[(1, 5)]),
        )
        .unwrap();
        assert_eq!(t.status(), TransferStatus::Draft);
        assert_eq!(event_types(&mut t), ["transfer.created"]);
    }

    #[test]
    fn origin_and_destination_must_differ() {
        let mut d = draft(&[(1, 5)]);
        d.destination = d.origin;
        assert_eq!(
            Transfer::new(
                TransferId::from_uuid(Uuid::from_u128(9)),
                TransferNumber::new(1).unwrap(),
                d
            ),
            Err(InventoryError::SameLocation)
        );
    }

    #[test]
    fn a_line_needs_a_unit_and_each_variant_comes_once() {
        let new = |d| {
            Transfer::new(
                TransferId::from_uuid(Uuid::from_u128(9)),
                TransferNumber::new(1).unwrap(),
                d,
            )
        };
        assert_eq!(
            new(draft(&[(1, 0)])),
            Err(InventoryError::InvalidTransferQuantity(var(1)))
        );
        assert!(new(draft(&[(1, MAX_TRANSFER_QUANTITY)])).is_ok());
        assert_eq!(
            new(draft(&[(1, MAX_TRANSFER_QUANTITY + 1)])),
            Err(InventoryError::TransferQuantityTooLarge {
                variant: var(1),
                max: MAX_TRANSFER_QUANTITY
            })
        );
        assert_eq!(
            new(draft(&[(1, 2), (1, 3)])),
            Err(InventoryError::DuplicateTransferLine(var(1)))
        );
    }

    #[test]
    fn a_transfer_holds_at_most_1000_lines() {
        let lines: Vec<(u128, u32)> = (0..1001).map(|n| (n, 1)).collect();
        let new = |lines: &[(u128, u32)]| {
            Transfer::new(
                TransferId::from_uuid(Uuid::from_u128(9)),
                TransferNumber::new(1).unwrap(),
                draft(lines),
            )
        };
        assert!(new(&lines[..1000]).is_ok());
        assert_eq!(
            new(&lines),
            Err(InventoryError::TooManyTransferLines { max: 1000 })
        );
    }

    #[test]
    fn a_note_is_trimmed_and_capped() {
        let mut d = draft(&[(1, 1)]);
        d.note = Some("  fragile  ".into());
        let mut t = transfer(&[(1, 1)]);
        t.update(d.clone()).unwrap();
        assert_eq!(t.note(), Some("fragile"));
        d.note = Some("x".repeat(5001));
        assert_eq!(
            t.update(d),
            Err(InventoryError::TransferNoteTooLong { max: 5000 })
        );
    }

    #[test]
    fn a_draft_changes_and_names_what_changed() {
        let mut t = transfer(&[(1, 5)]);
        let mut d = t.draft();
        d.reference = Some("DHL 123".into());
        d.lines.push(TransferLineDraft {
            variant: var(2),
            quantity: 1,
        });
        t.update(d).unwrap();
        let events = t.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "transfer.updated");
        assert_eq!(
            events[0].data["fields"],
            serde_json::json!(["reference", "lines"])
        );
    }

    #[test]
    fn shipping_sends_every_line_whole() {
        let mut t = transfer(&[(1, 5), (2, 3)]);
        t.ship(at(7)).unwrap();
        assert_eq!(t.status(), TransferStatus::InTransit);
        assert_eq!(t.shipped_at(), Some(at(7)));
        assert_eq!(
            t.lines()
                .iter()
                .map(|l| (l.shipped(), l.in_transit()))
                .collect::<Vec<_>>(),
            [(5, 5), (3, 3)]
        );
        assert_eq!(event_types(&mut t), ["transfer.shipped"]);
    }

    #[test]
    fn an_empty_draft_does_not_ship() {
        let mut t = transfer(&[]);
        assert_eq!(t.ship(at(1)), Err(InventoryError::TransferEmpty));
    }

    #[test]
    fn a_shipped_transfer_no_longer_changes_ships_or_cancels() {
        let mut t = shipped(&[(1, 5)]);
        let in_transit = InventoryError::TransferNotDraft {
            status: TransferStatus::InTransit,
        };
        assert_eq!(t.update(t.draft()), Err(in_transit.clone()));
        assert_eq!(t.ship(at(2)), Err(in_transit.clone()));
        assert_eq!(t.cancel(at(2)), Err(in_transit.clone()));
        assert_eq!(t.ensure_draft(), Err(in_transit));
    }

    #[test]
    fn a_draft_is_not_received() {
        let mut t = transfer(&[(1, 5)]);
        assert_eq!(
            t.receive(&[arrival(1, 1, 0)], at(2)),
            Err(InventoryError::TransferNotInTransit {
                status: TransferStatus::Draft
            })
        );
    }

    #[test]
    fn receiving_in_parts_ends_when_every_unit_is_accounted_for() {
        let mut t = shipped(&[(1, 5), (2, 3)]);
        t.receive(&[arrival(1, 4, 0)], at(2)).unwrap();
        assert_eq!(t.status(), TransferStatus::InTransit);
        let events = t.take_events();
        assert_eq!(events[0].event_type, "transfer.received");
        assert_eq!(events[0].data["complete"], serde_json::json!(false));
        assert_eq!(
            events[0].data["lines"],
            serde_json::json!([{"variant": var(1), "accepted": 4, "rejected": 0}])
        );

        t.receive(&[arrival(1, 0, 1), arrival(2, 3, 0)], at(3))
            .unwrap();
        assert_eq!(t.status(), TransferStatus::Received);
        assert_eq!(t.received_at(), Some(at(3)));
        assert_eq!(
            t.lines()
                .iter()
                .map(|l| (l.received(), l.rejected(), l.in_transit()))
                .collect::<Vec<_>>(),
            [(4, 1, 0), (3, 0, 0)]
        );
        assert_eq!(t.take_events()[0].data["complete"], serde_json::json!(true));
    }

    #[test]
    fn receiving_more_than_is_on_its_way_names_every_such_line() {
        let mut t = shipped(&[(1, 5), (2, 3)]);
        t.receive(&[arrival(1, 4, 0)], at(2)).unwrap();
        let before = t.clone();
        assert_eq!(
            t.receive(&[arrival(1, 1, 1), arrival(2, 4, 0)], at(3)),
            Err(InventoryError::OverReceived(vec![
                InTransit {
                    variant: var(1),
                    in_transit: 1
                },
                InTransit {
                    variant: var(2),
                    in_transit: 3
                },
            ]))
        );
        assert_eq!(t.lines(), before.lines(), "nothing changed");
    }

    #[test]
    fn an_arrival_names_a_line_once_with_at_least_a_unit() {
        let mut t = shipped(&[(1, 5)]);
        assert_eq!(
            t.receive(&[arrival(7, 1, 0)], at(2)),
            Err(InventoryError::UnknownTransferLine(var(7)))
        );
        assert_eq!(
            t.receive(&[arrival(1, 0, 0)], at(2)),
            Err(InventoryError::EmptyArrival(var(1)))
        );
        assert_eq!(
            t.receive(&[arrival(1, 1, 0), arrival(1, 1, 0)], at(2)),
            Err(InventoryError::DuplicateTransferLine(var(1)))
        );
        assert_eq!(
            t.receive(&[], at(2)),
            Err(InventoryError::TransferArrivalEmpty)
        );
    }

    #[test]
    fn a_received_transfer_takes_nothing_more() {
        let mut t = shipped(&[(1, 2)]);
        t.receive(&[arrival(1, 2, 0)], at(2)).unwrap();
        assert_eq!(
            t.receive(&[arrival(1, 1, 0)], at(3)),
            Err(InventoryError::TransferNotInTransit {
                status: TransferStatus::Received
            })
        );
    }

    #[test]
    fn a_draft_is_cancelled() {
        let mut t = transfer(&[(1, 2)]);
        t.cancel(at(4)).unwrap();
        assert_eq!(t.status(), TransferStatus::Cancelled);
        assert_eq!(t.cancelled_at(), Some(at(4)));
        assert_eq!(event_types(&mut t), ["transfer.cancelled"]);
        assert_eq!(
            t.cancel(at(5)),
            Err(InventoryError::TransferNotDraft {
                status: TransferStatus::Cancelled
            })
        );
    }

    #[test]
    fn a_snapshot_restores_to_the_same_transfer() {
        let mut t = shipped(&[(1, 5), (2, 3)]);
        t.receive(&[arrival(1, 2, 1)], at(2)).unwrap();
        t.take_events();
        assert_eq!(Transfer::restore(t.snapshot()), Ok(t));
    }

    #[test]
    fn restore_refuses_counts_past_what_shipped() {
        let mut s = shipped(&[(1, 5)]).snapshot();
        s.lines[0].received = 6;
        assert_eq!(
            Transfer::restore(s),
            Err(RestoreError::TransferCounts(var(1)))
        );
        let mut s = transfer(&[(1, 5)]).snapshot();
        s.lines[0].shipped = 5;
        assert_eq!(
            Transfer::restore(s),
            Err(RestoreError::TransferCounts(var(1)))
        );
    }

    #[test]
    fn a_filter_matches_on_every_field_set() {
        let t = shipped(&[(1, 5)]);
        assert!(TransferFilter::default().matches(&t));
        let only = |f: TransferFilter| f.matches(&t);
        assert!(only(TransferFilter {
            status: Some(TransferStatus::InTransit),
            origin: Some(loc(1)),
            destination: Some(loc(2)),
            variant: Some(var(1)),
        }));
        assert!(!only(TransferFilter {
            status: Some(TransferStatus::Draft),
            ..TransferFilter::default()
        }));
        assert!(!only(TransferFilter {
            origin: Some(loc(2)),
            ..TransferFilter::default()
        }));
        assert!(!only(TransferFilter {
            destination: Some(loc(1)),
            ..TransferFilter::default()
        }));
        assert!(!only(TransferFilter {
            variant: Some(var(2)),
            ..TransferFilter::default()
        }));
    }

    #[test]
    fn incoming_counts_what_transfers_in_transit_still_bring() {
        // Expected later than `sooner`: none set.
        let mut later = shipped(&[(1, 5), (2, 2)]);
        later.receive(&[arrival(1, 1, 1)], at(2)).unwrap();
        let mut sooner = transfer(&[(1, 3)]);
        let mut d = sooner.draft();
        d.expected_at = Some(at(20));
        sooner.update(d).unwrap();
        sooner.ship(at(1)).unwrap();
        let draft_only = transfer(&[(1, 9)]);
        let found = incoming_transfers([&later, &sooner, &draft_only], &[var(1)]);
        assert_eq!(
            found,
            [IncomingTransfer {
                variant: var(1),
                location: loc(2),
                quantity: 3 + 3,
                next_expected_at: Some(at(20)),
            }]
        );
        assert!(incoming_transfers([&later], &[var(3)]).is_empty());
    }
}
