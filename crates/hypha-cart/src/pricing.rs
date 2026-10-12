use hypha_core::{CartId, Money, MoneyError, Price, ProductId, VariantId};
use hypha_markets::{Catalog, Market, PriceList};
use hypha_pricing::resolve_price;
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

    Ok(resolve_price(
        product.id(),
        variant,
        base,
        market,
        catalogs,
        price_lists,
    )?)
}

/// Prices every line at current prices. Lines that can't be priced are marked
/// unavailable and left out of the total; only overflowing the cart total is an error.
///
/// `products` should contain every product in the cart; a missing one makes its
/// lines unavailable with `ProductNotFound`.
pub fn price_cart(
    cart: &Cart,
    products: &[Product],
    market: &Market,
    catalogs: &[Catalog],
    price_lists: &[PriceList],
) -> Result<PricedCart, MoneyError> {
    let lines: Vec<PricedLine> = cart
        .lines()
        .iter()
        .map(|line| {
            let status = match line_total(line, products, market, catalogs, price_lists) {
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
    let total = Money::sum(&totals, market.currency())?;

    Ok(PricedCart {
        id: cart.id(),
        lines,
        total,
    })
}

fn line_total(
    line: &CartLine,
    products: &[Product],
    market: &Market,
    catalogs: &[Catalog],
    price_lists: &[PriceList],
) -> Result<(Price, Money), LineError> {
    let product = products
        .iter()
        .find(|p| p.id() == line.product())
        .ok_or(LineError::ProductNotFound(line.product()))?;
    let unit_price = price_line(product, line.variant(), market, catalogs, price_lists)?;
    let total = unit_price.money().checked_mul(line.quantity().value())?;
    Ok((unit_price, total))
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use hypha_core::{BasisPoints, CatalogId, CountryCode, Currency, MarketId, PriceListId};
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

    fn priced(unit: i64, total: i64) -> LineStatus {
        LineStatus::Priced {
            unit_price: eur(unit),
            total: Money::new(total, Currency::Eur),
        }
    }

    fn statuses(cart: &PricedCart) -> Vec<LineStatus> {
        cart.lines.iter().map(|l| l.status.clone()).collect()
    }

    #[test]
    fn empty_cart_totals_zero_in_market_currency() {
        let priced = price_cart(&cart(&[]), &[], &market(), &[], &[]).unwrap();
        assert_eq!(priced.total, Money::zero(Currency::Eur));
        assert!(priced.lines.is_empty());
        assert!(priced.is_fully_available());
    }

    #[test]
    fn multiplies_unit_price_and_sums_lines() {
        let products = [product(1, eur(1000)), product(2, eur(250))];
        let result = price_cart(
            &cart(&[(1, 2), (2, 3)]),
            &products,
            &market(),
            &[catalog(&[pid(1), pid(2)], None)],
            &[],
        )
        .unwrap();
        assert_eq!(
            statuses(&result),
            vec![priced(1000, 2000), priced(250, 750)]
        );
        assert_eq!(result.total, Money::new(2750, Currency::Eur));
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
        let result = price_cart(
            &cart(&[(1, 1)]),
            &[product(1, eur(1000))],
            &market(),
            &[catalog(&[pid(1)], Some(list_id))],
            &[list],
        )
        .unwrap();
        assert_eq!(statuses(&result), vec![priced(900, 900)]);
    }

    #[test]
    fn unavailable_line_is_excluded_from_total() {
        let mut archived = product(2, eur(250));
        archived.set_status(ProductStatus::Archived).unwrap();
        let result = price_cart(
            &cart(&[(1, 1), (2, 1)]),
            &[product(1, eur(1000)), archived],
            &market(),
            &[catalog(&[pid(1), pid(2)], None)],
            &[],
        )
        .unwrap();
        assert_eq!(
            statuses(&result),
            vec![
                priced(1000, 1000),
                LineStatus::Unavailable(LineError::ProductNotActive(pid(2)))
            ]
        );
        assert_eq!(result.total, Money::new(1000, Currency::Eur));
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
        let result = price_cart(&cart(&[(1, 1)]), &[], &market(), &[], &[]).unwrap();
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
            &[product(1, eur(i64::MAX))],
            &market(),
            &[catalog(&[pid(1)], None)],
            &[],
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
        let products = [product(1, eur(i64::MAX)), product(2, eur(1))];
        assert_eq!(
            price_cart(
                &cart(&[(1, 1), (2, 1)]),
                &products,
                &market(),
                &[catalog(&[pid(1), pid(2)], None)],
                &[]
            ),
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
            price_line(&draft, vid(11), &market(), &[catalog(&[pid(1)], None)], &[]),
            Err(LineError::ProductNotActive(pid(1)))
        );
    }

    #[test]
    fn price_line_rejects_variant_of_another_product() {
        assert_eq!(
            price_line(
                &product(1, eur(1000)),
                vid(99),
                &market(),
                &[catalog(&[pid(1)], None)],
                &[]
            ),
            Err(LineError::VariantNotFound {
                product: pid(1),
                variant: vid(99),
            })
        );
    }

    #[test]
    fn price_line_passes_through_pricing_errors() {
        assert_eq!(
            price_line(&product(1, eur(1000)), vid(11), &market(), &[], &[]),
            Err(LineError::Pricing(PricingError::NotAvailable {
                product: pid(1),
                market: mid(),
            }))
        );
    }
}
