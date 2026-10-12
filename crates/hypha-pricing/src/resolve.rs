use hypha_core::{Currency, Price, VariantId};
use hypha_markets::{Catalog, CatalogContext, Market, PriceList, catalog_for};
use hypha_products::Product;

use crate::error::PricingError;

/// Price of `variant` in `market`.
///
/// 1. The market's catalog must include the product, otherwise it's not available there.
/// 2. If the catalog has a price list, a fixed price for the variant wins;
///    otherwise the variant's base price is adjusted by the list's basis points.
/// 3. Without a price list, the base price is used as is.
///
/// Every returned price is in the market's currency. Base prices in another currency
/// are an error until exchange rates exist.
pub fn resolve_price(
    product: &Product,
    variant: VariantId,
    market: &Market,
    catalogs: &[Catalog],
    price_lists: &[PriceList],
) -> Result<Price, PricingError> {
    let base = product
        .variants()
        .iter()
        .find(|v| v.id() == variant)
        .ok_or(PricingError::VariantNotFound {
            product: product.id(),
            variant,
        })?
        .base_price();

    let catalog = catalog_for(catalogs, CatalogContext::Market(market.id()))
        .filter(|c| c.includes(product.id()))
        .ok_or(PricingError::NotAvailable {
            product: product.id(),
            market: market.id(),
        })?;

    let Some(list_id) = catalog.price_list() else {
        ensure_currency(market.currency(), base)?;
        return Ok(base);
    };

    let list = price_lists
        .iter()
        .find(|l| l.id() == list_id)
        .ok_or(PricingError::PriceListNotFound(list_id))?;

    // A fixed price is in the list's currency, so the list itself has to match the market.
    if list.currency() != market.currency() {
        return Err(PricingError::CurrencyMismatch {
            expected: market.currency(),
            found: list.currency(),
        });
    }

    if let Some(fixed) = list.fixed_price(variant) {
        return Ok(fixed);
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
    use std::collections::{HashMap, HashSet};

    use hypha_core::{
        BasisPoints, CatalogId, CountryCode, MarketId, Money, PriceListId, ProductId,
    };
    use hypha_products::Handle;
    use uuid::Uuid;

    use super::*;

    const PRODUCT: u128 = 1;
    const VARIANT: u128 = 2;
    const MARKET: u128 = 3;
    const LIST: u128 = 4;

    fn vid(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn mid() -> MarketId {
        MarketId::from_uuid(Uuid::from_u128(MARKET))
    }

    fn plid() -> PriceListId {
        PriceListId::from_uuid(Uuid::from_u128(LIST))
    }

    fn pid() -> ProductId {
        ProductId::from_uuid(Uuid::from_u128(PRODUCT))
    }

    fn eur(amount: i64) -> Price {
        Price::new(Money::new(amount, Currency::Eur)).unwrap()
    }

    fn mug(base: Price) -> Product {
        Product::new_simple(
            pid(),
            Handle::new("mug").unwrap(),
            "Mug".to_string(),
            vid(VARIANT),
            base,
        )
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

    fn catalog(products: &[ProductId], price_list: Option<PriceListId>) -> Catalog {
        Catalog::new(
            CatalogId::from_uuid(Uuid::from_u128(5)),
            CatalogContext::Market(mid()),
            products.iter().copied().collect::<HashSet<_>>(),
            price_list,
        )
    }

    fn list(currency: Currency, bp: i32, fixed: &[(VariantId, Price)]) -> PriceList {
        PriceList::new(
            plid(),
            currency,
            BasisPoints::new(bp).unwrap(),
            fixed.iter().copied().collect::<HashMap<_, _>>(),
        )
        .unwrap()
    }

    #[test]
    fn uses_base_price_without_price_list() {
        let result = resolve_price(
            &mug(eur(1000)),
            vid(VARIANT),
            &market(Currency::Eur),
            &[catalog(&[pid()], None)],
            &[],
        );
        assert_eq!(result, Ok(eur(1000)));
    }

    #[test]
    fn fixed_price_wins_over_adjustment() {
        let result = resolve_price(
            &mug(eur(1000)),
            vid(VARIANT),
            &market(Currency::Eur),
            &[catalog(&[pid()], Some(plid()))],
            &[list(Currency::Eur, -5000, &[(vid(VARIANT), eur(777))])],
        );
        assert_eq!(result, Ok(eur(777)));
    }

    #[test]
    fn adjusts_base_price_without_fixed_price() {
        let result = resolve_price(
            &mug(eur(1000)),
            vid(VARIANT),
            &market(Currency::Eur),
            &[catalog(&[pid()], Some(plid()))],
            &[list(Currency::Eur, -1000, &[])],
        );
        assert_eq!(result, Ok(eur(900)));
    }

    #[test]
    fn adjustment_rounds_half_away_from_zero() {
        // 999 * 0.95 = 949.05 -> 949; 1010 * 0.95 = 959.5 -> 960
        let resolve = |base| {
            resolve_price(
                &mug(eur(base)),
                vid(VARIANT),
                &market(Currency::Eur),
                &[catalog(&[pid()], Some(plid()))],
                &[list(Currency::Eur, -500, &[])],
            )
        };
        assert_eq!(resolve(999), Ok(eur(949)));
        assert_eq!(resolve(1010), Ok(eur(960)));
    }

    #[test]
    fn rejects_variant_of_another_product() {
        let result = resolve_price(
            &mug(eur(1000)),
            vid(99),
            &market(Currency::Eur),
            &[catalog(&[pid()], None)],
            &[],
        );
        assert_eq!(
            result,
            Err(PricingError::VariantNotFound {
                product: pid(),
                variant: vid(99),
            })
        );
    }

    #[test]
    fn rejects_market_without_catalog() {
        let result = resolve_price(
            &mug(eur(1000)),
            vid(VARIANT),
            &market(Currency::Eur),
            &[],
            &[],
        );
        assert_eq!(
            result,
            Err(PricingError::NotAvailable {
                product: pid(),
                market: mid(),
            })
        );
    }

    #[test]
    fn rejects_product_missing_from_catalog() {
        let result = resolve_price(
            &mug(eur(1000)),
            vid(VARIANT),
            &market(Currency::Eur),
            &[catalog(&[], None)],
            &[],
        );
        assert_eq!(
            result,
            Err(PricingError::NotAvailable {
                product: pid(),
                market: mid(),
            })
        );
    }

    #[test]
    fn rejects_missing_price_list() {
        let result = resolve_price(
            &mug(eur(1000)),
            vid(VARIANT),
            &market(Currency::Eur),
            &[catalog(&[pid()], Some(plid()))],
            &[],
        );
        assert_eq!(result, Err(PricingError::PriceListNotFound(plid())));
    }

    #[test]
    fn rejects_base_price_in_other_currency_without_price_list() {
        let result = resolve_price(
            &mug(eur(1000)),
            vid(VARIANT),
            &market(Currency::Rsd),
            &[catalog(&[pid()], None)],
            &[],
        );
        assert_eq!(
            result,
            Err(PricingError::CurrencyMismatch {
                expected: Currency::Rsd,
                found: Currency::Eur,
            })
        );
    }

    #[test]
    fn rejects_base_price_in_other_currency_without_fixed_price() {
        let result = resolve_price(
            &mug(eur(1000)),
            vid(VARIANT),
            &market(Currency::Rsd),
            &[catalog(&[pid()], Some(plid()))],
            &[list(Currency::Rsd, 0, &[])],
        );
        assert_eq!(
            result,
            Err(PricingError::CurrencyMismatch {
                expected: Currency::Rsd,
                found: Currency::Eur,
            })
        );
    }

    #[test]
    fn fixed_price_covers_base_price_in_other_currency() {
        let rsd = Price::new(Money::new(117_000, Currency::Rsd)).unwrap();
        let result = resolve_price(
            &mug(eur(1000)),
            vid(VARIANT),
            &market(Currency::Rsd),
            &[catalog(&[pid()], Some(plid()))],
            &[list(Currency::Rsd, 0, &[(vid(VARIANT), rsd)])],
        );
        assert_eq!(result, Ok(rsd));
    }

    #[test]
    fn rejects_price_list_in_other_currency_than_market() {
        let result = resolve_price(
            &mug(eur(1000)),
            vid(VARIANT),
            &market(Currency::Rsd),
            &[catalog(&[pid()], Some(plid()))],
            &[list(Currency::Eur, 0, &[(vid(VARIANT), eur(900))])],
        );
        assert_eq!(
            result,
            Err(PricingError::CurrencyMismatch {
                expected: Currency::Rsd,
                found: Currency::Eur,
            })
        );
    }
}
