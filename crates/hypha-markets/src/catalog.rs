use std::{collections::HashSet, fmt};

use hypha_core::{CatalogId, MarketId, PriceListId, ProductId};

use crate::error::CatalogError;

/// Where a catalog applies. An enum so other contexts (e.g. B2B companies) can be added later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatalogContext {
    Market(MarketId),
}

impl fmt::Display for CatalogContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogContext::Market(id) => write!(f, "market {id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    id: CatalogId,
    context: CatalogContext,
    products: HashSet<ProductId>,
    price_list: Option<PriceListId>,
}

impl Catalog {
    pub fn new(
        id: CatalogId,
        context: CatalogContext,
        products: HashSet<ProductId>,
        price_list: Option<PriceListId>,
    ) -> Self {
        Self {
            id,
            context,
            products,
            price_list,
        }
    }

    /// Returns `false` if the product was already in the catalog.
    pub fn add_product(&mut self, product: ProductId) -> bool {
        self.products.insert(product)
    }

    /// Returns `false` if the product wasn't in the catalog.
    pub fn remove_product(&mut self, product: ProductId) -> bool {
        self.products.remove(&product)
    }

    pub fn includes(&self, product: ProductId) -> bool {
        self.products.contains(&product)
    }

    pub fn set_price_list(&mut self, price_list: Option<PriceListId>) {
        self.price_list = price_list;
    }

    pub fn id(&self) -> CatalogId {
        self.id
    }

    pub fn context(&self) -> CatalogContext {
        self.context
    }

    pub fn products(&self) -> &HashSet<ProductId> {
        &self.products
    }

    pub fn price_list(&self) -> Option<PriceListId> {
        self.price_list
    }
}

pub fn catalog_for(catalogs: &[Catalog], context: CatalogContext) -> Option<&Catalog> {
    catalogs.iter().find(|c| c.context() == context)
}

/// For now each context has at most one catalog, so resolution is unambiguous.
pub fn check_one_catalog_per_context(catalogs: &[Catalog]) -> Result<(), CatalogError> {
    let mut seen = HashSet::new();
    for catalog in catalogs {
        if !seen.insert(catalog.context()) {
            return Err(CatalogError::DuplicateContext(catalog.context()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn pid(n: u128) -> ProductId {
        ProductId::from_uuid(Uuid::from_u128(n))
    }

    fn in_market(n: u128) -> CatalogContext {
        CatalogContext::Market(MarketId::from_uuid(Uuid::from_u128(n)))
    }

    fn catalog(n: u128, context: CatalogContext) -> Catalog {
        Catalog::new(
            CatalogId::from_uuid(Uuid::from_u128(n)),
            context,
            HashSet::from([pid(1)]),
            None,
        )
    }

    #[test]
    fn includes_only_listed_products() {
        let c = catalog(1, in_market(1));
        assert!(c.includes(pid(1)));
        assert!(!c.includes(pid(2)));
    }

    #[test]
    fn add_product_reports_whether_it_was_new() {
        let mut c = catalog(1, in_market(1));
        assert!(c.add_product(pid(2)));
        assert!(!c.add_product(pid(2)));
        assert!(c.includes(pid(2)));
    }

    #[test]
    fn remove_product_reports_whether_it_was_there() {
        let mut c = catalog(1, in_market(1));
        assert!(c.remove_product(pid(1)));
        assert!(!c.remove_product(pid(1)));
        assert!(!c.includes(pid(1)));
    }

    #[test]
    fn set_price_list_replaces_and_clears_it() {
        let mut c = catalog(1, in_market(1));
        let pl = PriceListId::from_uuid(Uuid::from_u128(9));
        c.set_price_list(Some(pl));
        assert_eq!(c.price_list(), Some(pl));
        c.set_price_list(None);
        assert_eq!(c.price_list(), None);
    }

    #[test]
    fn catalog_for_finds_catalog_by_context() {
        let catalogs = [catalog(1, in_market(1)), catalog(2, in_market(2))];
        assert_eq!(
            catalog_for(&catalogs, in_market(2)).map(|c| c.id()),
            Some(CatalogId::from_uuid(Uuid::from_u128(2)))
        );
        assert!(catalog_for(&catalogs, in_market(3)).is_none());
    }

    #[test]
    fn one_catalog_per_context_passes_for_distinct_contexts() {
        let catalogs = [catalog(1, in_market(1)), catalog(2, in_market(2))];
        assert_eq!(check_one_catalog_per_context(&catalogs), Ok(()));
    }

    #[test]
    fn one_catalog_per_context_rejects_shared_context() {
        let catalogs = [catalog(1, in_market(1)), catalog(2, in_market(1))];
        assert_eq!(
            check_one_catalog_per_context(&catalogs),
            Err(CatalogError::DuplicateContext(in_market(1)))
        );
    }
}
