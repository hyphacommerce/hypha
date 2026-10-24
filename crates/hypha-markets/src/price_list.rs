use std::collections::{BTreeSet, HashSet};

use hypha_core::{
    BasisPoints, Currency, PendingEvent, Price, PriceListId, Recorded, Stamped, Stamps, Subject,
    Timestamp, VariantId, Version, Versioned, optional_text, required_text,
};

use crate::{
    error::{FixedPriceProblem, PriceListError},
    events::{PRICE_LIST, PriceListChanged, PriceListCreated, PriceListDeleted},
};

/// What a list's prices mean to a shopper. Either kind competes the same way (the lowest
/// live price wins); the kind decides only the compare-at price shown beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PriceListKind {
    /// A reduction: measured against the regular price, which is struck through.
    Sale,
    /// The prices a market or audience pays: they become the regular price.
    Override,
}

impl PriceListKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PriceListKind::Sale => "sale",
            PriceListKind::Override => "override",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sale" => Some(PriceListKind::Sale),
            "override" => Some(PriceListKind::Override),
            _ => None,
        }
    }
}

/// Whether a list may price anything. A draft never does, whatever its window says, so a
/// scheduled list can be paused without losing its dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PriceListStatus {
    Draft,
    Active,
}

impl PriceListStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PriceListStatus::Draft => "draft",
            PriceListStatus::Active => "active",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(PriceListStatus::Draft),
            "active" => Some(PriceListStatus::Active),
            _ => None,
        }
    }
}

/// Where a list stands at a moment, from its status and window: what the admin shows and
/// filters by. Only `Active` prices anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PriceListPhase {
    Draft,
    /// Active, but its window hasn't opened yet.
    Scheduled,
    Active,
    /// Active, but its window has closed.
    Expired,
}

impl PriceListPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            PriceListPhase::Draft => "draft",
            PriceListPhase::Scheduled => "scheduled",
            PriceListPhase::Active => "active",
            PriceListPhase::Expired => "expired",
        }
    }
}

/// How an `override` list shows the variant's own compare-at price when its fixed price
/// has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompareAtMode {
    /// The variant's compare-at, with the list's adjustment applied.
    Adjusted,
    /// None at all.
    Hidden,
}

impl CompareAtMode {
    pub fn as_str(self) -> &'static str {
        match self {
            CompareAtMode::Adjusted => "adjusted",
            CompareAtMode::Hidden => "hidden",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "adjusted" => Some(CompareAtMode::Adjusted),
            "hidden" => Some(CompareAtMode::Hidden),
            _ => None,
        }
    }
}

/// A percentage applied to base prices. A list without one prices only the variants it
/// holds fixed prices for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adjustment {
    percent: BasisPoints,
}

impl Adjustment {
    pub fn new(percent: BasisPoints) -> Self {
        Self { percent }
    }

    pub fn percent(&self) -> BasisPoints {
        self.percent
    }
}

/// When a list may price: from `starts_at` (inclusive) to `ends_at` (exclusive), either
/// end open when `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Window {
    starts_at: Option<Timestamp>,
    ends_at: Option<Timestamp>,
}

impl Window {
    /// An end at or before the start is `EndsNotAfterStart`: such a window would never
    /// open.
    pub fn new(
        starts_at: Option<Timestamp>,
        ends_at: Option<Timestamp>,
    ) -> Result<Self, PriceListError> {
        if let (Some(start), Some(end)) = (starts_at, ends_at)
            && end <= start
        {
            return Err(PriceListError::EndsNotAfterStart);
        }
        Ok(Self { starts_at, ends_at })
    }

    pub fn starts_at(&self) -> Option<Timestamp> {
        self.starts_at
    }

    pub fn ends_at(&self) -> Option<Timestamp> {
        self.ends_at
    }
}

/// A price list: a currency, a kind, a status and window, an optional adjustment, and
/// fixed per-variant prices.
///
/// The fixed prices are not held here. A list can have one per variant in a large
/// catalog, so they live behind `PriceListRepo::{set_fixed_prices, fixed_prices}`
/// and are read in batches. The rules for them stay here: see `check_fixed_prices`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceList {
    id: PriceListId,
    version: Version,
    /// When it was stored and last changed, set by the adapter.
    stamps: Option<Stamps>,
    name: String,
    description: Option<String>,
    currency: Currency,
    kind: PriceListKind,
    status: PriceListStatus,
    window: Window,
    compare_at: CompareAtMode,
    adjustment: Option<Adjustment>,
    changes: Changes,
}

