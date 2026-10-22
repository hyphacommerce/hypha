use std::collections::HashSet;

use std::collections::BTreeSet;

use hypha_core::{
    Address, AddressParts, LocationId, MarketId, PendingEvent, Recorded, Recorder, Stamped, Stamps,
    Subject, Version, Versioned, required_text,
};

use crate::{InventoryError, LocationChanged, events::LOCATION};

/// A place that holds stock: a warehouse, a store, a 3PL.
///
/// Its stock counts for the markets it lists (Shopify: a location's stock shows only in the
/// regions it ships to). A location with no markets still holds stock, it just doesn't sell
/// anywhere. When several locations could fill a line, the lowest `priority` goes first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    id: LocationId,
    version: Version,
    /// When it was stored and last changed, set by the adapter.
    stamps: Option<Stamps>,
    name: String,
    markets: Vec<MarketId>,
    priority: u32,
    /// Where it ships from: the origin carriers quote from.
    address: Option<Address>,
    /// The fields changed since it was loaded, for one `location.changed` per write.
    changed: Changed,
    /// Its event, once `take_events` builds it (`Recorder` keeps aggregates' equality).
    events: Recorder,
}

/// Like `Recorder`, it compares as always equal, so a restored location equals the one
/// written.
#[derive(Debug, Clone, Default)]
struct Changed(BTreeSet<&'static str>);

impl PartialEq for Changed {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changed {}

impl Recorded for Location {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        if !self.changed.0.is_empty() {
            let fields = std::mem::take(&mut self.changed.0).into_iter().collect();
            self.events.record(
                Subject::new(LOCATION, self.id.as_uuid()),
                &LocationChanged {
                    location: self.id,
                    fields,
                },
            );
        }
        self.events.take()
    }
}

impl Versioned for Location {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Location {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Location {
    pub fn new(
        id: LocationId,
        name: String,
        markets: Vec<MarketId>,
        priority: u32,
    ) -> Result<Self, InventoryError> {
        let mut location = Self {
            id,
            version: Version::NEW,
            stamps: None,
            name: String::new(),
            markets: Vec::new(),
            priority,
            address: None,
            changed: Changed::default(),
            events: Recorder::default(),
        };
        location.set_name(name)?;
        location.set_markets(markets)?;
        location.changed = Changed(["address", "markets", "name", "priority"].into());
        Ok(location)
    }

    pub fn set_name(&mut self, name: String) -> Result<(), InventoryError> {
        let name = required_text("name", &name)?;
        if name != self.name {
            self.name = name;
            self.changed.0.insert("name");
        }
        Ok(())
    }

    /// Replaces the markets this location sells to.
    pub fn set_markets(&mut self, markets: Vec<MarketId>) -> Result<(), InventoryError> {
        let mut seen = HashSet::new();
        if let Some(duplicate) = markets.iter().find(|m| !seen.insert(**m)) {
            return Err(InventoryError::DuplicateMarket(*duplicate));
        }
        if markets != self.markets {
            self.markets = markets;
            self.changed.0.insert("markets");
        }
        Ok(())
    }

    pub fn set_priority(&mut self, priority: u32) {
        if priority != self.priority {
            self.priority = priority;
            self.changed.0.insert("priority");
        }
    }

    pub fn set_address(&mut self, address: Option<Address>) {
        if address != self.address {
            self.address = address;
            self.changed.0.insert("address");
        }
    }

    pub fn serves(&self, market: MarketId) -> bool {
        self.markets.contains(&market)
    }

    pub fn id(&self) -> LocationId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn markets(&self) -> &[MarketId] {
        &self.markets
    }

    pub fn priority(&self) -> u32 {
        self.priority
    }

    pub fn address(&self) -> Option<&Address> {
        self.address.as_ref()
    }
}

/// Plain-data form of a `Location` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationSnapshot {
    pub id: LocationId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
    pub markets: Vec<MarketId>,
    pub priority: u32,
    /// Checked by `Address::new` on restore.
    pub address: Option<AddressParts>,
}

impl Location {
    pub fn snapshot(&self) -> LocationSnapshot {
        LocationSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            name: self.name.clone(),
            markets: self.markets.clone(),
            priority: self.priority,
            address: self.address.as_ref().map(|a| a.parts().clone()),
        }
    }

    /// Rebuilds a location from storage, with the same checks as `new`.
    pub fn restore(s: LocationSnapshot) -> Result<Location, InventoryError> {
        let mut location = Location::new(s.id, s.name, s.markets, s.priority)?;
        location.address = s.address.map(Address::new).transpose()?;
        location.version = s.version;
        location.stamps = s.stamps;
        location.changed = Changed::default();
        Ok(location)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use hypha_core::TextError;

    use super::*;

    fn market(n: u128) -> MarketId {
        MarketId::from_uuid(Uuid::from_u128(n))
    }

    fn location(markets: Vec<MarketId>) -> Result<Location, InventoryError> {
        Location::new(
            LocationId::from_uuid(Uuid::from_u128(1)),
            "Zagreb".to_string(),
            markets,
            0,
        )
    }

    #[test]
    fn serves_only_its_markets() {
        let l = location(vec![market(1)]).unwrap();
        assert!(l.serves(market(1)));
        assert!(!l.serves(market(2)));
    }

    #[test]
    fn may_serve_no_market() {
        assert!(location(vec![]).is_ok());
    }

    #[test]
    fn rejects_a_market_listed_twice() {
        assert_eq!(
            location(vec![market(1), market(2), market(1)]),
            Err(InventoryError::DuplicateMarket(market(1)))
        );
    }

    #[test]
    fn rejects_a_blank_name() {
        let mut l = location(vec![]).unwrap();
        assert_eq!(
            l.set_name("  ".to_string()),
            Err(InventoryError::Text(TextError::Missing("name")))
        );
        assert_eq!(l.name(), "Zagreb");
    }

    fn fields(l: &mut Location) -> Vec<serde_json::Value> {
        l.take_events()
            .into_iter()
            .map(|e| e.data["fields"].clone())
            .collect()
    }

    #[test]
    fn a_new_location_names_every_field() {
        let mut l = location(vec![market(1)]).unwrap();
        assert_eq!(
            fields(&mut l),
            [serde_json::json!([
                "address", "markets", "name", "priority"
            ])]
        );
        assert!(l.take_events().is_empty());
    }

    #[test]
    fn edits_name_only_what_changed_once_per_write() {
        let mut l = location(vec![market(1)]).unwrap();
        l.take_events();
        l.set_markets(vec![market(1)]).unwrap();
        l.set_priority(0);
        assert!(fields(&mut l).is_empty(), "nothing changed");
        l.set_markets(vec![market(2)]).unwrap();
        l.set_name("Split".to_string()).unwrap();
        assert_eq!(fields(&mut l), [serde_json::json!(["markets", "name"])]);
    }

    #[test]
    fn a_restored_location_has_no_events() {
        let l = location(vec![market(1)]).unwrap();
        let mut restored = Location::restore(l.snapshot()).unwrap();
        assert!(restored.take_events().is_empty());
    }

    #[test]
    fn restore_keeps_the_version() {
        let mut l = location(vec![market(1)]).unwrap();
        l.set_version(Version::new(3));
        assert_eq!(Location::restore(l.snapshot()), Ok(l));
    }
}
