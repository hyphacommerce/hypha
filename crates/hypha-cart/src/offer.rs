//! A product's prices and stock in a shopper's country: what a product page shows before
//! anything is in a cart. Priced with the cart's loaders and rules (`load_pricing`,
//! `price_line`, `StockContext::load`), so the page, search and the cart can't disagree.

use hypha_core::{CountryCode, Ctx, HasClock, MarketId, Quantity, Scope, VariantId};
use hypha_inventory::{Availability, HasInventoryItems, HasLocations, StockContext};
use hypha_markets::{HasCatalogs, HasMarkets, HasPriceLists, Market, MarketRepo, PriceList};
use hypha_pricing::{
    PriceExplanation, ResolvedPrice, VariantPrices, explain_price, load_pricing_for,
};
use hypha_products::Product;

use crate::{CommandError, price_line};

/// One variant's offer in a market.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantOffer {
    pub variant: VariantId,
    /// The price of one, with what it's compared against; `None` when the variant can't
    /// be priced there (its product isn't in the market's catalog, or isn't active).
    pub price: Option<ResolvedPrice>,
    /// Whether one can be bought now: in stock at a location serving the market, or not
    /// tracked. `false` whenever there's no price.
    pub available: bool,
    /// How many can be bought now; `None` without a limit (untracked, or selling past
    /// zero). A storefront caps its quantity at it.
    pub sellable: Option<u64>,
}

/// A product's offer in the market of a country.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductOffer {
    /// The country's market; `None` when no market sells to the country.
    pub market: Option<Market>,
    /// In the product's variant order.
    pub variants: Vec<VariantOffer>,
}

impl ProductOffer {
    /// Whether the country's market sells the product: some variant has a price there (a
    /// product page shows "not available in your country" otherwise).
    pub fn sold(&self) -> bool {
        self.variants.iter().any(|v| v.price.is_some())
    }
}

/// `product`'s prices and stock in `country`'s market. A country no market sells to
/// isn't an error: the product simply isn't sold there. The caller decides whether the
/// product may be shown at all (status, channel).
pub async fn price_product<S>(
    ctx: &Ctx,
    ports: &S,
    product: &Product,
    country: &str,
) -> Result<ProductOffer, CommandError>
where
    S: HasMarkets + HasCatalogs + HasPriceLists + HasInventoryItems + HasLocations + HasClock,
{
    ctx.require(Scope::ProductsRead)?;
    let country = CountryCode::new(country)?;
    let unpriced = |market| ProductOffer {
        market,
        variants: product
            .variants()
            .iter()
            .map(|v| VariantOffer {
                variant: v.id(),
                price: None,
                available: false,
                sellable: Some(0),
            })
            .collect(),
    };
    let Some(market) = ports.markets().get_by_country(country).await? else {
        return Ok(unpriced(None));
    };
    let variants: Vec<VariantId> = product.variants().iter().map(|v| v.id()).collect();
    let stock = StockContext::load(ports, market.id(), &variants).await?;
    // The market as found by country, not read again.
    let pricing = load_pricing_for(ports, market.clone(), &[product.id()], &variants).await?;
    let variants = variants
        .into_iter()
        .map(|variant| {
            let price = price_line(product, variant, Quantity::ONE, &pricing).ok();
            let in_stock = !matches!(
                stock.availability(variant, Quantity::ONE),
                Availability::OutOfStock { .. }
            );
            VariantOffer {
                variant,
                available: price.is_some() && in_stock,
                sellable: if price.is_some() {
                    stock.sellable(variant)
                } else {
                    Some(0)
                },
                price,
            }
        })
        .collect();
    Ok(ProductOffer {
        market: Some(market),
        variants,
    })
}

/// A product's prices in one market, each variant's explained: what the admin shows to say
/// why a shopper pays what they do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketPrices {
    pub market: Market,
    /// The market's catalog's lists, live or not, as `explanations` name them.
    pub price_lists: Vec<PriceList>,
    /// In the product's variant order.
    pub variants: Vec<(VariantId, PriceExplanation)>,
}

/// `product`'s prices for `quantity` of each variant, in `market` or every market, with
/// every price list of each market's catalog and what became of it. Unlike a shopper's
/// read, a product that isn't active is priced too: the admin sees what it would cost.
pub async fn explain_product_prices<S>(
    ctx: &Ctx,
    ports: &S,
    product: &Product,
    market: Option<MarketId>,
    quantity: Quantity,
) -> Result<Vec<MarketPrices>, CommandError>
where
    S: HasMarkets + HasCatalogs + HasPriceLists + HasClock,
{
    ctx.require(Scope::PricingRead)?;
    let markets = match market {
        Some(id) => vec![
            ports
                .markets()
                .get(id)
                .await?
                .ok_or(CommandError::MarketNotFound(id))?,
        ],
        None => ports.markets().list_all().await?,
    };
    let variants: Vec<VariantId> = product.variants().iter().map(|v| v.id()).collect();
    let mut found = Vec::with_capacity(markets.len());
    for market in markets {
        let pricing = load_pricing_for(ports, market.clone(), &[product.id()], &variants).await?;
        let explained = product
            .variants()
            .iter()
            .map(|v| {
                let prices = VariantPrices {
                    product: product.id(),
                    variant: v.id(),
                    base: v.base_price(),
                    compare_at: v.compare_at_price(),
                };
                (v.id(), explain_price(&pricing, prices, quantity))
            })
            .collect();
        found.push(MarketPrices {
            market,
            price_lists: pricing.price_lists,
            variants: explained,
        });
    }
    Ok(found)
}
