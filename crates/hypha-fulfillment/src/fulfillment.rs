use std::collections::HashSet;

use hypha_core::{
    FulfillmentId, LocationId, OrderId, OrderLineId, PendingEvent, Quantity, Recorded, Recorder,
    Stamped, Stamps, Subject, Timestamp, Version, Versioned, optional_text, required_text,
};

use crate::{
    FulfillmentError,
    events::{
        FULFILLMENT, FulfillmentCancelled, FulfillmentCreated, FulfillmentUpdated, ShippedLine,
        TrackingData,
    },
};

/// One order line in a fulfilment and how many of it left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FulfillmentLine {
    pub line: OrderLineId,
    pub quantity: Quantity,
    /// Whether these units came off the location's stock (the item was tracked when it
    /// shipped). Taking the fulfilment back puts back only what was taken, whatever
    /// tracking says by then.
    pub stock_taken: bool,
}

/// A tracking number, with the carrier and a URL to follow it when known (design note
/// 0018, FU6). Entered by staff or an app; carriers' own updates come later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracking {
    carrier: Option<String>,
    number: String,
    url: Option<String>,
}

impl Tracking {
    pub const MAX_URL_LEN: usize = 2048;

    /// Text is trimmed like all free text; a blank carrier or URL is `None`.
    pub fn new(
        carrier: Option<&str>,
        number: &str,
        url: Option<&str>,
    ) -> Result<Self, FulfillmentError> {
        let url = url.map(str::trim).filter(|u| !u.is_empty());
        if let Some(url) = url {
            let scheme = url.starts_with("https://") || url.starts_with("http://");
            if !scheme || url.chars().count() > Self::MAX_URL_LEN {
                return Err(FulfillmentError::InvalidTrackingUrl(url.to_string()));
            }
        }
        Ok(Self {
            carrier: optional_text("carrier", carrier.map(str::to_string))?,
            number: required_text("tracking number", number)?,
            url: url.map(str::to_string),
        })
    }

    pub fn carrier(&self) -> Option<&str> {
        self.carrier.as_deref()
    }

    pub fn number(&self) -> &str {
        &self.number
    }

    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    fn data(&self) -> TrackingData {
        TrackingData {
            carrier: self.carrier.clone(),
            number: self.number.clone(),
            url: self.url.clone(),
        }
    }
}

/// Whether a fulfilment stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FulfillmentState {
    /// It left, and counts as fulfilled on the order.
    Shipped,
    /// Recorded by mistake and taken back at `at`: its units are back in stock.
    Cancelled { at: Timestamp },
}

/// What `hypha-order-actions` knows when it records a fulfilment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFulfillment {
    pub id: FulfillmentId,
    pub order: OrderId,
    pub location: LocationId,
    pub lines: Vec<FulfillmentLine>,
    pub tracking: Vec<Tracking>,
}

/// What left one location for one order (design note 0018, FU1): its lines, how many
/// of each, and its tracking.
///
/// Invariants, checked by `new` and `restore`: at least one line, each order line once,
/// at most `MAX_TRACKING` tracking numbers. What was ordered and what's left to ship are
/// the order's to check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fulfillment {
    id: FulfillmentId,
    version: Version,
    /// When it was stored and last changed, set by the adapter (design note 0016);
    /// `created_at` is when it was recorded.
    stamps: Option<Stamps>,
    order: OrderId,
    location: LocationId,
    lines: Vec<FulfillmentLine>,
    tracking: Vec<Tracking>,
    state: FulfillmentState,
    events: Recorder,
}

impl Recorded for Fulfillment {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for Fulfillment {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Fulfillment {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Fulfillment {
    pub const MAX_TRACKING: usize = 10;

    /// A shipped fulfilment, recording `fulfillment.created`.
    pub fn new(new: NewFulfillment) -> Result<Self, FulfillmentError> {
        let mut fulfillment = Self::restore(FulfillmentSnapshot {
            id: new.id,
            version: Version::NEW,
            stamps: None,
            order: new.order,
            location: new.location,
            lines: new.lines,
            tracking: new.tracking,
            state: FulfillmentState::Shipped,
        })?;
        let created = FulfillmentCreated {
            fulfillment: fulfillment.id,
            order: fulfillment.order,
            location: fulfillment.location,
            lines: fulfillment
                .lines
                .iter()
                .map(|l| ShippedLine {
                    line: l.line,
                    quantity: l.quantity.value(),
                })
                .collect(),
            tracking: fulfillment.tracking.iter().map(Tracking::data).collect(),
        };
        fulfillment.events.record(fulfillment.subject(), &created);
        Ok(fulfillment)
    }

    fn subject(&self) -> Subject {
        Subject::new(FULFILLMENT, self.id.as_uuid())
    }

