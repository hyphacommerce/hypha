use hypha_core::{
    CountryCode, MarketId, Recorded, RepoError, Versioned, stamp_insert, stamp_update,
};
use hypha_markets::{Market, MarketRepo, repo::keys};

use crate::{InMemoryOutbox, table::Table};

#[derive(Default)]
pub struct InMemoryMarketRepo {
    pub(crate) markets: std::sync::Arc<Table<MarketId, Market>>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryMarketRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The events this repo wrote (into its outbox, shared or not).
    pub fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }
}

/// A country belongs to at most one market.
fn unique(new: &Market, other: &Market) -> Option<&'static str> {
    new.countries()
        .iter()
        .any(|c| other.covers(*c))
        .then_some(keys::COUNTRY)
}

impl InMemoryMarketRepo {
    /// The next `n` lookups by a unique key (and `list_all`, where there is one) find
    /// nothing, as if another request inserted the row right after: for testing a
    /// command that loses the race between its pre-check and its write.
    pub fn hide_next_lookups(&self, n: usize) {
        self.markets.hide_next_lookups(n);
    }
}

impl MarketRepo for InMemoryMarketRepo {
    async fn get(&self, id: MarketId) -> Result<Option<Market>, RepoError> {
        Ok(self.markets.get(&id))
    }

    async fn get_by_country(&self, country: CountryCode) -> Result<Option<Market>, RepoError> {
        Ok(self.markets.find(|m| m.covers(country)))
    }

    async fn list_all(&self) -> Result<Vec<Market>, RepoError> {
        Ok(self.markets.all_oldest_first())
    }

    async fn insert(&self, market: &mut Market) -> Result<(), RepoError> {
        stamp_insert(market, self.outbox.now());
        let events = market.take_events();
        self.markets
            .insert(market.id(), market, keys::MARKET_ID, &unique)?;
        self.outbox.write(events, Some(market.version()));
        Ok(())
    }

    async fn update(&self, market: &mut Market) -> Result<(), RepoError> {
        stamp_update(market, self.outbox.now());
        let events = market.take_events();
        self.markets.update(market.id(), market, &unique)?;
        self.outbox.write(events, Some(market.version()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{CountryCode, Currency, Version, Versioned};
    use pollster::block_on;
    use uuid::Uuid;

    use super::*;

    fn market(n: u128, country: &str) -> Market {
        Market::new(
            MarketId::from_uuid(Uuid::from_u128(n)),
            "Test".to_string(),
            vec![CountryCode::new(country).unwrap()],
            Currency::EUR,
        )
        .unwrap()
    }

    #[test]
    fn insert_stores_market_at_version_1() {
        let repo = InMemoryMarketRepo::default();
        let mut m = market(1, "RS");
        block_on(repo.insert(&mut m)).unwrap();
        assert_eq!(m.version(), Version::new(1));
        assert_eq!(block_on(repo.get(m.id())), Ok(Some(m)))
    }

    #[test]
    fn get_unknown_is_none() {
        let repo = InMemoryMarketRepo::default();
        let id = MarketId::from_uuid(Uuid::from_u128(99));
        assert_eq!(block_on(repo.get(id)), Ok(None));
    }

    #[test]
    fn list_all_returns_every_market() {
        let repo = InMemoryMarketRepo::default();
        block_on(repo.insert(&mut market(1, "RS"))).unwrap();
        block_on(repo.insert(&mut market(2, "HR"))).unwrap();
        assert_eq!(block_on(repo.list_all()).unwrap().len(), 2);
    }

    #[test]
    fn insert_rejects_country_of_another_market() {
        let repo = InMemoryMarketRepo::default();
        block_on(repo.insert(&mut market(1, "RS"))).unwrap();
        assert_eq!(
            block_on(repo.insert(&mut market(2, "RS"))),
            Err(RepoError::Duplicate(keys::COUNTRY))
        );
    }

    #[test]
    fn update_rejects_taking_another_markets_country() {
        let repo = InMemoryMarketRepo::default();
        block_on(repo.insert(&mut market(1, "RS"))).unwrap();
        let mut hr = market(2, "HR");
        block_on(repo.insert(&mut hr)).unwrap();

        hr.add_country(CountryCode::new("RS").unwrap()).unwrap();

        assert_eq!(
            block_on(repo.update(&mut hr)),
            Err(RepoError::Duplicate(keys::COUNTRY))
        );
    }

    #[test]
    fn update_from_stale_copy_fails() {
        let repo = InMemoryMarketRepo::default();
        let mut m = market(1, "RS");
        block_on(repo.insert(&mut m)).unwrap();
        let mut stale = m.clone();
        block_on(repo.update(&mut m)).unwrap();

        assert_eq!(
            block_on(repo.update(&mut stale)),
            Err(RepoError::StaleVersion)
        );
    }
}
