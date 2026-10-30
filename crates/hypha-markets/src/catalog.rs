use std::fmt;

use hypha_core::{
    CatalogId, MarketId, PendingEvent, PriceListId, Recorded, Recorder, Stamped, Stamps, Subject,
    Version, Versioned,
};

use crate::{
    error::MarketError,
    events::{CATALOG, CatalogChanged, CatalogPriceListsAdded, CatalogPriceListsRemoved},
};

/// Where a catalog applies. An enum so other contexts (e.g. B2B companies) can be added later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatalogContext {
    Market(MarketId),
}

impl fmt::Display for CatalogContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogContext::Market(id) => write!(f, "market {id}"),
        }
    }
}

/// Which products are sellable in a context, and the price lists that compete to price
/// them there (the lowest live price wins).
///
/// The products themselves are not held here: a catalog can include a whole large
/// catalog's worth of them. They live behind `CatalogRepo::{add_products,
/// included_products}` and are queried in batches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    id: CatalogId,
    version: Version,
    /// When it was stored and last changed, set by the adapter.
    stamps: Option<Stamps>,
    context: CatalogContext,
    /// Sorted by ID, each once: a set.
    price_lists: Vec<PriceListId>,
    events: Recorder,
}

impl Recorded for Catalog {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for Catalog {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Catalog {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Catalog {
    /// Most price lists one catalog holds: each is a candidate for every price, and they
    /// load together.
    pub const MAX_PRICE_LISTS: usize = 50;

    /// A new catalog with `price_lists` (repeats count once).
    pub fn new(
        id: CatalogId,
        context: CatalogContext,
        price_lists: impl IntoIterator<Item = PriceListId>,
    ) -> Self {
        let mut lists: Vec<PriceListId> = price_lists.into_iter().collect();
        lists.sort();
        lists.dedup();
        let mut catalog = Self {
            id,
            version: Version::NEW,
            stamps: None,
            context,
            price_lists: lists,
            events: Recorder::default(),
        };
        catalog.changed();
        catalog
    }

    fn changed(&mut self) {
        let event = CatalogChanged { catalog: self.id };
        self.events
            .record_once(Subject::new(CATALOG, self.id.as_uuid()), &event);
    }

    /// Replaces the catalog's price lists with `lists`: each once, at most
    /// `MAX_PRICE_LISTS`. Records which were added and removed.
    pub fn set_price_lists(&mut self, lists: Vec<PriceListId>) -> Result<(), MarketError> {
        let mut sorted = lists;
        sorted.sort();
        if let Some(twice) = sorted.windows(2).find(|w| w[0] == w[1]) {
            return Err(MarketError::DuplicatePriceList(twice[0]));
        }
        if sorted.len() > Self::MAX_PRICE_LISTS {
            return Err(MarketError::TooManyPriceLists {
                max: Self::MAX_PRICE_LISTS,
            });
        }
        let added: Vec<PriceListId> = sorted
            .iter()
            .filter(|l| !self.price_lists.contains(l))
            .copied()
            .collect();
        let removed: Vec<PriceListId> = self
            .price_lists
            .iter()
            .filter(|l| !sorted.contains(l))
            .copied()
            .collect();
        if added.is_empty() && removed.is_empty() {
            return Ok(());
        }
        self.price_lists = sorted;
        let subject = Subject::new(CATALOG, self.id.as_uuid());
        if !added.is_empty() {
            let event = CatalogPriceListsAdded {
                catalog: self.id,
                price_lists: added,
            };
            self.events.record(subject, &event);
        }
        if !removed.is_empty() {
            let event = CatalogPriceListsRemoved {
                catalog: self.id,
                price_lists: removed,
            };
            self.events.record(subject, &event);
        }
        // What priced it changed: caches keyed on the catalog follow `catalog.changed`.
        self.changed();
        Ok(())
    }

    /// Adds `list`; one already there changes nothing.
    pub fn add_price_list(&mut self, list: PriceListId) -> Result<(), MarketError> {
        if self.price_lists.contains(&list) {
            return Ok(());
        }
        let mut lists = self.price_lists.clone();
        lists.push(list);
        self.set_price_lists(lists)
    }

    /// Removes `list`, which must be on the catalog (`PriceListNotInCatalog`).
    pub fn remove_price_list(&mut self, list: PriceListId) -> Result<(), MarketError> {
        if !self.price_lists.contains(&list) {
            return Err(MarketError::PriceListNotInCatalog(list));
        }
        let lists = self
            .price_lists
            .iter()
            .copied()
            .filter(|l| *l != list)
            .collect();
        self.set_price_lists(lists)
    }

