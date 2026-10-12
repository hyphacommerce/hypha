use hypha_core::{CartId, Money, Price, ProductId, VariantId};
use hypha_markets::{Catalog, Market, PriceList};
use hypha_pricing::resolve_price;
use hypha_products::{Product, ProductStatus};

use crate::{cart::Cart, error::PriceCartError, quantity::Quantity};

/// A cart line with its current price. A read model: computed, never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedLine {
    pub product: ProductId,
    pub variant: VariantId,
    pub quantity: Quantity,
    pub unit_price: Price,
    pub total: Money,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedCart {
    pub id: CartId,
    pub lines: Vec<PricedLine>,
    /// In the market's currency.
    pub total: Money,
}

/// Unit price of `variant` in `market`. The product must be active and own the variant.
///
/// This is where the cart composes the leaf domains: it pulls the base price out of
/// the `Product` and hands plain values to `resolve_price`.
pub fn price_line(
    product: &Product,
    variant: VariantId,
    market: &Market,
    catalogs: &[Catalog],
    price_lists: &[PriceList],
) -> Result<Price, PriceCartError> {
    if product.status() != ProductStatus::Active {
        return Err(PriceCartError::ProductNotActive(product.id()));
    }

    let base = product
        .variants()
        .iter()
        .find(|v| v.id() == variant)
        .ok_or(PriceCartError::VariantNotFound {
            product: product.id(),
            variant,
        })?
        .base_price();

    Ok(resolve_price(
        product.id(),
        variant,
        base,
        market,
        catalogs,
        price_lists,
    )?)
}

