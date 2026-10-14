use std::{collections::HashMap, sync::Mutex};

use hypha_core::{Price, PriceListId, RepoError, VariantId};
use hypha_markets::{PriceList, PriceListRepo, repo::keys};

use crate::table::{Table, missing, no_unique_keys};

#[derive(Default)]
pub struct InMemoryPriceListRepo {
    price_lists: Table<PriceListId, PriceList>,
    /// Fixed prices, stored apart from the list like a separate table would be.
    fixed_prices: Mutex<HashMap<PriceListId, HashMap<VariantId, Price>>>,
}

impl PriceListRepo for InMemoryPriceListRepo {
    async fn get(&self, id: PriceListId) -> Result<Option<PriceList>, RepoError> {
        Ok(self.price_lists.get(&id))
    }

    async fn insert(&self, price_list: &mut PriceList) -> Result<(), RepoError> {
        self.price_lists.insert(
            price_list.id(),
            price_list,
            keys::PRICE_LIST_ID,
            &no_unique_keys,
        )
    }

    async fn update(&self, price_list: &mut PriceList) -> Result<(), RepoError> {
        self.price_lists
            .update(price_list.id(), price_list, &no_unique_keys)
    }

    async fn set_fixed_prices(
        &self,
        list: PriceListId,
        prices: &[(VariantId, Price)],
    ) -> Result<(), RepoError> {
        if self.price_lists.get(&list).is_none() {
            return Err(missing("price list", list));
        }
        let mut seen = std::collections::HashSet::new();
        if let Some((variant, _)) = prices.iter().find(|(v, _)| !seen.insert(*v)) {
            // Like a database statement that would update the same row twice.
            return Err(RepoError::Backend(format!(
                "variant {variant} is listed twice; callers check_fixed_prices first"
            )));
        }
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

    /// A repo holding the price list `list_id()`.
    fn with_list() -> InMemoryPriceListRepo {
        let repo = InMemoryPriceListRepo::default();
        let mut list = PriceList::new(list_id(), Currency::Eur, BasisPoints::ZERO);
        block_on(repo.insert(&mut list)).unwrap();
        repo
    }

    #[test]
    fn saves_and_loads_price_lists() {
        let repo = InMemoryPriceListRepo::default();
        let mut list = PriceList::new(list_id(), Currency::Eur, BasisPoints::ZERO);
        block_on(repo.insert(&mut list)).unwrap();
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
        let repo = with_list();
        block_on(repo.set_fixed_prices(list_id(), &[(vid(1), eur(100)), (vid(2), eur(200))]))
            .unwrap();

        let found = block_on(repo.fixed_prices(list_id(), &[vid(2), vid(9)])).unwrap();

        assert_eq!(found, HashMap::from([(vid(2), eur(200))]));
    }

    #[test]
    fn set_replaces_and_remove_deletes() {
        let repo = with_list();
        block_on(repo.set_fixed_prices(list_id(), &[(vid(1), eur(100)), (vid(2), eur(200))]))
            .unwrap();
        block_on(repo.set_fixed_prices(list_id(), &[(vid(1), eur(150))])).unwrap();
        block_on(repo.remove_fixed_prices(list_id(), &[vid(2), vid(9)])).unwrap();

        let found = block_on(repo.fixed_prices(list_id(), &[vid(1), vid(2)])).unwrap();

        assert_eq!(found, HashMap::from([(vid(1), eur(150))]));
    }
}
