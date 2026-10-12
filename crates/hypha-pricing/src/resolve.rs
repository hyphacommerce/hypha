use std::collections::{HashMap, HashSet};

use hypha_core::{Currency, Price, ProductId, VariantId};
use hypha_markets::{Catalog, Market, PriceList};

use crate::error::PricingError;

/// Everything needed to price a set of variants in one market, as plain data.
///
/// The caller fills it with batched lookups (`CatalogRepo::included_products`,
/// `PriceListRepo::fixed_prices`) for exactly the products and variants it is about
/// to price. Pricing itself stays a pure function: no I/O, easy to test.
///
/// A product or variant that wasn't asked for reads as "not in the catalog" or
/// "no fixed price", so build the context from everything you'll resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricingContext {
    pub market: Market,
    /// The market's catalog, if it has one.
    pub catalog: Option<Catalog>,
    /// The catalog's price list, if it has one and it was found.
    pub price_list: Option<PriceList>,
    /// Which of the requested products the catalog includes.
    pub included_products: HashSet<ProductId>,
    /// Fixed prices in `price_list` for the requested variants that have one.
    pub fixed_prices: HashMap<VariantId, Price>,
}

/// Price of `variant` (of `product`, with `base` as its base price) in the context's market.
///
/// 1. The market's catalog must include the product, otherwise it's not available there.
/// 2. If the catalog has a price list, a fixed price for the variant wins;
///    otherwise the base price is adjusted by the list's basis points.
/// 3. Without a price list, the base price is used as is.
///
/// Every returned price is in the market's currency. Base prices in another currency
/// are an error until exchange rates exist.
pub fn resolve_price(
    pricing: &PricingContext,
    product: ProductId,
    variant: VariantId,
    base: Price,
) -> Result<Price, PricingError> {
    let market = &pricing.market;
    let catalog = pricing
        .catalog
        .as_ref()
        .filter(|_| pricing.included_products.contains(&product))
        .ok_or(PricingError::NotAvailable {
            product,
            market: market.id(),
        })?;

    let Some(list_id) = catalog.price_list() else {
        ensure_currency(market.currency(), base)?;
        return Ok(base);
    };

    let list = pricing
        .price_list
        .as_ref()
        .filter(|l| l.id() == list_id)
        .ok_or(PricingError::PriceListNotFound(list_id))?;

    // A fixed price is in the list's currency, so the list itself has to match the market.
    if list.currency() != market.currency() {
        return Err(PricingError::CurrencyMismatch {
            expected: market.currency(),
            found: list.currency(),
        });
    }

    if let Some(fixed) = pricing.fixed_prices.get(&variant) {
        return Ok(*fixed);
    }

    ensure_currency(market.currency(), base)?;
    let adjusted = base.money().adjust(list.adjustment())?;
    Ok(Price::new(adjusted)?)
}