/// What happened to a list since it was loaded, coalesced into one event per write.
/// Compares as always equal, so it never makes two lists unequal.
#[derive(Debug, Clone, Default)]
struct Changes {
    created: bool,
    deleted: bool,
    fields: BTreeSet<&'static str>,
}

impl PartialEq for Changes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changes {}

impl Recorded for PriceList {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let changes = std::mem::take(&mut self.changes);
        let subject = Subject::new(PRICE_LIST, self.id.as_uuid());
        if changes.deleted {
            let event = PriceListDeleted {
                price_list: self.id,
            };
            return vec![PendingEvent::new(subject, &event)];
        }
        if changes.created {
            let event = PriceListCreated {
                price_list: self.id,
            };
            return vec![PendingEvent::new(subject, &event)];
        }
        if changes.fields.is_empty() {
            return Vec::new();
        }
        let event = PriceListChanged {
            price_list: self.id,
            fields: changes.fields.into_iter().collect(),
        };
        vec![PendingEvent::new(subject, &event)]
    }
}

impl Versioned for PriceList {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for PriceList {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

/// A new list as a caller describes it; `PriceList::new` checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceListDraft {
    pub name: String,
    pub description: Option<String>,
    pub currency: Currency,
    pub kind: PriceListKind,
    pub status: PriceListStatus,
    pub starts_at: Option<Timestamp>,
    pub ends_at: Option<Timestamp>,
    pub compare_at: CompareAtMode,
    pub adjustment: Option<Adjustment>,
}

impl PriceListDraft {
    /// An active `override` list without a window or an adjustment, showing compare-at
    /// prices adjusted: the defaults a new list starts from.
    pub fn new(name: &str, currency: Currency) -> Self {
        Self {
            name: name.to_string(),
            description: None,
            currency,
            kind: PriceListKind::Override,
            status: PriceListStatus::Active,
            starts_at: None,
            ends_at: None,
            compare_at: CompareAtMode::Adjusted,
            adjustment: None,
        }
    }

    /// The same draft, adjusting base prices by `percent`.
    pub fn adjusted(self, percent: BasisPoints) -> Self {
        Self {
            adjustment: Some(Adjustment::new(percent)),
            ..self
        }
    }
}

impl PriceList {
    /// The name follows the shared text rule (trimmed, 1 to 255 characters), and so does
    /// the description (blank is none). Unique names are the repository's rule
    /// (`keys::PRICE_LIST_NAME`).
    pub fn new(id: PriceListId, draft: PriceListDraft) -> Result<Self, PriceListError> {
        let mut list = Self {
            id,
            version: Version::NEW,
            stamps: None,
            name: required_text("name", &draft.name)?,
            description: optional_text("description", draft.description)?,
            currency: draft.currency,
            kind: draft.kind,
            status: draft.status,
            window: Window::new(draft.starts_at, draft.ends_at)?,
            compare_at: draft.compare_at,
            adjustment: draft.adjustment,
            changes: Changes::default(),
        };
        list.changes.created = true;
        Ok(list)
    }

    fn changed(&mut self, field: &'static str) {
        self.changes.fields.insert(field);
    }

