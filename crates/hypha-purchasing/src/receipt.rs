//! A receipt: what arrived from a supplier against a purchase order, at one location, at
//! one time. The order counts what its receipts brought; the receipt keeps how each line
//! came in and at what cost, so taking it back undoes exactly that.

use std::collections::HashSet;

use hypha_core::{
    ExchangeRate, LocationId, PendingEvent, PurchaseOrderId, ReceiptId, Recorded, Stamped, Stamps,
    Subject, Timestamp, UnitCost, VariantId, Version, Versioned,
};

use crate::{
    MAX_LINES, MAX_QUANTITY, PurchasingError, ReceiptBasis, ReceivedQuantities,
    events::{RECEIPT, ReceiptCancelled, ReceiptCreated, ReceiptLineEvent},
    purchase_order::long_text,
};

/// One variant's units in a receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiptLine {
    pub variant: VariantId,
    /// Into stock.
    pub accepted: u32,
    /// Refused (damaged, wrong): no stock.
    pub rejected: u32,
    /// What each accepted unit cost, in the store's currency: its order line's cost and
    /// share of the extra costs, at the receipt's rate. `None` when it couldn't be known.
    pub landed_unit_cost: Option<UnitCost>,
}

impl ReceiptLine {
    pub fn quantities(&self) -> ReceivedQuantities {
        ReceivedQuantities {
            variant: self.variant,
            accepted: self.accepted,
            rejected: self.rejected,
        }
    }
}

/// Whether a receipt still counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptState {
    Recorded,
    /// Taken back: its units came off stock and off the order's counts.
    Cancelled {
        at: Timestamp,
    },
}

impl ReceiptState {
    pub fn as_str(self) -> &'static str {
        match self {
            ReceiptState::Recorded => "recorded",
            ReceiptState::Cancelled { .. } => "cancelled",
        }
    }
}

/// What a new receipt holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewReceipt {
    pub id: ReceiptId,
    pub purchase_order: PurchaseOrderId,
    pub location: LocationId,
    pub lines: Vec<ReceiptLine>,
    /// The order's rate when it was received, if it has one.
    pub exchange_rate: Option<ExchangeRate>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    id: ReceiptId,
    version: Version,
    stamps: Option<Stamps>,
    purchase_order: PurchaseOrderId,
    location: LocationId,
    lines: Vec<ReceiptLine>,
    exchange_rate: Option<ExchangeRate>,
    note: Option<String>,
    state: ReceiptState,
    changes: Changes,
}

/// What happened since it was loaded. Compares as always equal.
#[derive(Debug, Clone, Copy, Default)]
struct Changes {
    created: bool,
    cancelled: bool,
}

impl PartialEq for Changes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changes {}

impl Recorded for Receipt {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let changes = std::mem::take(&mut self.changes);
        let subject = Subject::new(RECEIPT, self.id.as_uuid());
        let lines: Vec<ReceiptLineEvent> = self
            .lines
            .iter()
            .map(|l| ReceiptLineEvent {
                variant: l.variant,
                accepted: l.accepted,
                rejected: l.rejected,
            })
            .collect();
        let mut events = Vec::new();
        if changes.created {
            let event = ReceiptCreated {
                receipt: self.id,
                purchase_order: self.purchase_order,
                location: self.location,
                lines: lines.clone(),
            };
            events.push(PendingEvent::new(subject, &event));
        }
        if changes.cancelled {
            let event = ReceiptCancelled {
                receipt: self.id,
                purchase_order: self.purchase_order,
                location: self.location,
                lines,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        events
    }
}

impl Versioned for Receipt {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Receipt {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

/// One line per variant, at least one and at most `MAX_LINES`, each with a unit.
fn check_lines(lines: &[ReceiptLine]) -> Result<(), PurchasingError> {
    if lines.is_empty() {
        return Err(PurchasingError::EmptyReceipt);
    }
    if lines.len() > MAX_LINES {
        return Err(PurchasingError::TooManyLines { max: MAX_LINES });
    }
    let mut seen = HashSet::new();
    for line in lines {
        if !seen.insert(line.variant) {
            return Err(PurchasingError::DuplicateLine(line.variant));
        }
        if line.accepted == 0 && line.rejected == 0 {
            return Err(PurchasingError::EmptyReceiptLine(line.variant));
        }
        if u64::from(line.accepted) + u64::from(line.rejected) > u64::from(MAX_QUANTITY) {
            return Err(PurchasingError::QuantityTooLarge {
                variant: line.variant,
                max: MAX_QUANTITY,
            });
        }
    }
    Ok(())
}

impl Receipt {
    /// A receipt as recorded now (`receipt.created`).
    pub fn new(new: NewReceipt) -> Result<Self, PurchasingError> {
        check_lines(&new.lines)?;
        Ok(Self {
            id: new.id,
            version: Version::NEW,
            stamps: None,
            purchase_order: new.purchase_order,
            location: new.location,
            lines: new.lines,
            exchange_rate: new.exchange_rate,
            note: long_text(new.note)?,
            state: ReceiptState::Recorded,
            changes: Changes {
                created: true,
                cancelled: false,
            },
        })
    }

    /// Takes it back (`receipt.cancelled`); refused once it was (`receipt_cancelled`).
    pub fn cancel(&mut self, now: Timestamp) -> Result<(), PurchasingError> {
        if let ReceiptState::Cancelled { .. } = self.state {
            return Err(PurchasingError::ReceiptCancelled);
        }
        self.state = ReceiptState::Cancelled { at: now };
        self.changes.cancelled = true;
        Ok(())
    }

