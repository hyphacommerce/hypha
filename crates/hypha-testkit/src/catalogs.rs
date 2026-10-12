use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

use hypha_core::{CatalogId, ProductId, RepoError};
use hypha_markets::{Catalog, CatalogContext, CatalogRepo};

#[derive(Default)]
pub struct InMemoryCatalogRepo {
    catalogs: Mutex<HashMap<CatalogId, Catalog>>,
    /// Catalog membership, stored apart from the catalog like a separate table would be.
    products: Mutex<HashMap<CatalogId, HashSet<ProductId>>>,
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

    async fn add_products(
        &self,
        catalog: CatalogId,
        products: &[ProductId],
    ) -> Result<(), RepoError> {
        let mut all = self.products.lock().unwrap();
        all.entry(catalog)
            .or_default()
            .extend(products.iter().copied());
        Ok(())
    }

    async fn remove_products(
        &self,
        catalog: CatalogId,
        products: &[ProductId],
    ) -> Result<(), RepoError> {
        let mut all = self.products.lock().unwrap();
        if let Some(included) = all.get_mut(&catalog) {
            for product in products {
                included.remove(product);
            }
        }
        Ok(())
    }

    async fn included_products(
        &self,
        catalog: CatalogId,
        products: &[ProductId],
    ) -> Result<HashSet<ProductId>, RepoError> {
        let all = self.products.lock().unwrap();
        let Some(included) = all.get(&catalog) else {
            return Ok(HashSet::new());
        };
        Ok(products
            .iter()
            .copied()
            .filter(|p| included.contains(p))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::MarketId;
    use pollster::block_on;
    use uuid::Uuid;

    use super::*;

    fn in_market(n: u128) -> CatalogContext {
        CatalogContext::Market(MarketId::from_uuid(Uuid::from_u128(n)))
    }

    fn catalog(n: u128, context: CatalogContext) -> Catalog {
        Catalog::new(CatalogId::from_uuid(Uuid::from_u128(n)), context, None)
    }

    fn pid(n: u128) -> ProductId {
        ProductId::from_uuid(Uuid::from_u128(n))
    }

    #[test]
    fn saves_and_loads_catalogs() {
        let repo = InMemoryCatalogRepo::default();
        let c = catalog(1, in_market(1));
        block_on(repo.save(&c)).unwrap();
        assert_eq!(block_on(repo.get(c.id())), Ok(Some(c)));
    }

    #[test]
    fn finds_catalog_by_context() {
        let repo = InMemoryCatalogRepo::default();
        block_on(repo.save(&catalog(1, in_market(1)))).unwrap();
        block_on(repo.save(&catalog(2, in_market(2)))).unwrap();
        assert_eq!(
            block_on(repo.get_by_context(in_market(2))),
            Ok(Some(catalog(2, in_market(2))))
        );
        assert_eq!(block_on(repo.get_by_context(in_market(3))), Ok(None));
    }

    #[test]
    fn included_products_returns_only_members_that_were_asked_for() {
        let repo = InMemoryCatalogRepo::default();
        let id = CatalogId::from_uuid(Uuid::from_u128(1));
        block_on(repo.add_products(id, &[pid(1), pid(2), pid(3)])).unwrap();

        let included = block_on(repo.included_products(id, &[pid(2), pid(3), pid(9)])).unwrap();

        assert_eq!(included, HashSet::from([pid(2), pid(3)]));
    }

    #[test]
    fn add_products_ignores_duplicates_and_remove_ignores_missing() {
        let repo = InMemoryCatalogRepo::default();
        let id = CatalogId::from_uuid(Uuid::from_u128(1));
        block_on(repo.add_products(id, &[pid(1)])).unwrap();
        block_on(repo.add_products(id, &[pid(1), pid(2)])).unwrap();
        block_on(repo.remove_products(id, &[pid(1), pid(9)])).unwrap();

        let included = block_on(repo.included_products(id, &[pid(1), pid(2)])).unwrap();

        assert_eq!(included, HashSet::from([pid(2)]));
    }

    #[test]
    fn unknown_catalog_includes_nothing() {
        let repo = InMemoryCatalogRepo::default();
        let id = CatalogId::from_uuid(Uuid::from_u128(1));
        assert_eq!(
            block_on(repo.included_products(id, &[pid(1)])),
            Ok(HashSet::new())
        );
    }
}
