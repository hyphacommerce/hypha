//! What happens to markets, catalogs and price lists, as events (design note 0016,
//! section 4). Search and caches react to them: which products a market sells, and at
//! what price. Contract types: a reshaped field is a new `VERSION`, never an edit.

use hypha_core::{CatalogId, Event, MarketId, PriceListId, ProductId, VariantId};
use serde::Serialize;

pub const MARKET: &str = "market";
pub const CATALOG: &str = "catalog";
pub const PRICE_LIST: &str = "price_list";

/// `market.changed`: created, renamed, its countries or tax setting changed.
#[derive(Debug, Serialize)]
pub struct MarketChanged {
    pub market: MarketId,
}

impl Event for MarketChanged {
    const TYPE: &'static str = "market.changed";
    const VERSION: u16 = 1;
}

/// `catalog.changed`: created, or its price list assigned.
#[derive(Debug, Serialize)]
pub struct CatalogChanged {
    pub catalog: CatalogId,
}

impl Event for CatalogChanged {
    const TYPE: &'static str = "catalog.changed";
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

/// `price_list.changed`: created, or its adjustment changed (every price that follows
/// it changes).
#[derive(Debug, Serialize)]
pub struct PriceListChanged {
    pub price_list: PriceListId,
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
