//! What happens to markets, catalogs and price lists, as events. Search and caches react to
//! them: which products a market sells, and at what price. Contract types: a reshaped field
//! is a new `VERSION`, never an edit.

use hypha_core::{CatalogId, CategoryId, Event, MarketId, PriceListId, ProductId, VariantId};
use serde::Serialize;

pub const MARKET: &str = "market";
pub const CATALOG: &str = "catalog";
pub const PRICE_LIST: &str = "price_list";

/// `market.changed`: what changed, among `created`, `name`, `countries` and
/// `prices_include_tax` (search rebuilds only for what documents depend on, so a rename
/// costs nothing).
#[derive(Debug, Serialize)]
pub struct MarketChanged {
    pub market: MarketId,
    pub fields: Vec<&'static str>,
}

impl Event for MarketChanged {
    const TYPE: &'static str = "market.changed";
    const VERSION: u16 = 1;
}

/// `catalog.changed`: created, or its price lists changed (with
/// `catalog.price_lists_added` / `_removed`, which name them).
#[derive(Debug, Serialize)]
pub struct CatalogChanged {
    pub catalog: CatalogId,
}

impl Event for CatalogChanged {
    const TYPE: &'static str = "catalog.changed";
    const VERSION: u16 = 1;
}

/// `catalog.price_lists_added`: these lists now compete to price the catalog's products.
#[derive(Debug, Serialize)]
pub struct CatalogPriceListsAdded {
    pub catalog: CatalogId,
    pub price_lists: Vec<PriceListId>,
}

impl Event for CatalogPriceListsAdded {
    const TYPE: &'static str = "catalog.price_lists_added";
    const VERSION: u16 = 1;
}

/// `catalog.price_lists_removed`.
#[derive(Debug, Serialize)]
pub struct CatalogPriceListsRemoved {
    pub catalog: CatalogId,
    pub price_lists: Vec<PriceListId>,
}

impl Event for CatalogPriceListsRemoved {
    const TYPE: &'static str = "catalog.price_lists_removed";
    const VERSION: u16 = 1;
}

/// `catalog.products_added`: these products are sold in the catalog's context now.
#[derive(Debug, Serialize)]
pub struct CatalogProductsAdded {
    pub catalog: CatalogId,
    pub products: Vec<ProductId>,
}

impl Event for CatalogProductsAdded {
    const TYPE: &'static str = "catalog.products_added";
    const VERSION: u16 = 1;
}

/// `catalog.products_removed`.
#[derive(Debug, Serialize)]
pub struct CatalogProductsRemoved {
    pub catalog: CatalogId,
    pub products: Vec<ProductId>,
}

impl Event for CatalogProductsRemoved {
    const TYPE: &'static str = "catalog.products_removed";
    const VERSION: u16 = 1;
}

/// `price_list.created`: a new list, on no catalog yet.
#[derive(Debug, Serialize)]
pub struct PriceListCreated {
    pub price_list: PriceListId,
}

impl Event for PriceListCreated {
    const TYPE: &'static str = "price_list.created";
    const VERSION: u16 = 1;
}

/// `price_list.deleted`: gone with its fixed prices; no catalog held it.
#[derive(Debug, Serialize)]
pub struct PriceListDeleted {
    pub price_list: PriceListId,
}

impl Event for PriceListDeleted {
    const TYPE: &'static str = "price_list.deleted";
    const VERSION: u16 = 1;
}

/// `price_list.changed`: what changed, among `name`, `description`, `kind`, `status`,
/// `starts_at`, `ends_at`, `compare_at`, `adjustment` and `audience`. Events written before `fields`
/// existed were for a creation or a new adjustment.
///
/// When the adjustment's reach changed, `previous_applies_to` is what it reached before
/// (absent when it reached the same, or there was no adjustment): what was priced by it
/// and may not be any more.
#[derive(Debug, Serialize)]
pub struct PriceListChanged {
    pub price_list: PriceListId,
    pub fields: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_applies_to: Option<AppliesToPayload>,
}

/// What an adjustment reaches, as events carry it: `{"type": "all"}`,
/// `{"type": "products", "products": [...]}` or `{"type": "categories", "categories": [...]}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppliesToPayload {
    All,
    Products { products: Vec<ProductId> },
    Categories { categories: Vec<CategoryId> },
}

impl From<&crate::AppliesTo> for AppliesToPayload {
    fn from(a: &crate::AppliesTo) -> Self {
        match a {
            crate::AppliesTo::All => AppliesToPayload::All,
            crate::AppliesTo::Products(products) => AppliesToPayload::Products {
                products: products.clone(),
            },
            crate::AppliesTo::Categories(categories) => AppliesToPayload::Categories {
                categories: categories.clone(),
            },
        }
    }
}

impl Event for PriceListChanged {
    const TYPE: &'static str = "price_list.changed";
    const VERSION: u16 = 1;
}

/// `price_list.prices_set`: fixed prices set or replaced for these variants.
#[derive(Debug, Serialize)]
pub struct PricesSet {
    pub price_list: PriceListId,
    pub variants: Vec<VariantId>,
}

impl Event for PricesSet {
    const TYPE: &'static str = "price_list.prices_set";
    const VERSION: u16 = 1;
}

/// `price_list.prices_removed`: these variants follow the adjustment again.
#[derive(Debug, Serialize)]
pub struct PricesRemoved {
    pub price_list: PriceListId,
    pub variants: Vec<VariantId>,
}

impl Event for PricesRemoved {
    const TYPE: &'static str = "price_list.prices_removed";
    const VERSION: u16 = 1;
}

/// `price_list.started`: an active list's window opened (`starts_at`, µs since the epoch),
/// so it prices the products of `catalogs` from then on. Recorded by the schedule job
/// within a tick of the moment; a list made active inside its window records
/// `price_list.changed` instead.
#[derive(Debug, Serialize)]
pub struct PriceListStarted {
    pub price_list: PriceListId,
    pub catalogs: Vec<CatalogId>,
    pub starts_at: i64,
}

impl Event for PriceListStarted {
    const TYPE: &'static str = "price_list.started";
    const VERSION: u16 = 1;
}

/// `price_list.ended`: an active list's window closed (`ends_at`, µs since the epoch), so
/// it prices nothing in `catalogs` any more.
#[derive(Debug, Serialize)]
pub struct PriceListEnded {
    pub price_list: PriceListId,
    pub catalogs: Vec<CatalogId>,
    pub ends_at: i64,
}

impl Event for PriceListEnded {
    const TYPE: &'static str = "price_list.ended";
    const VERSION: u16 = 1;
}
