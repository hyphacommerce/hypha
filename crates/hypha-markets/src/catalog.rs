use std::fmt;

use hypha_core::{
    CatalogId, MarketId, PendingEvent, PriceListId, Recorded, Recorder, Stamped, Stamps, Subject,
    Version, Versioned,
};

use crate::events::{CATALOG, CatalogChanged};

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

/// Which products are sellable in a context, and at which price list.
///
/// The products themselves are not held here: a catalog can include a whole large
/// catalog's worth of them. They live behind `CatalogRepo::{add_products,
/// included_products}` and are queried in batches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    id: CatalogId,
    version: Version,
    /// When it was stored and last changed, set by the adapter (design note 0016).
    stamps: Option<Stamps>,
    context: CatalogContext,
    price_list: Option<PriceListId>,
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
    pub fn new(id: CatalogId, context: CatalogContext, price_list: Option<PriceListId>) -> Self {
        let mut catalog = Self {
            id,
            version: Version::NEW,
            stamps: None,
            context,
            price_list,
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

    pub fn set_price_list(&mut self, price_list: Option<PriceListId>) {
        if self.price_list != price_list {
            self.price_list = price_list;
            self.changed();
        }
    }

    pub fn id(&self) -> CatalogId {
        self.id
    }

    pub fn context(&self) -> CatalogContext {
        self.context
    }

    pub fn price_list(&self) -> Option<PriceListId> {
        self.price_list
    }
}

/// Plain-data form of a `Catalog` for storage adapters (design note 0001, 3.4).
/// Its products are stored separately (see `CatalogRepo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSnapshot {
    pub id: CatalogId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub context: CatalogContext,
    pub price_list: Option<PriceListId>,
}

impl Catalog {
    pub fn snapshot(&self) -> CatalogSnapshot {
        CatalogSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            context: self.context,
            price_list: self.price_list,
        }
    }

    /// Infallible: a catalog has no rules beyond its field types.
    pub fn restore(s: CatalogSnapshot) -> Catalog {
        Catalog {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            context: s.context,
            price_list: s.price_list,
            events: Recorder::default(),
        }
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

    #[test]
    fn set_price_list_replaces_and_clears_it() {
        let mut c = catalog(1, in_market(1));
        let pl = PriceListId::from_uuid(Uuid::from_u128(9));
        c.set_price_list(Some(pl));
        assert_eq!(c.price_list(), Some(pl));
        c.set_price_list(None);
        assert_eq!(c.price_list(), None);
    }

    #[test]
    fn restore_round_trips() {
        let mut c = catalog(1, in_market(1));
        c.set_price_list(Some(PriceListId::from_uuid(Uuid::from_u128(9))));
        c.set_version(Version::new(4));
        assert_eq!(Catalog::restore(c.snapshot()), c);
    }
}