    pub fn id(&self) -> ReceiptId {
        self.id
    }

    pub fn purchase_order(&self) -> PurchaseOrderId {
        self.purchase_order
    }

    pub fn location(&self) -> LocationId {
        self.location
    }

    pub fn lines(&self) -> &[ReceiptLine] {
        &self.lines
    }

    pub fn exchange_rate(&self) -> Option<ExchangeRate> {
        self.exchange_rate
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn state(&self) -> ReceiptState {
        self.state
    }

    /// When it was received: when it was stored.
    pub fn received_at(&self) -> Option<Timestamp> {
        self.stamps.map(|s| s.created_at)
    }

    /// Its counts, to record on or take back off its order.
    pub fn quantities(&self) -> Vec<ReceivedQuantities> {
        self.lines.iter().map(ReceiptLine::quantities).collect()
    }

    /// What revaluing its order counts of it: its rate and what it took in.
    pub fn basis(&self) -> ReceiptBasis {
        ReceiptBasis {
            exchange_rate: self.exchange_rate,
            lines: self.quantities(),
        }
    }
}

/// Plain-data form of a `Receipt` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptSnapshot {
    pub id: ReceiptId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub purchase_order: PurchaseOrderId,
    pub location: LocationId,
    pub lines: Vec<ReceiptLine>,
    pub exchange_rate: Option<ExchangeRate>,
    pub note: Option<String>,
    pub state: ReceiptState,
}

impl Receipt {
    pub fn snapshot(&self) -> ReceiptSnapshot {
        ReceiptSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            purchase_order: self.purchase_order,
            location: self.location,
            lines: self.lines.clone(),
            exchange_rate: self.exchange_rate,
            note: self.note.clone(),
            state: self.state,
        }
    }

    /// Rebuilds a receipt from storage, checking its lines and note again.
    pub fn restore(s: ReceiptSnapshot) -> Result<Self, PurchasingError> {
        check_lines(&s.lines)?;
        Ok(Self {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            purchase_order: s.purchase_order,
            location: s.location,
            lines: s.lines,
            exchange_rate: s.exchange_rate,
            note: long_text(s.note)?,
            state: s.state,
            changes: Changes::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::Currency;
    use uuid::Uuid;

    use super::*;

    fn variant(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn line(n: u128, accepted: u32, rejected: u32) -> ReceiptLine {
        ReceiptLine {
            variant: variant(n),
            accepted,
            rejected,
            landed_unit_cost: Some(UnitCost::parse("500", Currency::EUR).unwrap()),
        }
    }

    fn new(lines: Vec<ReceiptLine>) -> NewReceipt {
        NewReceipt {
            id: ReceiptId::from_uuid(Uuid::from_u128(1)),
            purchase_order: PurchaseOrderId::from_uuid(Uuid::from_u128(2)),
            location: LocationId::from_uuid(Uuid::from_u128(3)),
            lines,
            exchange_rate: None,
            note: Some("  pallet 2 of 3 ".to_string()),
        }
    }

    #[test]
    fn a_receipt_is_recorded_with_its_lines() {
        let mut r = Receipt::new(new(vec![line(10, 4, 1)])).unwrap();
        assert_eq!(r.state(), ReceiptState::Recorded);
        assert_eq!(r.note(), Some("pallet 2 of 3"));
        assert_eq!(
            r.quantities(),
            [ReceivedQuantities {
                variant: variant(10),
                accepted: 4,
                rejected: 1
            }]
        );
        let events = r.take_events();
        assert_eq!(events[0].event_type, "receipt.created");
        assert_eq!(events[0].data["lines"][0]["rejected"], 1);
    }

    #[test]
    fn a_receipt_needs_a_unit_on_each_of_its_lines() {
        assert_eq!(
            Receipt::new(new(vec![])),
            Err(PurchasingError::EmptyReceipt)
        );
        assert_eq!(
            Receipt::new(new(vec![line(10, 0, 0)])),
            Err(PurchasingError::EmptyReceiptLine(variant(10)))
        );
        assert_eq!(
            Receipt::new(new(vec![line(10, 1, 0), line(10, 0, 1)])),
            Err(PurchasingError::DuplicateLine(variant(10)))
        );
    }

    #[test]
    fn rejected_units_alone_make_a_line() {
        assert!(Receipt::new(new(vec![line(10, 0, 2)])).is_ok());
    }

    #[test]
    fn a_receipt_is_taken_back_once() {
        let mut r = Receipt::new(new(vec![line(10, 1, 0)])).unwrap();
        r.take_events();
        let at = Timestamp::from_unix_micros(5);
        r.cancel(at).unwrap();
        assert_eq!(r.state(), ReceiptState::Cancelled { at });
        assert_eq!(r.take_events()[0].event_type, "receipt.cancelled");
        assert_eq!(r.cancel(at), Err(PurchasingError::ReceiptCancelled));
    }

    #[test]
    fn a_receipt_survives_storage() {
        let mut r = Receipt::new(new(vec![line(10, 2, 1)])).unwrap();
        r.cancel(Timestamp::from_unix_micros(9)).unwrap();
        let mut restored = Receipt::restore(r.snapshot()).unwrap();
        assert_eq!(restored, r);
        assert_eq!(restored.take_events(), []);
    }
}
