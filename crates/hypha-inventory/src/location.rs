use std::collections::HashSet;

use hypha_core::{LocationId, MarketId, Version, Versioned, required_text};

use crate::InventoryError;

/// A place that holds stock: a warehouse, a store, a 3PL.
///
/// Its stock counts for the markets it lists (Shopify: a location's stock shows only in
/// the regions it ships to). A location with no markets still holds stock, it just
/// doesn't sell anywhere. When several locations could fill a line, the lowest
/// `priority` goes first (design note 0004, I9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    id: LocationId,
    version: Version,
    name: String,
    markets: Vec<MarketId>,
    priority: u32,
}

impl Versioned for Location {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
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
            name: String::new(),
            markets: Vec::new(),
            priority,
        };
        location.set_name(name)?;
        location.set_markets(markets)?;
        Ok(location)
    }

    pub fn set_name(&mut self, name: String) -> Result<(), InventoryError> {
        self.name = required_text("name", &name)?;
        Ok(())
    }

    /// Replaces the markets this location sells to.
    pub fn set_markets(&mut self, markets: Vec<MarketId>) -> Result<(), InventoryError> {
        let mut seen = HashSet::new();
        if let Some(duplicate) = markets.iter().find(|m| !seen.insert(**m)) {
            return Err(InventoryError::DuplicateMarket(*duplicate));
        }
        self.markets = markets;
        Ok(())
    }

    pub fn set_priority(&mut self, priority: u32) {
        self.priority = priority;
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
}

/// Plain-data form of a `Location` for storage adapters (design note 0001, 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationSnapshot {
    pub id: LocationId,
    pub version: Version,
    pub name: String,
    pub markets: Vec<MarketId>,
    pub priority: u32,
}

impl Location {
    pub fn snapshot(&self) -> LocationSnapshot {
        LocationSnapshot {
            id: self.id,
            version: self.version,
            name: self.name.clone(),
            markets: self.markets.clone(),
            priority: self.priority,
        }
    }

    /// Rebuilds a location from storage, with the same checks as `new`.
    pub fn restore(s: LocationSnapshot) -> Result<Location, InventoryError> {
        let mut location = Location::new(s.id, s.name, s.markets, s.priority)?;
        location.version = s.version;
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

    #[test]
    fn restore_keeps_the_version() {
        let mut l = location(vec![market(1)]).unwrap();
        l.set_version(Version::new(3));
        assert_eq!(Location::restore(l.snapshot()), Ok(l));
    }
}
