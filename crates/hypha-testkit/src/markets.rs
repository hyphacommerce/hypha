use std::{collections::HashMap, sync::Mutex};

use hypha_core::{MarketId, RepoError};
use hypha_markets::{Market, MarketRepo};

#[derive(Default)]
pub struct InMemoryMarketRepo {
    markets: Mutex<HashMap<MarketId, Market>>,
}

impl MarketRepo for InMemoryMarketRepo {
    async fn get(&self, id: MarketId) -> Result<Option<Market>, RepoError> {
        let markets = self.markets.lock().unwrap();
        Ok(markets.get(&id).cloned())
    }

    async fn list_all(&self) -> Result<Vec<Market>, RepoError> {
        let markets = self.markets.lock().unwrap();
        Ok(markets.values().cloned().collect())
    }

    async fn save(&self, market: &Market) -> Result<(), RepoError> {
        let mut markets = self.markets.lock().unwrap();
        markets.insert(market.id(), market.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{CountryCode, Currency};
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
    fn saves_and_loads_markets() {
        let repo = InMemoryMarketRepo::default();
        let m = market(1, "RS");
        pollster::block_on(repo.save(&m)).unwrap();
        assert_eq!(pollster::block_on(repo.get(m.id())), Ok(Some(m)))
    }

    #[test]
    fn get_unknown_is_none() {
        let repo = InMemoryMarketRepo::default();
        let id = MarketId::from_uuid(Uuid::from_u128(99));
        assert_eq!(pollster::block_on(repo.get(id)), Ok(None));
    }

    #[test]
    fn list_all_returns_every_market() {
        let repo = InMemoryMarketRepo::default();
        pollster::block_on(repo.save(&market(1, "RS"))).unwrap();
        pollster::block_on(repo.save(&market(2, "HR"))).unwrap();
        assert_eq!(pollster::block_on(repo.list_all()).unwrap().len(), 2);
    }
}