    fn refuse_cancelled(&self) -> Result<(), FulfillmentError> {
        match self.state {
            FulfillmentState::Cancelled { .. } => Err(FulfillmentError::Cancelled(self.id)),
            FulfillmentState::Shipped => Ok(()),
        }
    }

    /// Replaces the tracking; returns whether it changed (`fulfillment.updated` then).
    /// A cancelled fulfilment keeps what it had.
    pub fn set_tracking(&mut self, tracking: Vec<Tracking>) -> Result<bool, FulfillmentError> {
        self.refuse_cancelled()?;
        check_tracking(&tracking)?;
        if tracking == self.tracking {
            return Ok(false);
        }
        self.tracking = tracking;
        let updated = FulfillmentUpdated {
            fulfillment: self.id,
            order: self.order,
            tracking: self.tracking.iter().map(Tracking::data).collect(),
        };
        self.events.record(self.subject(), &updated);
        Ok(true)
    }

    /// Takes it back (`fulfillment.cancelled`); returns whether that changed anything (a
    /// cancelled one stays cancelled at its first time). The caller puts the stock and
    /// the order's counts back in the same batch.
    pub fn cancel(&mut self, at: Timestamp) -> bool {
        if self.refuse_cancelled().is_err() {
            return false;
        }
        self.state = FulfillmentState::Cancelled { at };
        let cancelled = FulfillmentCancelled {
            fulfillment: self.id,
            order: self.order,
        };
        self.events.record(self.subject(), &cancelled);
        true
    }

    pub fn id(&self) -> FulfillmentId {
        self.id
    }

    pub fn order(&self) -> OrderId {
        self.order
    }

    pub fn location(&self) -> LocationId {
        self.location
    }

    pub fn lines(&self) -> &[FulfillmentLine] {
        &self.lines
    }

    pub fn tracking(&self) -> &[Tracking] {
        &self.tracking
    }

    pub fn state(&self) -> FulfillmentState {
        self.state
    }

    /// Shipped and not cancelled: it counts on the order.
    pub fn stands(&self) -> bool {
        self.state == FulfillmentState::Shipped
    }
}

fn check_tracking(tracking: &[Tracking]) -> Result<(), FulfillmentError> {
    if tracking.len() > Fulfillment::MAX_TRACKING {
        return Err(FulfillmentError::TooMuchTracking {
            count: tracking.len(),
        });
    }
    Ok(())
}

/// Plain-data form of a `Fulfillment` for storage adapters (design note 0001, 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FulfillmentSnapshot {
    pub id: FulfillmentId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub order: OrderId,
    pub location: LocationId,
    /// In the order they were given.
    pub lines: Vec<FulfillmentLine>,
    pub tracking: Vec<Tracking>,
    pub state: FulfillmentState,
}

impl Fulfillment {
    pub fn snapshot(&self) -> FulfillmentSnapshot {
        FulfillmentSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            order: self.order,
            location: self.location,
            lines: self.lines.clone(),
            tracking: self.tracking.clone(),
            state: self.state,
        }
    }

