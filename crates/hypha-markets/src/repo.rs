use std::collections::{HashMap, HashSet};

use hypha_core::{
    CatalogId, CountryCode, MarketId, Page, PageRequest, Price, PriceListId, ProductId, RepoError,
    VariantId,
};

use crate::{
    catalog::{Catalog, CatalogContext},
    market::Market,
    price_list::PriceList,
};

/// Uniqueness rules the markets repos enforce, reported as `RepoError::Duplicate(key)`.
/// Adapters map their constraint names to these.
pub mod keys {
    pub const MARKET_ID: &str = "market_id";
    /// A country belongs to at most one market.
    pub const COUNTRY: &str = "market_country";
    pub const CATALOG_ID: &str = "catalog_id";
    /// At most one catalog per context.
    pub const CATALOG_CONTEXT: &str = "catalog_context";
    pub const PRICE_LIST_ID: &str = "price_list_id";
}

#[allow(async_fn_in_trait)]
pub trait MarketRepo {
    async fn get(&self, id: MarketId) -> Result<Option<Market>, RepoError>;
    async fn get_by_country(&self, country: CountryCode) -> Result<Option<Market>, RepoError>;
    /// Stores a new market (version 1). Enforces `MARKET_ID` and `COUNTRY`.
    async fn insert(&self, market: &mut Market) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`. Enforces `COUNTRY`.
    async fn update(&self, market: &mut Market) -> Result<(), RepoError>;
    /// Every market, oldest first (by ID). A store has a handful.
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
    /// At most one catalog per context (`keys::CATALOG_CONTEXT`).
    async fn get_by_context(&self, context: CatalogContext) -> Result<Option<Catalog>, RepoError>;
    /// Every catalog, oldest first (by ID). A store has one per market.
    async fn list_all(&self) -> Result<Vec<Catalog>, RepoError>;
    /// Stores a new catalog (version 1). Enforces `CATALOG_ID` and `CATALOG_CONTEXT`.
    async fn insert(&self, catalog: &mut Catalog) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, catalog: &mut Catalog) -> Result<(), RepoError>;

    /// Adds products to the catalog. Products already in it are ignored. Records no
    /// event: commands stage it (`StageCatalogProducts`) with `catalog.products_added`;
    /// this is for setting up data.
    ///
    /// Precondition: the catalog exists (commands load it first). Otherwise adapters
    /// fail with `Backend` rather than store orphaned membership.
    async fn add_products(
        &self,
        catalog: CatalogId,
        products: &[ProductId],
    ) -> Result<(), RepoError>;
    /// Removes products from the catalog. Products not in it are ignored. Records no
    /// event, like `add_products`.
    async fn remove_products(
        &self,
        catalog: CatalogId,
        products: &[ProductId],
    ) -> Result<(), RepoError>;
    /// A page of the catalog's products, newest first by product ID. An unknown catalog has
    /// none.
    async fn product_page(
        &self,
        catalog: CatalogId,
        page: PageRequest<ProductId>,
    ) -> Result<Page<ProductId>, RepoError>;
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
    /// Every price list, oldest first (by ID). A store has a handful.
    async fn list_all(&self) -> Result<Vec<PriceList>, RepoError>;
    /// Stores a new price list (version 1). Enforces `PRICE_LIST_ID`.
    async fn insert(&self, price_list: &mut PriceList) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, price_list: &mut PriceList) -> Result<(), RepoError>;

    /// Inserts or replaces fixed prices for the given variants. Records no event:
    /// commands stage it (`StageFixedPrices`) with `price_list.prices_set`; this is for
    /// setting up data.
    ///
    /// Preconditions: the list exists and each variant appears once
    /// (`PriceList::check_fixed_prices` reports duplicates precisely). Otherwise adapters
    /// fail with `Backend` and store nothing.
    async fn set_fixed_prices(
        &self,
        list: PriceListId,
        prices: &[(VariantId, Price)],
    ) -> Result<(), RepoError>;
    /// Removes fixed prices. Variants without one are ignored. Records no event, like
    /// `set_fixed_prices`.
    async fn remove_fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<(), RepoError>;
    /// The price lists with a fixed price for `variant`, by ID: what to clean up when the
    /// variant is removed.
    async fn lists_pricing(&self, variant: VariantId) -> Result<Vec<PriceListId>, RepoError>;
    /// A page of the list's fixed prices, newest variant first (by variant ID). An
    /// unknown list has none.
    async fn fixed_price_page(
        &self,
        list: PriceListId,
        page: PageRequest<VariantId>,
    ) -> Result<Page<(VariantId, Price)>, RepoError>;
    /// The fixed prices of those of `variants` that have one.
    async fn fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<HashMap<VariantId, Price>, RepoError>;
}

/// Whatever holds the `MarketRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use (review plan D1); `hypha_cart::CartPorts` is
/// the union of all of them.
pub trait HasMarkets {
    type Markets: MarketRepo;
    fn markets(&self) -> &Self::Markets;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasMarkets + ?Sized> HasMarkets for &T {
    type Markets = T::Markets;
    fn markets(&self) -> &Self::Markets {
        (**self).markets()
    }
}

/// Whatever holds the `CatalogRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use (review plan D1); `hypha_cart::CartPorts` is
/// the union of all of them.
pub trait HasCatalogs {
    type Catalogs: CatalogRepo;
    fn catalogs(&self) -> &Self::Catalogs;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasCatalogs + ?Sized> HasCatalogs for &T {
    type Catalogs = T::Catalogs;
    fn catalogs(&self) -> &Self::Catalogs {
        (**self).catalogs()
    }
}

/// Whatever holds the `PriceListRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use (review plan D1); `hypha_cart::CartPorts` is
/// the union of all of them.
pub trait HasPriceLists {
    type PriceLists: PriceListRepo;
    fn price_lists(&self) -> &Self::PriceLists;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasPriceLists + ?Sized> HasPriceLists for &T {
    type PriceLists = T::PriceLists;
    fn price_lists(&self) -> &Self::PriceLists {
        (**self).price_lists()
    }
}

/// Catalog membership as a batch writes it, so the change and its event commit together.
/// Same preconditions as `CatalogRepo::add_products` and `remove_products`.
pub trait StageCatalogProducts {
    fn add_catalog_products(&mut self, catalog: CatalogId, products: &[ProductId]);
    fn remove_catalog_products(&mut self, catalog: CatalogId, products: &[ProductId]);
}

/// Fixed prices as a batch writes them. Same preconditions as
/// `PriceListRepo::set_fixed_prices` and `remove_fixed_prices`.
pub trait StageFixedPrices {
    fn set_fixed_prices(&mut self, list: PriceListId, prices: &[(VariantId, Price)]);
    fn remove_fixed_prices(&mut self, list: PriceListId, variants: &[VariantId]);
}