    pub fn id(&self) -> CatalogId {
        self.id
    }

    pub fn context(&self) -> CatalogContext {
        self.context
    }

    /// Sorted by ID.
    pub fn price_lists(&self) -> &[PriceListId] {
        &self.price_lists
    }
}

/// Plain-data form of a `Catalog` for storage adapters. Its products are stored separately
/// (see `CatalogRepo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSnapshot {
    pub id: CatalogId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub context: CatalogContext,
    pub price_lists: Vec<PriceListId>,
}

impl Catalog {
    pub fn snapshot(&self) -> CatalogSnapshot {
        CatalogSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            context: self.context,
            price_lists: self.price_lists.clone(),
        }
    }

    /// Rebuilds a catalog from storage, re-checking its price lists' rules.
    pub fn restore(s: CatalogSnapshot) -> Result<Catalog, MarketError> {
        let mut catalog = Catalog {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            context: s.context,
            price_lists: Vec::new(),
            events: Recorder::default(),
        };
        catalog.set_price_lists(s.price_lists)?;
        catalog.events = Recorder::default();
        Ok(catalog)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn in_market(n: u128) -> CatalogContext {
        CatalogContext::Market(MarketId::from_uuid(Uuid::from_u128(n)))
    }

    fn catalog(n: u128, context: CatalogContext) -> Catalog {
        Catalog::new(CatalogId::from_uuid(Uuid::from_u128(n)), context, None)
    }

    fn pl(n: u128) -> PriceListId {
        PriceListId::from_uuid(Uuid::from_u128(n))
    }

    fn types(c: &mut Catalog) -> Vec<String> {
        c.take_events()
            .into_iter()
            .map(|e| e.event_type.to_string())
            .collect()
    }

    #[test]
    fn set_price_lists_keeps_them_sorted_and_records_the_changes() {
        let mut c = catalog(1, in_market(1));
        c.take_events();
        c.set_price_lists(vec![pl(3), pl(1)]).unwrap();
        assert_eq!(c.price_lists(), [pl(1), pl(3)]);
        assert_eq!(
            types(&mut c),
            ["catalog.price_lists_added", "catalog.changed"]
        );
        c.set_price_lists(vec![pl(3), pl(2)]).unwrap();
        let events = c.take_events();
        assert_eq!(events[0].data["price_lists"], serde_json::json!([pl(2)]));
        assert_eq!(events[1].event_type, "catalog.price_lists_removed");
        assert_eq!(events[1].data["price_lists"], serde_json::json!([pl(1)]));
    }

    #[test]
    fn setting_the_same_lists_records_nothing() {
        let mut c = Catalog::new(
            CatalogId::from_uuid(Uuid::from_u128(1)),
            in_market(1),
            [pl(1)],
        );
        c.take_events();
        c.set_price_lists(vec![pl(1)]).unwrap();
        assert!(types(&mut c).is_empty());
    }

    #[test]
    fn a_list_is_on_a_catalog_once() {
        let mut c = catalog(1, in_market(1));
        assert_eq!(
            c.set_price_lists(vec![pl(1), pl(1)]),
            Err(MarketError::DuplicatePriceList(pl(1)))
        );
        c.add_price_list(pl(1)).unwrap();
        c.add_price_list(pl(1)).unwrap();
        assert_eq!(c.price_lists(), [pl(1)]);
    }

    #[test]
    fn a_catalog_holds_at_most_fifty_lists() {
        let mut c = catalog(1, in_market(1));
        assert_eq!(c.set_price_lists((1..=50).map(pl).collect()), Ok(()));
        assert_eq!(
            c.set_price_lists((1..=51).map(pl).collect()),
            Err(MarketError::TooManyPriceLists { max: 50 })
        );
    }

    #[test]
    fn removing_a_list_it_lacks_is_an_error() {
        let mut c = catalog(1, in_market(1));
        assert_eq!(
            c.remove_price_list(pl(1)),
            Err(MarketError::PriceListNotInCatalog(pl(1)))
        );
    }

    #[test]
    fn restore_round_trips() {
        let mut c = catalog(1, in_market(1));
        c.set_price_lists(vec![pl(9), pl(4)]).unwrap();
        c.set_version(Version::new(4));
        assert_eq!(Catalog::restore(c.snapshot()), Ok(c));
    }
}