    /// Rebuilds a fulfilment from storage (and builds new ones, through `new`), checking
    /// every invariant.
    pub fn restore(s: FulfillmentSnapshot) -> Result<Self, FulfillmentError> {
        if s.lines.is_empty() {
            return Err(FulfillmentError::NoLines);
        }
        let mut seen = HashSet::new();
        if let Some(twice) = s.lines.iter().find(|l| !seen.insert(l.line)) {
            return Err(FulfillmentError::DuplicateLine(twice.line));
        }
        check_tracking(&s.tracking)?;
        Ok(Self {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            order: s.order,
            location: s.location,
            lines: s.lines,
            tracking: s.tracking,
            state: s.state,
            events: Recorder::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    fn line(n: u128, quantity: u32) -> FulfillmentLine {
        FulfillmentLine {
            line: OrderLineId::from_uuid(Uuid::from_u128(n)),
            quantity: Quantity::new(quantity).unwrap(),
            stock_taken: true,
        }
    }

    fn tracking(number: &str) -> Tracking {
        Tracking::new(Some("DHL"), number, Some("https://dhl.example/t/1")).unwrap()
    }

    fn new(lines: Vec<FulfillmentLine>) -> NewFulfillment {
        NewFulfillment {
            id: FulfillmentId::from_uuid(Uuid::from_u128(1)),
            order: OrderId::from_uuid(Uuid::from_u128(2)),
            location: LocationId::from_uuid(Uuid::from_u128(3)),
            lines,
            tracking: vec![tracking("123")],
        }
    }

    fn shipped() -> Fulfillment {
        let mut f = Fulfillment::new(new(vec![line(10, 2)])).unwrap();
        f.take_events();
        f
    }

    fn types(f: &mut Fulfillment) -> Vec<&'static str> {
        f.take_events().iter().map(|e| e.event_type).collect()
    }

    #[test]
    fn a_new_fulfilment_is_shipped_and_recorded() {
        let mut f = Fulfillment::new(new(vec![line(10, 2), line(11, 1)])).unwrap();
        assert!(f.stands());
        assert_eq!(f.version(), Version::NEW);
        let events = f.take_events();
        assert_eq!(events[0].event_type, "fulfillment.created");
        assert_eq!(events[0].data["lines"][0]["quantity"], 2);
        assert_eq!(events[0].data["tracking"][0]["carrier"], "DHL");
    }

    #[test]
    fn a_fulfilment_needs_a_line() {
        assert_eq!(
            Fulfillment::new(new(vec![])),
            Err(FulfillmentError::NoLines)
        );
    }

    #[test]
    fn each_line_appears_once() {
        assert_eq!(
            Fulfillment::new(new(vec![line(10, 1), line(10, 1)])),
            Err(FulfillmentError::DuplicateLine(OrderLineId::from_uuid(
                Uuid::from_u128(10)
            )))
        );
    }

    #[test]
    fn tracking_is_capped() {
        let mut f = shipped();
        let eleven = (0..11).map(|n| tracking(&n.to_string())).collect();
        assert_eq!(
            f.set_tracking(eleven),
            Err(FulfillmentError::TooMuchTracking { count: 11 })
        );
        let ten = (0..10).map(|n| tracking(&n.to_string())).collect();
        assert_eq!(f.set_tracking(ten), Ok(true));
    }

    #[test]
    fn tracking_text_is_trimmed_and_blanks_are_none() {
        let t = Tracking::new(Some("  "), " 123 ", Some(" ")).unwrap();
        assert_eq!((t.carrier(), t.number(), t.url()), (None, "123", None));
    }

    #[test]
    fn a_tracking_number_is_required() {
        assert_eq!(
            Tracking::new(None, "  ", None),
            Err(FulfillmentError::Text(TextError::Missing(
                "tracking number"
            )))
        );
    }

    #[test]
    fn a_tracking_url_is_http_or_https_and_capped() {
        assert_eq!(
            Tracking::new(None, "1", Some("ftp://x")),
            Err(FulfillmentError::InvalidTrackingUrl("ftp://x".to_string()))
        );
        assert!(Tracking::new(None, "1", Some("http://x")).is_ok());
        let long = format!("https://{}", "x".repeat(Tracking::MAX_URL_LEN - 8));
        assert!(Tracking::new(None, "1", Some(&long)).is_ok());
        let longer = format!("{long}x");
        assert_eq!(
            Tracking::new(None, "1", Some(&longer)),
            Err(FulfillmentError::InvalidTrackingUrl(longer.clone()))
        );
    }

    #[test]
    fn changing_tracking_records_an_update() {
        let mut f = shipped();
        assert_eq!(f.set_tracking(vec![tracking("456")]), Ok(true));
        assert_eq!(types(&mut f), ["fulfillment.updated"]);
        assert_eq!(f.tracking()[0].number(), "456");
    }

    #[test]
    fn the_same_tracking_changes_nothing() {
        let mut f = shipped();
        assert_eq!(f.set_tracking(vec![tracking("123")]), Ok(false));
        assert_eq!(types(&mut f), Vec::<&str>::new());
    }

    #[test]
    fn cancelling_records_it_once() {
        let mut f = shipped();
        let at = Timestamp::from_unix_micros(1_767_225_600_000_000);
        assert!(f.cancel(at));
        assert_eq!(f.state(), FulfillmentState::Cancelled { at });
        assert!(!f.stands());
        assert_eq!(types(&mut f), ["fulfillment.cancelled"]);
        assert!(!f.cancel(Timestamp::from_unix_micros(1)));
        assert_eq!(f.state(), FulfillmentState::Cancelled { at });
        assert_eq!(types(&mut f), Vec::<&str>::new());
    }

    #[test]
    fn a_cancelled_fulfilments_tracking_stays() {
        let mut f = shipped();
        f.cancel(Timestamp::UNIX_EPOCH);
        assert_eq!(
            f.set_tracking(vec![]),
            Err(FulfillmentError::Cancelled(f.id()))
        );
    }

    #[test]
    fn restore_round_trips() {
        let mut f = Fulfillment::new(new(vec![line(10, 2), line(11, 1)])).unwrap();
        f.take_events();
        f.cancel(Timestamp::UNIX_EPOCH);
        f.take_events();
        assert_eq!(Fulfillment::restore(f.snapshot()), Ok(f));
    }
}
