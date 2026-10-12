use std::{collections::HashMap, sync::Mutex};

use hypha_core::{CatalogId, RepoError};
use hypha_markets::{Catalog, CatalogContext, CatalogRepo};

#[derive(Default)]
pub struct InMemoryCatalogRepo {
    catalogs: Mutex<HashMap<CatalogId, Catalog>>,
}

impl CatalogRepo for InMemoryCatalogRepo {
    async fn get(&self, id: CatalogId) -> Result<Option<Catalog>, RepoError> {
        let catalogs = self.catalogs.lock().unwrap();
        Ok(catalogs.get(&id).cloned())
    }

    async fn get_by_context(&self, context: CatalogContext) -> Result<Option<Catalog>, RepoError> {
        let catalogs = self.catalogs.lock().unwrap();
        Ok(catalogs.values().find(|c| c.context() == context).cloned())
    }

    async fn save(&self, catalog: &Catalog) -> Result<(), RepoError> {
        let mut catalogs = self.catalogs.lock().unwrap();
        catalogs.insert(catalog.id(), catalog.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use hypha_core::MarketId;
    use uuid::Uuid;

    use super::*;

    fn in_market(n: u128) -> CatalogContext {
        CatalogContext::Market(MarketId::from_uuid(Uuid::from_u128(n)))
    }

    fn catalog(n: u128, context: CatalogContext) -> Catalog {
        Catalog::new(
            CatalogId::from_uuid(Uuid::from_u128(n)),
            context,
            HashSet::new(),
            None,
        )
    }

    #[test]
    fn saves_and_loads_catalogs() {
        let repo = InMemoryCatalogRepo::default();
        let c = catalog(1, in_market(1));
        pollster::block_on(repo.save(&c)).unwrap();
        assert_eq!(pollster::block_on(repo.get(c.id())), Ok(Some(c)));
    }

    #[test]
    fn finds_catalog_by_context() {
        let repo = InMemoryCatalogRepo::default();
        pollster::block_on(repo.save(&catalog(1, in_market(1)))).unwrap();
        pollster::block_on(repo.save(&catalog(2, in_market(2)))).unwrap();
        assert_eq!(
            pollster::block_on(repo.get_by_context(in_market(2))),
            Ok(Some(catalog(2, in_market(2))))
        );
        assert_eq!(
            pollster::block_on(repo.get_by_context(in_market(3))),
            Ok(None)
        );
    }
}
