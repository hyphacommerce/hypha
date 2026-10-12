use std::{collections::HashMap, sync::Mutex};

use hypha_core::{PriceListId, RepoError};
use hypha_markets::{PriceList, PriceListRepo};

#[derive(Default)]
pub struct InMemoryPriceListRepo {
    price_lists: Mutex<HashMap<PriceListId, PriceList>>,
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
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use hypha_core::{BasisPoints, Currency};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn saves_and_loads_price_lists() {
        let repo = InMemoryPriceListRepo::default();
        let list = PriceList::new(
            PriceListId::from_uuid(Uuid::from_u128(1)),
            Currency::Eur,
            BasisPoints::ZERO,
            HashMap::new(),
        )
        .unwrap();
        pollster::block_on(repo.save(&list)).unwrap();
        assert_eq!(pollster::block_on(repo.get(list.id())), Ok(Some(list)));
    }

    #[test]
    fn get_unknown_is_none() {
        let repo = InMemoryPriceListRepo::default();
        let id = PriceListId::from_uuid(Uuid::from_u128(99));
        assert_eq!(pollster::block_on(repo.get(id)), Ok(None));
    }
}
