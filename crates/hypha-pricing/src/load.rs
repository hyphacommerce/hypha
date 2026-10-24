//! Loading a `PricingContext`: shared by everything that prices variants in a market
//! (carts, search documents), so they can't disagree.

use std::collections::{HashMap, HashSet};

use hypha_core::{HasClock, MarketId, PriceListId, ProductId, RepoError, VariantId};
use hypha_markets::{
    AppliesTo, CatalogContext, CatalogRepo, HasCatalogs, HasMarkets, HasPriceLists, Market,
    MarketRepo, PriceList, PriceListRepo,
};

use crate::PricingContext;

/// Everything needed to price `variants` (of `products`) in `market` now (the ports'
/// clock), with a fixed number of queries however many there are: membership and fixed
/// prices are fetched in batches. `None` when the market doesn't exist.
///
/// A missing catalog or price list is left out rather than reported here:
/// `resolve_price` turns it into a precise reason per variant. Ask for every product
/// and variant you'll resolve: one not asked for reads as "not in the catalog" or "no
/// fixed price".
pub async fn load_pricing<S: HasMarkets + HasCatalogs + HasPriceLists + HasClock>(
    ports: &S,
    market: MarketId,
    products: &[ProductId],
    variants: &[VariantId],
) -> Result<Option<PricingContext>, RepoError> {
    let Some(market) = ports.markets().get(market).await? else {
        return Ok(None);
    };
    load_pricing_for(ports, market, products, variants)
        .await
        .map(Some)
}

/// [`load_pricing`] for a market already loaded (by country, say), without reading it
/// again.
pub async fn load_pricing_for<S: HasCatalogs + HasPriceLists + HasClock>(
    ports: &S,
    market: Market,
    products: &[ProductId],
    variants: &[VariantId],
) -> Result<PricingContext, RepoError> {
    let at = ports.now();
    let catalog = ports
        .catalogs()
        .get_by_context(CatalogContext::Market(market.id()))
        .await?;
    let price_lists = match &catalog {
        Some(c) if !c.price_lists().is_empty() => {
            ports.price_lists().get_many(c.price_lists()).await?
        }
        _ => Vec::new(),
    };
    let included_products = match &catalog {
        Some(c) if !products.is_empty() => {
            ports.catalogs().included_products(c.id(), products).await?
        }
        _ => HashSet::new(),
    };
    // Only a live list's fixed prices can count.
    let live: Vec<&PriceList> = price_lists.iter().filter(|l| l.is_live(at)).collect();
    let live_ids: Vec<PriceListId> = live.iter().map(|l| l.id()).collect();
    let fixed_prices = if live_ids.is_empty() || variants.is_empty() {
        HashMap::new()
    } else {
        ports
            .price_lists()
            .fixed_prices_in(&live_ids, variants)
            .await?
    };
    let by_category = live.iter().any(|l| {
        l.adjustment()
            .is_some_and(|a| matches!(a.applies_to(), AppliesTo::Categories(_)))
    });
    let product_categories = if by_category && !products.is_empty() {
        ports.price_lists().product_categories(products).await?
    } else {
        HashMap::new()
    };
    Ok(PricingContext {
        market,
        catalog,
        price_lists,
        included_products,
        fixed_prices,
        product_categories,
        at,
    })
}
