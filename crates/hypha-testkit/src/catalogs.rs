use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

use hypha_core::{CatalogId, ProductId, RepoError};
use hypha_markets::{Catalog, CatalogContext, CatalogRepo, repo::keys};

use crate::table::Table;

#[derive(Default)]
pub struct InMemoryCatalogRepo {
    catalogs: Table<CatalogId, Catalog>,
    /// Catalog membership, stored apart from the catalog like a separate table would be.
    products: Mutex<HashMap<CatalogId, HashSet<ProductId>>>,
}

/// At most one catalog per context.
fn unique(new: &Catalog, other: &Catalog) -> Option<&'static str> {
    (new.context() == other.context()).then_some(keys::CATALOG_CONTEXT)
}

impl CatalogRepo for InMemoryCatalogRepo {
    async fn get(&self, id: CatalogId) -> Result<Option<Catalog>, RepoError> {
        Ok(self.catalogs.get(&id))
    }

    async fn get_by_context(&self, context: CatalogContext) -> Result<Option<Catalog>, RepoError> {
        Ok(self.catalogs.find(|c| c.context() == context))
    }

    async fn insert(&self, catalog: &mut Catalog) -> Result<(), RepoError> {
        self.catalogs
            .insert(catalog.id(), catalog, keys::CATALOG_ID, &unique)
    }

    async fn update(&self, catalog: &mut Catalog) -> Result<(), RepoError> {
        self.catalogs.update(catalog.id(), catalog, &unique)
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
        let mut c = catalog(1, in_market(1));
        block_on(repo.insert(&mut c)).unwrap();
        assert_eq!(block_on(repo.get(c.id())), Ok(Some(c)));
    }

    #[test]
    fn finds_catalog_by_context() {
        let repo = InMemoryCatalogRepo::default();
        block_on(repo.insert(&mut catalog(1, in_market(1)))).unwrap();
        let mut second = catalog(2, in_market(2));
        block_on(repo.insert(&mut second)).unwrap();
        assert_eq!(
            block_on(repo.get_by_context(in_market(2))),
            Ok(Some(second))
        );
        assert_eq!(block_on(repo.get_by_context(in_market(3))), Ok(None));
    }

    #[test]
    fn insert_rejects_second_catalog_for_context() {
        let repo = InMemoryCatalogRepo::default();
        block_on(repo.insert(&mut catalog(1, in_market(1)))).unwrap();
        assert_eq!(
            block_on(repo.insert(&mut catalog(2, in_market(1)))),
            Err(RepoError::Duplicate(keys::CATALOG_CONTEXT))
        );
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
