//! Loading a `PricingContext`: shared by everything that prices variants in a market
//! (carts, search documents), so they can't disagree.

use std::collections::{HashMap, HashSet};

use hypha_core::{MarketId, ProductId, RepoError, VariantId};
use hypha_markets::{
    CatalogContext, CatalogRepo, HasCatalogs, HasMarkets, HasPriceLists, Market, MarketRepo,
    PriceListRepo,
};

use crate::PricingContext;

/// Everything needed to price `variants` (of `products`) in `market`, with a fixed
/// number of queries however many there are: membership and fixed prices are fetched
/// in batches. `None` when the market doesn't exist.
///
/// A missing catalog or price list is left out rather than reported here:
/// `resolve_price` turns it into a precise reason per variant. Ask for every product
/// and variant you'll resolve: one not asked for reads as "not in the catalog" or "no
/// fixed price".
pub async fn load_pricing<S: HasMarkets + HasCatalogs + HasPriceLists>(
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
pub async fn load_pricing_for<S: HasCatalogs + HasPriceLists>(
    ports: &S,
    market: Market,
    products: &[ProductId],
    variants: &[VariantId],
) -> Result<PricingContext, RepoError> {
    let catalog = ports
        .catalogs()
        .get_by_context(CatalogContext::Market(market.id()))
        .await?;
    let price_list = match catalog.as_ref().and_then(|c| c.price_list()) {
        Some(id) => ports.price_lists().get(id).await?,
        None => None,
    };
    let included_products = match &catalog {
        Some(c) if !products.is_empty() => {
            ports.catalogs().included_products(c.id(), products).await?
        }
        _ => HashSet::new(),
    };
    let fixed_prices = match &price_list {
        Some(l) if !variants.is_empty() => {
            ports.price_lists().fixed_prices(l.id(), variants).await?
        }
        _ => HashMap::new(),
    };
    Ok(PricingContext {
        market,
        catalog,
        price_list,
        included_products,
        fixed_prices,
    })
}
