use std::{collections::HashMap, sync::Mutex};

use hypha_core::{Price, PriceListId, RepoError, VariantId};
use hypha_markets::{PriceList, PriceListRepo};

#[derive(Default)]
pub struct InMemoryPriceListRepo {
    price_lists: Mutex<HashMap<PriceListId, PriceList>>,
    /// Fixed prices, stored apart from the list like a separate table would be.
    fixed_prices: Mutex<HashMap<PriceListId, HashMap<VariantId, Price>>>,
}

impl PriceListRepo for InMemoryPriceListRepo {
    async fn get(&self, id: PriceListId) -> Result<Option<PriceList>, RepoError> {
        let price_lists = self.price_lists.lock().unwrap();
        Ok(price_lists.get(&id).cloned())
    }

    async fn save(&self, price_list: &PriceList) -> Result<(), RepoError> {
        let mut price_lists = self.price_lists.lock().unwrap();
        price_lists.insert(price_list.id(), price_list.clone());
        Ok(())
    }

    async fn set_fixed_prices(
        &self,
        list: PriceListId,
        prices: &[(VariantId, Price)],
    ) -> Result<(), RepoError> {
        let mut all = self.fixed_prices.lock().unwrap();
        all.entry(list).or_default().extend(prices.iter().copied());
        Ok(())
    }

    async fn remove_fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<(), RepoError> {
        let mut all = self.fixed_prices.lock().unwrap();
        if let Some(prices) = all.get_mut(&list) {
            for variant in variants {
                prices.remove(variant);
            }
        }
        Ok(())
    }

    async fn fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<HashMap<VariantId, Price>, RepoError> {
        let all = self.fixed_prices.lock().unwrap();
        let Some(prices) = all.get(&list) else {
            return Ok(HashMap::new());
        };
        Ok(variants
            .iter()
            .filter_map(|v| prices.get(v).map(|p| (*v, *p)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{BasisPoints, Currency, Money};
    use pollster::block_on;
    use uuid::Uuid;

    use super::*;

    fn list_id() -> PriceListId {
        PriceListId::from_uuid(Uuid::from_u128(1))
    }

    fn vid(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn eur(amount: i64) -> Price {
        Price::new(Money::new(amount, Currency::Eur).unwrap()).unwrap()
    }

    #[test]
    fn saves_and_loads_price_lists() {
        let repo = InMemoryPriceListRepo::default();
        let list = PriceList::new(list_id(), Currency::Eur, BasisPoints::ZERO);
        block_on(repo.save(&list)).unwrap();
        assert_eq!(block_on(repo.get(list.id())), Ok(Some(list)));
    }

    #[test]
    fn get_unknown_is_none() {
        let repo = InMemoryPriceListRepo::default();
        let id = PriceListId::from_uuid(Uuid::from_u128(99));
        assert_eq!(block_on(repo.get(id)), Ok(None));
    }

    #[test]
    fn fixed_prices_returns_only_requested_variants_that_have_one() {
        let repo = InMemoryPriceListRepo::default();
        block_on(repo.set_fixed_prices(list_id(), &[(vid(1), eur(100)), (vid(2), eur(200))]))
            .unwrap();

        let found = block_on(repo.fixed_prices(list_id(), &[vid(2), vid(9)])).unwrap();

        assert_eq!(found, HashMap::from([(vid(2), eur(200))]));
    }

    #[test]
    fn set_replaces_and_remove_deletes() {
        let repo = InMemoryPriceListRepo::default();
        block_on(repo.set_fixed_prices(list_id(), &[(vid(1), eur(100)), (vid(2), eur(200))]))
            .unwrap();
        block_on(repo.set_fixed_prices(list_id(), &[(vid(1), eur(150))])).unwrap();
        block_on(repo.remove_fixed_prices(list_id(), &[vid(2), vid(9)])).unwrap();

        let found = block_on(repo.fixed_prices(list_id(), &[vid(1), vid(2)])).unwrap();

        assert_eq!(found, HashMap::from([(vid(1), eur(150))]));
    }
}
