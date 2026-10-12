use hypha_core::{CartId, Money, MoneyError, Price, ProductId, VariantId};
use hypha_pricing::{PricingContext, resolve_price};
use hypha_products::{Product, ProductStatus};

use crate::{
    cart::{Cart, CartLine},
    error::LineError,
    quantity::Quantity,
};

/// Whether a line can be bought right now.
///
/// A line can go stale after it was added (product archived or deleted, removed from
/// the catalog, price list changed currency). That makes only that line unavailable,
/// with a typed reason, instead of failing the whole cart. A UCP adapter can map each
/// reason to a `messages` entry pointing at the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineStatus {
    Priced { unit_price: Price, total: Money },
    Unavailable(LineError),
}

/// A cart line with its current status. A read model: computed, never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedLine {
    pub product: ProductId,
    pub variant: VariantId,
    pub quantity: Quantity,
    pub status: LineStatus,
}

impl PricedLine {
    pub fn is_available(&self) -> bool {
        matches!(self.status, LineStatus::Priced { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedCart {
    pub id: CartId,
    pub lines: Vec<PricedLine>,
    /// Sum of the available lines only, in the market's currency.
    pub total: Money,
}

impl PricedCart {
    /// Checkout should refuse a cart unless this is true.
    pub fn is_fully_available(&self) -> bool {
        self.lines.iter().all(PricedLine::is_available)
    }

    pub fn unavailable_lines(&self) -> impl Iterator<Item = &PricedLine> {
        self.lines.iter().filter(|l| !l.is_available())
    }
}

/// Unit price of `variant` in the pricing context's market. The product must be active
/// and own the variant.
///
/// This is where the cart composes the leaf domains: it pulls the base price out of
/// the `Product` and hands plain values to `resolve_price`.
pub fn price_line(
    product: &Product,
    variant: VariantId,
    pricing: &PricingContext,
) -> Result<Price, LineError> {
    if product.status() != ProductStatus::Active {
        return Err(LineError::ProductNotActive(product.id()));
    }

    let base = product
        .variants()
        .iter()
        .find(|v| v.id() == variant)
        .ok_or(LineError::VariantNotFound {
            product: product.id(),
            variant,
        })?
        .base_price();

    Ok(resolve_price(pricing, product.id(), variant, base)?)
}

/// Prices every line at current prices. Lines that can't be priced are marked
/// unavailable and left out of the total; only overflowing the cart total is an error.
///
/// `products` should contain every product in the cart; a missing one makes its
/// lines unavailable with `ProductNotFound`. `pricing` must have been built for all
/// of the cart's products and variants.
pub fn price_cart(
    cart: &Cart,
    products: &[Product],
    pricing: &PricingContext,
) -> Result<PricedCart, MoneyError> {
    let lines: Vec<PricedLine> = cart
        .lines()
        .iter()
        .map(|line| {
            let status = match line_total(line, products, pricing) {
                Ok((unit_price, total)) => LineStatus::Priced { unit_price, total },
                Err(reason) => LineStatus::Unavailable(reason),
            };
            PricedLine {
                product: line.product(),
                variant: line.variant(),
                quantity: line.quantity(),
                status,
            }
        })
        .collect();

    let totals: Vec<Money> = lines
        .iter()
        .filter_map(|l| match l.status {
            LineStatus::Priced { total, .. } => Some(total),
            LineStatus::Unavailable(_) => None,
        })
        .collect();
    let total = Money::sum(&totals, pricing.market.currency())?;

    Ok(PricedCart {
        id: cart.id(),
        lines,
        total,
    })
}

fn line_total(
    line: &CartLine,
    products: &[Product],
    pricing: &PricingContext,
) -> Result<(Price, Money), LineError> {
    let product = products
        .iter()
        .find(|p| p.id() == line.product())
        .ok_or(LineError::ProductNotFound(line.product()))?;
    let unit_price = price_line(product, line.variant(), pricing)?;
    let total = unit_price.money().checked_mul(line.quantity().value())?;
    Ok((unit_price, total))
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use hypha_core::{BasisPoints, CatalogId, CountryCode, Currency, MarketId, PriceListId};
    use hypha_markets::{Catalog, CatalogContext, Market, PriceList};
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
        Price::new(Money::new(amount, Currency::Eur).unwrap()).unwrap()
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

    /// An EUR market whose catalog (no price list) includes `products`.
    fn pricing(products: &[u128]) -> PricingContext {
        PricingContext {
            market: Market::new(
                mid(),
                "Eurozone".to_string(),
                vec![CountryCode::new("HR").unwrap()],
                Currency::Eur,
            )
            .unwrap(),
            catalog: Some(Catalog::new(
                CatalogId::from_uuid(Uuid::from_u128(200)),
                CatalogContext::Market(mid()),
                None,
            )),
            price_list: None,
            included_products: products.iter().map(|n| pid(*n)).collect::<HashSet<_>>(),
            fixed_prices: HashMap::new(),
        }
    }

    fn cart(lines: &[(u128, u32)]) -> Cart {
        let mut c = Cart::new(CartId::from_uuid(Uuid::from_u128(300)), mid());
        for (n, q) in lines {
            c.add_line(pid(*n), vid(n + 10), qty(*q)).unwrap();
        }
        c
    }

    fn priced(unit: i64, total: i64) -> LineStatus {
        LineStatus::Priced {
            unit_price: eur(unit),
            total: Money::new(total, Currency::Eur).unwrap(),
        }
    }

    fn statuses(cart: &PricedCart) -> Vec<LineStatus> {
        cart.lines.iter().map(|l| l.status.clone()).collect()
    }

    #[test]
    fn empty_cart_totals_zero_in_market_currency() {
        let priced = price_cart(&cart(&[]), &[], &pricing(&[])).unwrap();
        assert_eq!(priced.total, Money::zero(Currency::Eur));
        assert!(priced.lines.is_empty());
        assert!(priced.is_fully_available());
    }

    #[test]
    fn multiplies_unit_price_and_sums_lines() {
        let products = [product(1, eur(1000)), product(2, eur(250))];
        let result = price_cart(&cart(&[(1, 2), (2, 3)]), &products, &pricing(&[1, 2])).unwrap();
        assert_eq!(
            statuses(&result),
            vec![priced(1000, 2000), priced(250, 750)]
        );
        assert_eq!(result.total, Money::new(2750, Currency::Eur).unwrap());
    }

    #[test]
    fn uses_price_list_prices() {
        let list = PriceList::new(
            PriceListId::from_uuid(Uuid::from_u128(400)),
            Currency::Eur,
            BasisPoints::new(-1000).unwrap(),
        );
        let mut ctx = pricing(&[1, 2]);
        ctx.catalog
            .as_mut()
            .unwrap()
            .set_price_list(Some(list.id()));
        ctx.price_list = Some(list);
        ctx.fixed_prices.insert(vid(12), eur(100));

        let result = price_cart(
            &cart(&[(1, 1), (2, 1)]),
            &[product(1, eur(1000)), product(2, eur(250))],
            &ctx,
        )
        .unwrap();

        // Product 1 gets the -10% adjustment, product 2 its fixed price.
        assert_eq!(statuses(&result), vec![priced(900, 900), priced(100, 100)]);
    }

    #[test]
    fn unavailable_line_is_excluded_from_total() {
        let mut archived = product(2, eur(250));
        archived.set_status(ProductStatus::Archived).unwrap();
        let result = price_cart(
            &cart(&[(1, 1), (2, 1)]),
            &[product(1, eur(1000)), archived],
            &pricing(&[1, 2]),
        )
        .unwrap();
        assert_eq!(
            statuses(&result),
            vec![
                priced(1000, 1000),
                LineStatus::Unavailable(LineError::ProductNotActive(pid(2)))
            ]
        );
        assert_eq!(result.total, Money::new(1000, Currency::Eur).unwrap());
        assert!(!result.is_fully_available());
        assert_eq!(
            result
                .unavailable_lines()
                .map(|l| l.variant)
                .collect::<Vec<_>>(),
            vec![vid(12)]
        );
    }

    #[test]
    fn missing_product_makes_line_unavailable() {
        let result = price_cart(&cart(&[(1, 1)]), &[], &pricing(&[1])).unwrap();
        assert_eq!(
            statuses(&result),
            vec![LineStatus::Unavailable(LineError::ProductNotFound(pid(1)))]
        );
        assert_eq!(result.total, Money::zero(Currency::Eur));
    }

    #[test]
    fn line_total_overflow_makes_line_unavailable() {
        let result = price_cart(
            &cart(&[(1, 2)]),
            &[product(1, eur(Money::MAX_AMOUNT))],
            &pricing(&[1]),
        )
        .unwrap();
        assert_eq!(
            statuses(&result),
            vec![LineStatus::Unavailable(LineError::Money(
                MoneyError::Overflow
            ))]
        );
    }

    #[test]
    fn cart_total_overflow_is_an_error() {
        let products = [product(1, eur(Money::MAX_AMOUNT)), product(2, eur(1))];
        assert_eq!(
            price_cart(&cart(&[(1, 1), (2, 1)]), &products, &pricing(&[1, 2])),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn price_line_rejects_inactive_product() {
        let draft = Product::new_simple(
            pid(1),
            Handle::new("draft").unwrap(),
            "Draft".to_string(),
            vid(11),
            eur(1000),
        );
        assert_eq!(
            price_line(&draft, vid(11), &pricing(&[1])),
            Err(LineError::ProductNotActive(pid(1)))
        );
    }

    #[test]
    fn price_line_rejects_variant_of_another_product() {
        assert_eq!(
            price_line(&product(1, eur(1000)), vid(99), &pricing(&[1])),
            Err(LineError::VariantNotFound {
                product: pid(1),
                variant: vid(99),
            })
        );
    }

    #[test]
    fn price_line_passes_through_pricing_errors() {
        assert_eq!(
            price_line(&product(1, eur(1000)), vid(11), &pricing(&[])),
            Err(LineError::Pricing(PricingError::NotAvailable {
                product: pid(1),
                market: mid(),
            }))
        );
    }
}
