use hypha_core::{CountryCode, MarketId, RepoError};
use hypha_markets::{Market, MarketRepo, repo::keys};

use crate::table::Table;

#[derive(Default)]
pub struct InMemoryMarketRepo {
    markets: Table<MarketId, Market>,
}

/// A country belongs to at most one market.
fn unique(new: &Market, other: &Market) -> Option<&'static str> {
    new.countries()
        .iter()
        .any(|c| other.covers(*c))
        .then_some(keys::COUNTRY)
}

impl MarketRepo for InMemoryMarketRepo {
    async fn get(&self, id: MarketId) -> Result<Option<Market>, RepoError> {
        Ok(self.markets.get(&id))
    }

    async fn get_by_country(&self, country: CountryCode) -> Result<Option<Market>, RepoError> {
        Ok(self.markets.find(|m| m.covers(country)))
    }

    async fn list_all(&self) -> Result<Vec<Market>, RepoError> {
        Ok(self.markets.all())
    }

    async fn insert(&self, market: &mut Market) -> Result<(), RepoError> {
        self.markets
            .insert(market.id(), market, keys::MARKET_ID, &unique)
    }

    async fn update(&self, market: &mut Market) -> Result<(), RepoError> {
        self.markets.update(market.id(), market, &unique)
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
            Currency::Eur,
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
