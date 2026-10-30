use std::collections::{HashMap, HashSet};

use hypha_core::{
    CatalogId, CategoryId, CountryCode, Keyset, MarketId, Page, PageRequest, PriceListId,
    ProductId, RepoError, Timestamp, VariantId,
};

use crate::{
    catalog::{Catalog, CatalogContext},
    filter::{FixedPriceFilter, PriceListFilter, PriceListSort},
    market::Market,
    price_list::{FixedPrice, PriceList},
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
    /// A price list's name, ignoring case.
    pub const PRICE_LIST_NAME: &str = "price_list_name";
    /// A catalog's price lists exist, and one a catalog holds can't be deleted
    /// (`RepoError::Reference`).
    pub const CATALOG_PRICE_LIST: &str = "catalog_price_list";
    /// A list's audience names customer groups that exist, and a group an audience names
    /// can't be deleted (`RepoError::Reference`).
    pub const PRICE_LIST_GROUP: &str = "price_list_group";
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
    /// The catalogs holding `list`, oldest first (by ID).
    async fn holding_price_list(&self, list: PriceListId) -> Result<Vec<Catalog>, RepoError>;
    /// Stores a new catalog (version 1) with its price lists. Enforces `CATALOG_ID` and
    /// `CATALOG_CONTEXT`.
    ///
    /// Precondition: its price lists exist (commands load them first); adapters that can
    /// see them refuse others with `Reference(CATALOG_PRICE_LIST)`.
    async fn insert(&self, catalog: &mut Catalog) -> Result<(), RepoError>;
    /// Stores changes, its set of price lists included, and bumps the version, or fails
    /// with `StaleVersion`. The same precondition as `insert`.
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
    /// The lists of `ids` that exist, oldest first (by ID).
    async fn get_many(&self, ids: &[PriceListId]) -> Result<Vec<PriceList>, RepoError>;
    /// Every price list, oldest first (by ID).
    async fn list_all(&self) -> Result<Vec<PriceList>, RepoError>;
    /// The list named `name`, ignoring case (`keys::PRICE_LIST_NAME` allows one).
    async fn get_by_name(&self, name: &str) -> Result<Option<PriceList>, RepoError>;
    /// A page of the lists `filter` matches, in `sort`'s order. `page.after`'s key fits
    /// the sort (`PriceListSort::fits`; hosts check cursors).
    async fn list(
        &self,
        filter: &PriceListFilter,
        sort: PriceListSort,
        page: PageRequest<Keyset<PriceListId>>,
    ) -> Result<Page<PriceList>, RepoError>;
    /// Stores a new price list (version 1). Enforces `PRICE_LIST_ID` and `PRICE_LIST_NAME`;
    /// adapters that can see customer groups refuse an audience naming one that doesn't
    /// exist with `Reference(PRICE_LIST_GROUP)`.
    async fn insert(&self, price_list: &mut PriceList) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`. Enforces
    /// `PRICE_LIST_NAME` and the audience's groups as `insert` does.
    async fn update(&self, price_list: &mut PriceList) -> Result<(), RepoError>;
    /// Removes the list with its fixed prices, writing the events it recorded
    /// (`PriceList::delete`), when it's still at the version loaded: otherwise (changed,
    /// or gone already) `StaleVersion`, and no event. Refused with
    /// `Reference(CATALOG_PRICE_LIST)` while a catalog holds it, in the same step as the
    /// delete.
    async fn delete(&self, price_list: &mut PriceList) -> Result<(), RepoError>;

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
        prices: &[(VariantId, FixedPrice)],
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
    /// Up to `limit` of the variants `list` has a fixed price for, by ID: what it prices
    /// beside its adjustment.
    async fn priced_variants(
        &self,
        list: PriceListId,
        limit: u32,
    ) -> Result<Vec<VariantId>, RepoError>;
    /// A page of the list's fixed prices, newest variant first (by variant ID). An
    /// unknown list has none.
    /// `filter` joins the products and categories the variants belong to: a variant no
    /// product has matches only an empty filter.
    async fn fixed_price_page(
        &self,
        list: PriceListId,
        filter: &FixedPriceFilter,
        page: PageRequest<VariantId>,
    ) -> Result<Page<(VariantId, FixedPrice)>, RepoError>;
    /// How many fixed prices each of `lists` has; a list without any is left out.
    async fn fixed_price_counts(
        &self,
        lists: &[PriceListId],
    ) -> Result<HashMap<PriceListId, u64>, RepoError>;
    /// Each of `lists`' fixed prices for those of `variants` that have one, in one read: a
    /// list with none is left out.
    async fn fixed_prices_in(
        &self,
        lists: &[PriceListId],
        variants: &[VariantId],
    ) -> Result<HashMap<PriceListId, HashMap<VariantId, FixedPrice>>, RepoError>;
    /// Each of `products`' categories with all their ancestors, as the categories domain
    /// stores them: what an adjustment applying to categories reaches. A product in none
    /// is left out.
    async fn product_categories(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, HashSet<CategoryId>>, RepoError>;
    /// The fixed prices of those of `variants` that have one.
    async fn fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<HashMap<VariantId, FixedPrice>, RepoError>;
    /// How far the schedule job has announced window boundaries
    /// (`StagePriceListSchedule`); `None` before its first run.
    async fn schedule_ran_until(&self) -> Result<Option<Timestamp>, RepoError>;
}

/// Whatever holds the `MarketRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use; `hypha_cart::CartPorts` is
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
/// bind just the accessors they use; `hypha_cart::CartPorts` is
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
/// bind just the accessors they use; `hypha_cart::CartPorts` is
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
    fn set_fixed_prices(&mut self, list: PriceListId, prices: &[(VariantId, FixedPrice)]);
    fn remove_fixed_prices(&mut self, list: PriceListId, variants: &[VariantId]);
}

/// The schedule job's progress as a batch writes it, with the events it announced: every
/// boundary up to `to`, where the run found it at `from` (`None`: never ran). If another
/// run moved it since, the commit fails with `StaleVersion` and writes nothing.
pub trait StagePriceListSchedule {
    fn advance_price_list_schedule(&mut self, from: Option<Timestamp>, to: Timestamp);
}