    /// Validates fixed prices before they are stored for this list: each variant at most
    /// once, and every price in the list's currency. Reports every entry's problem at
    /// once, with its index in `prices`, so a caller can fix them all in one go.
    pub fn check_fixed_prices(
        &self,
        prices: &[(VariantId, FixedPrice)],
    ) -> Result<(), PriceListError> {
        let mut problems = Vec::new();
        let mut seen = HashSet::with_capacity(prices.len());
        for (index, (variant, fixed)) in prices.iter().enumerate() {
            let problem = if !seen.insert(*variant) {
                Some(FixedPriceProblem::Duplicate)
            } else if fixed.price().money().currency() != self.currency {
                Some(FixedPriceProblem::CurrencyMismatch {
                    expected: self.currency,
                    found: fixed.price().money().currency(),
                })
            } else {
                None
            };
            if let Some(problem) = problem {
                problems.push(EntryProblem {
                    index,
                    variant: Some(*variant),
                    problem,
                });
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(PriceListError::InvalidFixedPrices(problems))
        }
    }

    /// The shared text rule: trimmed, 1 to 255 characters.
    pub fn rename(&mut self, name: &str) -> Result<(), PriceListError> {
        let name = required_text("name", name)?;
        if self.name != name {
            self.name = name;
            self.changed("name");
        }
        Ok(())
    }

    /// Blank clears it.
    pub fn set_description(&mut self, description: Option<String>) -> Result<(), PriceListError> {
        let description = optional_text("description", description)?;
        if self.description != description {
            self.description = description;
            self.changed("description");
        }
        Ok(())
    }

    pub fn set_kind(&mut self, kind: PriceListKind) {
        if self.kind != kind {
            self.kind = kind;
            self.changed("kind");
        }
    }

    pub fn set_status(&mut self, status: PriceListStatus) {
        if self.status != status {
            self.status = status;
            self.changed("status");
        }
    }

    pub fn set_window(&mut self, window: Window) {
        if self.window.starts_at != window.starts_at {
            self.changed("starts_at");
        }
        if self.window.ends_at != window.ends_at {
            self.changed("ends_at");
        }
        self.window = window;
    }

    pub fn set_compare_at(&mut self, mode: CompareAtMode) {
        if self.compare_at != mode {
            self.compare_at = mode;
            self.changed("compare_at");
        }
    }

    pub fn set_adjustment(&mut self, adjustment: Option<Adjustment>) {
        if self.adjustment != adjustment {
            self.adjustment = adjustment;
            self.changed("adjustment");
        }
    }

    /// Records `price_list.deleted`; the repository removes it (`PriceListRepo::delete`).
    pub fn delete(&mut self) {
        self.changes.deleted = true;
    }

    /// Where the list stands at `at`.
    pub fn phase(&self, at: Timestamp) -> PriceListPhase {
        if self.status == PriceListStatus::Draft {
            return PriceListPhase::Draft;
        }
        if self.window.starts_at.is_some_and(|start| at < start) {
            return PriceListPhase::Scheduled;
        }
        if self.window.ends_at.is_some_and(|end| at >= end) {
            return PriceListPhase::Expired;
        }
        PriceListPhase::Active
    }

    /// Whether it prices anything at `at`: active, and within its window.
    pub fn is_live(&self, at: Timestamp) -> bool {
        self.phase(at) == PriceListPhase::Active
    }

    pub fn id(&self) -> PriceListId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    pub fn kind(&self) -> PriceListKind {
        self.kind
    }

    pub fn status(&self) -> PriceListStatus {
        self.status
    }

    pub fn window(&self) -> Window {
        self.window
    }

    pub fn compare_at(&self) -> CompareAtMode {
        self.compare_at
    }

    pub fn adjustment(&self) -> Option<&Adjustment> {
        self.adjustment.as_ref()
    }
}

/// One entry's problem in a list of fixed prices, at its position in the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryProblem {
    pub index: usize,
    /// `None` when the entry's variant couldn't be read.
    pub variant: Option<VariantId>,
    pub problem: FixedPriceProblem,
}

/// A list's price for one variant: what it pays, and what the shopper sees it compared
/// against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedPrice {
    price: Price,
    compare_at: Option<Price>,
}

impl FixedPrice {
    /// A compare-at price must be in the price's currency and above it: otherwise it
    /// would strike through something cheaper.
    pub fn new(price: Price, compare_at: Option<Price>) -> Result<Self, FixedPriceProblem> {
        if let Some(compare_at) = compare_at {
            if compare_at.money().currency() != price.money().currency() {
                return Err(FixedPriceProblem::CompareAtCurrencyMismatch);
            }
            if compare_at.money().amount() <= price.money().amount() {
                return Err(FixedPriceProblem::CompareAtNotAbovePrice);
            }
        }
        Ok(Self { price, compare_at })
    }

    /// A price with nothing else.
    pub fn plain(price: Price) -> Self {
        Self {
            price,
            compare_at: None,
        }
    }

    pub fn price(&self) -> Price {
        self.price
    }

    pub fn compare_at(&self) -> Option<Price> {
        self.compare_at
    }
}