/// Prices every line at current prices and sums them with `Money::sum`.
/// `products` must contain every product in the cart.
pub fn price_cart(
    cart: &Cart,
    products: &[Product],
    market: &Market,
    catalogs: &[Catalog],
    price_lists: &[PriceList],
) -> Result<PricedCart, PriceCartError> {
    let mut lines = Vec::with_capacity(cart.lines().len());
    for line in cart.lines() {
        let product = products
            .iter()
            .find(|p| p.id() == line.product())
            .ok_or(PriceCartError::ProductNotFound(line.product()))?;
        let unit_price = price_line(product, line.variant(), market, catalogs, price_lists)?;
        let total = unit_price.money().checked_mul(line.quantity().value())?;
        lines.push(PricedLine {
            product: line.product(),
            variant: line.variant(),
            quantity: line.quantity(),
            unit_price,
            total,
        });
    }

    let totals: Vec<Money> = lines.iter().map(|l| l.total).collect();
    let total = Money::sum(&totals, market.currency())?;

    Ok(PricedCart {
        id: cart.id(),
        lines,
        total,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use hypha_core::{
        BasisPoints, CatalogId, CountryCode, Currency, MarketId, MoneyError, PriceListId,
    };
    use hypha_markets::CatalogContext;
    use hypha_pricing::PricingError;
    use hypha_products::Handle;
    use uuid::Uuid;

    use super::*;

    fn mid() -> MarketId {
        MarketId::from_uuid(Uuid::from_u128(100))
    }

    fn pid(n: u128) -> ProductId {
        ProductId::from_uuid(Uuid::from_u128(n))
    }

    fn vid(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn eur(amount: i64) -> Price {
        Price::new(Money::new(amount, Currency::Eur)).unwrap()
    }

    fn qty(n: u32) -> Quantity {
        Quantity::new(n).unwrap()
    }

    /// Active simple product `n` with variant `n + 10`.
    fn product(n: u128, base: Price) -> Product {
        let mut p = Product::new_simple(
            pid(n),
            Handle::new(&format!("p{n}")).unwrap(),
            format!("Product {n}"),
            vid(n + 10),
            base,
        );
        p.set_status(ProductStatus::Active).unwrap();
        p
    }

    fn market() -> Market {
        Market::new(
            mid(),
            "Eurozone".to_string(),
            vec![CountryCode::new("HR").unwrap()],
            Currency::Eur,
        )
        .unwrap()
    }

    fn catalog(products: &[ProductId], price_list: Option<PriceListId>) -> Catalog {
        Catalog::new(
            CatalogId::from_uuid(Uuid::from_u128(200)),
            CatalogContext::Market(mid()),
            products.iter().copied().collect::<HashSet<_>>(),
            price_list,
        )
    }

    fn cart(lines: &[(u128, u32)]) -> Cart {
        let mut c = Cart::new(CartId::from_uuid(Uuid::from_u128(300)), mid());
        for (n, q) in lines {
            c.add_line(pid(*n), vid(n + 10), qty(*q)).unwrap();
        }
        c
    }

    #[test]
    fn empty_cart_totals_zero_in_market_currency() {
        let priced = price_cart(&cart(&[]), &[], &market(), &[], &[]).unwrap();
        assert_eq!(priced.total, Money::zero(Currency::Eur));
        assert!(priced.lines.is_empty());
    }

    #[test]
    fn multiplies_unit_price_and_sums_lines() {
        let products = [product(1, eur(1000)), product(2, eur(250))];
        let priced = price_cart(
            &cart(&[(1, 2), (2, 3)]),
            &products,
            &market(),
            &[catalog(&[pid(1), pid(2)], None)],
            &[],
        )
        .unwrap();
        let line_totals: Vec<Money> = priced.lines.iter().map(|l| l.total).collect();
        assert_eq!(
            line_totals,
            vec![
                Money::new(2000, Currency::Eur),
                Money::new(750, Currency::Eur)
            ]
        );
        assert_eq!(priced.total, Money::new(2750, Currency::Eur));
    }

    #[test]
    fn uses_price_list_prices() {
        let list_id = PriceListId::from_uuid(Uuid::from_u128(400));
        let list = PriceList::new(
            list_id,
            Currency::Eur,
            BasisPoints::new(-1000).unwrap(),
            HashMap::new(),
        )
        .unwrap();
        let priced = price_cart(
            &cart(&[(1, 1)]),
            &[product(1, eur(1000))],
            &market(),
            &[catalog(&[pid(1)], Some(list_id))],
            &[list],
        )
        .unwrap();
        assert_eq!(priced.lines[0].unit_price, eur(900));
    }

    #[test]
    fn rejects_inactive_product() {
        let draft = Product::new_simple(
            pid(1),
            Handle::new("draft").unwrap(),
            "Draft".to_string(),
            vid(11),
            eur(1000),
        );
        assert_eq!(
            price_line(&draft, vid(11), &market(), &[catalog(&[pid(1)], None)], &[]),
            Err(PriceCartError::ProductNotActive(pid(1)))
        );
    }

    #[test]
    fn rejects_variant_of_another_product() {
        assert_eq!(
            price_line(
                &product(1, eur(1000)),
                vid(99),
                &market(),
                &[catalog(&[pid(1)], None)],
                &[]
            ),
            Err(PriceCartError::VariantNotFound {
                product: pid(1),
                variant: vid(99),
            })
        );
    }

    #[test]
    fn passes_through_pricing_errors() {
        assert_eq!(
            price_line(&product(1, eur(1000)), vid(11), &market(), &[], &[]),
            Err(PriceCartError::Pricing(PricingError::NotAvailable {
                product: pid(1),
                market: mid(),
            }))
        );
    }

    #[test]
    fn rejects_missing_product() {
        assert_eq!(
            price_cart(&cart(&[(1, 1)]), &[], &market(), &[], &[]),
            Err(PriceCartError::ProductNotFound(pid(1)))
        );
    }

    #[test]
    fn rejects_line_total_overflow() {
        let products = [product(1, eur(i64::MAX))];
        assert_eq!(
            price_cart(
                &cart(&[(1, 2)]),
                &products,
                &market(),
                &[catalog(&[pid(1)], None)],
                &[]
            ),
            Err(PriceCartError::Money(MoneyError::Overflow))
        );
    }
}
