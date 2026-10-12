use std::collections::{HashMap, HashSet};

use hypha_core::{CatalogId, MarketId, Price, PriceListId, ProductId, RepoError, VariantId};

use crate::{
    catalog::{Catalog, CatalogContext},
    market::Market,
    price_list::PriceList,
};

#[allow(async_fn_in_trait)]
pub trait MarketRepo {
    async fn get(&self, id: MarketId) -> Result<Option<Market>, RepoError>;
    async fn save(&self, market: &Market) -> Result<(), RepoError>;
    async fn list_all(&self) -> Result<Vec<Market>, RepoError>;
}

/// Catalogs, plus the products each one includes.
///
/// Membership is read and written in batches (slices in, sets out), never by loading
/// every product of a catalog: one query whether a cart has 1 line or 50, which keeps
/// within per-request query limits on hosts like Cloudflare D1.
#[allow(async_fn_in_trait)]
pub trait CatalogRepo {
    async fn get(&self, id: CatalogId) -> Result<Option<Catalog>, RepoError>;
    /// At most one catalog per context, see `check_one_catalog_per_context`.
    async fn get_by_context(&self, context: CatalogContext) -> Result<Option<Catalog>, RepoError>;
    async fn save(&self, catalog: &Catalog) -> Result<(), RepoError>;

    /// Adds products to the catalog. Products already in it are ignored.
    async fn add_products(
        &self,
        catalog: CatalogId,
        products: &[ProductId],
    ) -> Result<(), RepoError>;
    /// Removes products from the catalog. Products not in it are ignored.
    async fn remove_products(
        &self,
        catalog: CatalogId,
        products: &[ProductId],
    ) -> Result<(), RepoError>;
    /// Which of `products` the catalog includes.
    async fn included_products(
        &self,
        catalog: CatalogId,
        products: &[ProductId],
    ) -> Result<HashSet<ProductId>, RepoError>;
}

/// Price lists, plus their fixed per-variant prices.
///
/// Fixed prices are batched like catalog membership. Callers validate them with
/// `PriceList::check_fixed_prices` before `set_fixed_prices`; the repo only stores.
#[allow(async_fn_in_trait)]
pub trait PriceListRepo {
    async fn get(&self, id: PriceListId) -> Result<Option<PriceList>, RepoError>;
    async fn save(&self, price_list: &PriceList) -> Result<(), RepoError>;

    /// Inserts or replaces fixed prices for the given variants.
    async fn set_fixed_prices(
        &self,
        list: PriceListId,
        prices: &[(VariantId, Price)],
    ) -> Result<(), RepoError>;
    /// Removes fixed prices. Variants without one are ignored.
    async fn remove_fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<(), RepoError>;
    /// The fixed prices of those of `variants` that have one.
    async fn fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<HashMap<VariantId, Price>, RepoError>;
}