fn ensure_currency(expected: Currency, price: Price) -> Result<(), PricingError> {
    let found = price.money().currency();
    if found != expected {
        Err(PricingError::CurrencyMismatch { expected, found })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{BasisPoints, CatalogId, CountryCode, MarketId, Money, PriceListId};
    use hypha_markets::CatalogContext;
    use uuid::Uuid;

    use super::*;

    fn vid() -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(2))
    }

    fn mid() -> MarketId {
        MarketId::from_uuid(Uuid::from_u128(3))
    }

    fn plid() -> PriceListId {
        PriceListId::from_uuid(Uuid::from_u128(4))
    }

    fn pid() -> ProductId {
        ProductId::from_uuid(Uuid::from_u128(1))
    }

    fn eur(amount: i64) -> Price {
        Price::new(Money::new(amount, Currency::Eur).unwrap()).unwrap()
    }

    fn rsd(amount: i64) -> Price {
        Price::new(Money::new(amount, Currency::Rsd).unwrap()).unwrap()
    }

    fn market(currency: Currency) -> Market {
        Market::new(
            mid(),
            "Test".to_string(),
            vec![CountryCode::new("RS").unwrap()],
            currency,
        )
        .unwrap()
    }

    fn list(currency: Currency, bp: i32) -> PriceList {
        PriceList::new(plid(), currency, BasisPoints::new(bp).unwrap())
    }

    /// The product is in the market's catalog; the catalog uses `price_list` if given.
    fn context(market_currency: Currency, price_list: Option<PriceList>) -> PricingContext {
        PricingContext {
            market: market(market_currency),
            catalog: Some(Catalog::new(
                CatalogId::from_uuid(Uuid::from_u128(5)),
                CatalogContext::Market(mid()),
                price_list.as_ref().map(|l| l.id()),
            )),
            price_list,
            included_products: HashSet::from([pid()]),
            fixed_prices: HashMap::new(),
        }
    }

    #[test]
    fn uses_base_price_without_price_list() {
        let ctx = context(Currency::Eur, None);
        assert_eq!(resolve_price(&ctx, pid(), vid(), eur(1000)), Ok(eur(1000)));
    }

    #[test]
    fn fixed_price_wins_over_adjustment() {
        let mut ctx = context(Currency::Eur, Some(list(Currency::Eur, -5000)));
        ctx.fixed_prices.insert(vid(), eur(777));
        assert_eq!(resolve_price(&ctx, pid(), vid(), eur(1000)), Ok(eur(777)));
    }

    #[test]
    fn adjusts_base_price_without_fixed_price() {
        let ctx = context(Currency::Eur, Some(list(Currency::Eur, -1000)));
        assert_eq!(resolve_price(&ctx, pid(), vid(), eur(1000)), Ok(eur(900)));
    }

    #[test]
    fn adjustment_rounds_half_away_from_zero() {
        // 999 * 0.95 = 949.05 -> 949; 1010 * 0.95 = 959.5 -> 960
        let ctx = context(Currency::Eur, Some(list(Currency::Eur, -500)));
        assert_eq!(resolve_price(&ctx, pid(), vid(), eur(999)), Ok(eur(949)));
        assert_eq!(resolve_price(&ctx, pid(), vid(), eur(1010)), Ok(eur(960)));
    }

    #[test]
    fn rejects_market_without_catalog() {
        let ctx = PricingContext {
            catalog: None,
            ..context(Currency::Eur, None)
        };
        assert_eq!(
            resolve_price(&ctx, pid(), vid(), eur(1000)),
            Err(PricingError::NotAvailable {
                product: pid(),
                market: mid(),
            })
        );
    }

    #[test]
    fn rejects_product_missing_from_catalog() {
        let ctx = PricingContext {
            included_products: HashSet::new(),
            ..context(Currency::Eur, None)
        };
        assert_eq!(
            resolve_price(&ctx, pid(), vid(), eur(1000)),
            Err(PricingError::NotAvailable {
                product: pid(),
                market: mid(),
            })
        );
    }

    #[test]
    fn rejects_missing_price_list() {
        let ctx = PricingContext {
            price_list: None,
            ..context(Currency::Eur, Some(list(Currency::Eur, 0)))
        };
        assert_eq!(
            resolve_price(&ctx, pid(), vid(), eur(1000)),
            Err(PricingError::PriceListNotFound(plid()))
        );
    }

    #[test]
    fn rejects_base_price_in_other_currency_without_price_list() {
        let ctx = context(Currency::Rsd, None);
        assert_eq!(
            resolve_price(&ctx, pid(), vid(), eur(1000)),
            Err(PricingError::CurrencyMismatch {
                expected: Currency::Rsd,
                found: Currency::Eur,
            })
        );
    }

    #[test]
    fn rejects_base_price_in_other_currency_without_fixed_price() {
        let ctx = context(Currency::Rsd, Some(list(Currency::Rsd, 0)));
        assert_eq!(
            resolve_price(&ctx, pid(), vid(), eur(1000)),
            Err(PricingError::CurrencyMismatch {
                expected: Currency::Rsd,
                found: Currency::Eur,
            })
        );
    }

    #[test]
    fn fixed_price_covers_base_price_in_other_currency() {
        let mut ctx = context(Currency::Rsd, Some(list(Currency::Rsd, 0)));
        ctx.fixed_prices.insert(vid(), rsd(117_000));
        assert_eq!(
            resolve_price(&ctx, pid(), vid(), eur(1000)),
            Ok(rsd(117_000))
        );
    }

    #[test]
    fn rejects_price_list_in_other_currency_than_market() {
        let mut ctx = context(Currency::Rsd, Some(list(Currency::Eur, 0)));
        ctx.fixed_prices.insert(vid(), eur(900));
        assert_eq!(
            resolve_price(&ctx, pid(), vid(), eur(1000)),
            Err(PricingError::CurrencyMismatch {
                expected: Currency::Rsd,
                found: Currency::Eur,
            })
        );
    }
}