/// Plain-data form of a `PriceList` for storage adapters. Its fixed prices are stored
/// separately (see `PriceListRepo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceListSnapshot {
    pub id: PriceListId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
    pub description: Option<String>,
    pub currency: Currency,
    pub kind: PriceListKind,
    pub status: PriceListStatus,
    pub starts_at: Option<Timestamp>,
    pub ends_at: Option<Timestamp>,
    pub compare_at: CompareAtMode,
    /// The adjustment's percentage; `None` without one.
    pub adjustment: Option<BasisPoints>,
}

impl PriceList {
    pub fn snapshot(&self) -> PriceListSnapshot {
        PriceListSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            name: self.name.clone(),
            description: self.description.clone(),
            currency: self.currency,
            kind: self.kind,
            status: self.status,
            starts_at: self.window.starts_at,
            ends_at: self.window.ends_at,
            compare_at: self.compare_at,
            adjustment: self.adjustment.as_ref().map(|a| a.percent),
        }
    }

    /// Rebuilds a list from storage, re-checking the rules `new` enforces.
    pub fn restore(s: PriceListSnapshot) -> Result<PriceList, PriceListError> {
        Ok(PriceList {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            name: required_text("name", &s.name)?,
            description: optional_text("description", s.description)?,
            currency: s.currency,
            kind: s.kind,
            status: s.status,
            window: Window::new(s.starts_at, s.ends_at)?,
            compare_at: s.compare_at,
            adjustment: s.adjustment.map(Adjustment::new),
            changes: Changes::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{Money, TextError};
    use uuid::Uuid;

    use super::*;

    fn vid(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn price(amount: i64, currency: Currency) -> Price {
        Price::new(Money::new(amount, currency).unwrap()).unwrap()
    }

    fn fixed(amount: i64, currency: Currency) -> FixedPrice {
        FixedPrice::plain(price(amount, currency))
    }

    fn at(micros: i64) -> Timestamp {
        Timestamp::from_unix_micros(micros)
    }

    fn eur_list() -> PriceList {
        PriceList::new(
            PriceListId::from_uuid(Uuid::from_u128(1)),
            PriceListDraft::new("Euro prices", Currency::EUR),
        )
        .unwrap()
    }

    fn windowed(starts: Option<i64>, ends: Option<i64>) -> PriceList {
        PriceList::new(
            PriceListId::from_uuid(Uuid::from_u128(1)),
            PriceListDraft {
                starts_at: starts.map(at),
                ends_at: ends.map(at),
                ..PriceListDraft::new("Sale", Currency::EUR)
            },
        )
        .unwrap()
    }

    fn types(list: &mut PriceList) -> Vec<String> {
        list.take_events()
            .into_iter()
            .map(|e| e.event_type.to_string())
            .collect()
    }

    #[test]
    fn new_keeps_adjustment() {
        let markup = BasisPoints::new(1500).unwrap();
        let list = PriceList::new(
            PriceListId::from_uuid(Uuid::from_u128(1)),
            PriceListDraft::new("US", Currency::USD).adjusted(markup),
        )
        .unwrap();
        assert_eq!(list.adjustment().map(Adjustment::percent), Some(markup));
    }

    #[test]
    fn name_is_trimmed_and_required() {
        let named = |name: &str| {
            PriceList::new(
                PriceListId::from_uuid(Uuid::from_u128(1)),
                PriceListDraft::new(name, Currency::EUR),
            )
        };
        assert_eq!(named("  Sale ").unwrap().name(), "Sale");
        assert_eq!(
            named(" ").map(|l| l.id()),
            Err(PriceListError::Text(TextError::Missing("name")))
        );
    }

    #[test]
    fn window_must_end_after_it_starts() {
        assert_eq!(
            Window::new(Some(at(10)), Some(at(10))),
            Err(PriceListError::EndsNotAfterStart)
        );
        assert_eq!(
            Window::new(Some(at(10)), Some(at(9))),
            Err(PriceListError::EndsNotAfterStart)
        );
        assert!(Window::new(Some(at(10)), Some(at(11))).is_ok());
        assert!(Window::new(None, Some(at(1))).is_ok());
    }

    #[test]
    fn a_draft_is_never_live() {
        let mut list = windowed(None, None);
        list.set_status(PriceListStatus::Draft);
        assert_eq!(list.phase(at(5)), PriceListPhase::Draft);
        assert!(!list.is_live(at(5)));
    }

    #[test]
    fn a_window_opens_at_its_start() {
        let list = windowed(Some(10), None);
        assert_eq!(list.phase(at(9)), PriceListPhase::Scheduled);
        assert_eq!(list.phase(at(10)), PriceListPhase::Active);
    }

    #[test]
    fn a_window_closes_at_its_end() {
        let list = windowed(None, Some(20));
        assert_eq!(list.phase(at(19)), PriceListPhase::Active);
        assert_eq!(list.phase(at(20)), PriceListPhase::Expired);
    }

    #[test]
    fn a_new_list_records_created() {
        let mut list = eur_list();
        list.rename("Other").unwrap();
        assert_eq!(types(&mut list), ["price_list.created"]);
    }

    #[test]
    fn changes_name_the_fields_they_changed() {
        let mut list = eur_list();
        list.take_events();
        list.set_kind(PriceListKind::Sale);
        list.set_window(Window::new(Some(at(1)), None).unwrap());
        list.set_status(PriceListStatus::Active);
        let events = list.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "price_list.changed");
        assert_eq!(
            events[0].data["fields"],
            serde_json::json!(["kind", "starts_at"])
        );
    }

    #[test]
    fn deleting_records_deleted() {
        let mut list = eur_list();
        list.take_events();
        list.delete();
        assert_eq!(types(&mut list), ["price_list.deleted"]);
    }

    #[test]
    fn accepts_prices_in_list_currency() {
        let prices = [
            (vid(1), fixed(1000, Currency::EUR)),
            (vid(2), fixed(0, Currency::EUR)),
        ];
        assert_eq!(eur_list().check_fixed_prices(&prices), Ok(()));
    }

    #[test]
    fn accepts_no_prices() {
        assert_eq!(eur_list().check_fixed_prices(&[]), Ok(()));
    }

    #[test]
    fn reports_every_problem_with_its_index() {
        let prices = [
            (vid(3), fixed(300, Currency::RSD)),
            (vid(2), fixed(200, Currency::EUR)),
            (vid(2), fixed(100, Currency::EUR)),
        ];
        assert_eq!(
            eur_list().check_fixed_prices(&prices),
            Err(PriceListError::InvalidFixedPrices(vec![
                EntryProblem {
                    index: 0,
                    variant: Some(vid(3)),
                    problem: FixedPriceProblem::CurrencyMismatch {
                        expected: Currency::EUR,
                        found: Currency::RSD,
                    },
                },
                EntryProblem {
                    index: 2,
                    variant: Some(vid(2)),
                    problem: FixedPriceProblem::Duplicate,
                },
            ]))
        );
    }

    #[test]
    fn compare_at_must_be_above_the_price() {
        assert_eq!(
            FixedPrice::new(price(900, Currency::EUR), Some(price(900, Currency::EUR))),
            Err(FixedPriceProblem::CompareAtNotAbovePrice)
        );
        assert!(
            FixedPrice::new(price(900, Currency::EUR), Some(price(901, Currency::EUR))).is_ok()
        );
    }

    #[test]
    fn compare_at_must_be_in_the_price_currency() {
        assert_eq!(
            FixedPrice::new(price(900, Currency::EUR), Some(price(1000, Currency::USD))),
            Err(FixedPriceProblem::CompareAtCurrencyMismatch)
        );
    }

    #[test]
    fn restore_round_trips() {
        let mut list = eur_list();
        list.set_adjustment(Some(Adjustment::new(BasisPoints::new(-1500).unwrap())));
        list.set_description(Some("For the euro markets".to_string()))
            .unwrap();
        list.set_window(Window::new(Some(at(1)), Some(at(2))).unwrap());
        list.set_version(Version::new(2));
        assert_eq!(PriceList::restore(list.snapshot()), Ok(list));
    }

    #[test]
    fn restore_rechecks_the_window() {
        let mut snapshot = eur_list().snapshot();
        snapshot.starts_at = Some(at(5));
        snapshot.ends_at = Some(at(5));
        assert_eq!(
            PriceList::restore(snapshot),
            Err(PriceListError::EndsNotAfterStart)
        );
    }
}
