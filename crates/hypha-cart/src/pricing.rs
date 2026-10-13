use hypha_core::{
    CartId, CountryCode, Currency, CustomerId, Email, Money, MoneyError, Price, ProductId,
    Quantity, VariantId,
};
use hypha_inventory::{Availability, StockContext};
use hypha_pricing::{PricingContext, resolve_price};
use hypha_products::{Product, ProductStatus};
use hypha_tax::{TaxContext, TaxLine, TaxableLine, tax_line};

use crate::{
    cart::{Cart, CartLine},
    error::LineError,
};

/// Whether a line can be bought right now.
///
/// A line can go stale after it was added (product archived or deleted, removed from
/// the catalog, price list changed currency, stock ran out). That makes only that line unavailable,
/// with a typed reason, instead of failing the whole cart. A UCP adapter can map each
/// reason to a `messages` entry pointing at the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineStatus {
    Priced {
        unit_price: Price,
        total: Money,
        /// `None` when the cart's country has no tax region.
        tax: Option<TaxLine>,
    },
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
    pub country: CountryCode,
    pub email: Option<Email>,
    pub customer: Option<CustomerId>,
    pub lines: Vec<PricedLine>,
    /// Whether line prices include tax (the market decides).
    pub taxes_included: bool,
    /// Sum of the available lines' totals, as priced. All amounts are in the market's
    /// currency and count the available lines only.
    pub subtotal: Money,
    /// Tax on the available lines: already in `subtotal` if `taxes_included`, otherwise
    /// added on top of it.
    pub tax_total: Money,
    /// What the buyer pays.
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

/// Prices and taxes every line at current prices and rates, and checks its stock. Lines
/// that can't be priced or supplied are marked unavailable and left out of the totals.
///
/// Never fails: a line that would push a cart total past `Money::MAX_AMOUNT` (lines
/// that each fit can stop fitting together after a price change) is marked
/// unavailable with `Money(Overflow)` too, in line order, so a cart always stays
/// readable.
///
/// `products` should contain every product in the cart; a missing one makes its
/// lines unavailable with `ProductNotFound`. `pricing`, `tax` and `stock` must have
/// been built for all of the cart's products and variants.
pub fn price_cart(
    cart: &Cart,
    products: &[Product],
    pricing: &PricingContext,
    tax: &TaxContext,
    stock: &StockContext,
) -> PricedCart {
    let currency = pricing.market.currency();
    let mut totals = Totals::zero(currency);
    let mut lines = Vec::with_capacity(cart.lines().len());

    for line in cart.lines() {
        let status = match line_total(line, products, pricing, tax, stock) {
            Ok((unit_price, total, line_tax)) => {
                let line_tax_amount = line_tax.map(|t| t.amount);
                match totals.add(total, line_tax_amount, tax.prices_include_tax) {
                    Ok(new_totals) => {
                        totals = new_totals;
                        LineStatus::Priced {
                            unit_price,
                            total,
                            tax: line_tax,
                        }
                    }
                    Err(e) => LineStatus::Unavailable(LineError::Money(e)),
                }
            }
            Err(reason) => LineStatus::Unavailable(reason),
        };
        lines.push(PricedLine {
            product: line.product(),
            variant: line.variant(),
            quantity: line.quantity(),
            status,
        });
    }

    PricedCart {
        id: cart.id(),
        country: cart.country(),
        email: cart.email().cloned(),
        customer: cart.customer(),
        lines,
        taxes_included: tax.prices_include_tax,
        subtotal: totals.subtotal,
        tax_total: totals.tax_total,
        total: totals.total,
    }
}

/// A cart's running totals while its lines are priced.
#[derive(Clone, Copy)]
struct Totals {
    subtotal: Money,
    tax_total: Money,
    /// `subtotal`, plus `tax_total` when taxes are added on top of prices.
    total: Money,
}

impl Totals {
    fn zero(currency: Currency) -> Self {
        let zero = Money::zero(currency);
        Self {
            subtotal: zero,
            tax_total: zero,
            total: zero,
        }
    }

    /// The totals with one more line, or `Overflow` if any of them would not fit.
    fn add(
        self,
        line_total: Money,
        line_tax: Option<Money>,
        taxes_included: bool,
    ) -> Result<Self, MoneyError> {
        let line_tax = line_tax.unwrap_or(Money::zero(line_total.currency()));
        let into_total = if taxes_included {
            line_total
        } else {
            line_total.checked_add(line_tax)?
        };
        Ok(Self {
            subtotal: self.subtotal.checked_add(line_total)?,
            tax_total: self.tax_total.checked_add(line_tax)?,
            total: self.total.checked_add(into_total)?,
        })
    }
}

fn line_total(
    line: &CartLine,
    products: &[Product],
    pricing: &PricingContext,
    tax: &TaxContext,
    stock: &StockContext,
) -> Result<(Price, Money, Option<TaxLine>), LineError> {
    let product = products
        .iter()
        .find(|p| p.id() == line.product())
        .ok_or(LineError::ProductNotFound(line.product()))?;
    let unit_price = price_line(product, line.variant(), pricing)?;
    check_stock(stock, line.variant(), line.quantity())?;
    let total = unit_price.money().checked_mul(line.quantity().value())?;
    let taxable = TaxableLine {
        product: line.product(),
        amount: total,
    };
    Ok((unit_price, total, tax_line(tax, &taxable)?))
}

/// Whether the market's locations can supply `quantity` of `variant` right now.
/// Untracked stock always can.
pub fn check_stock(
    stock: &StockContext,
    variant: VariantId,
    quantity: Quantity,
) -> Result<(), LineError> {
    match stock.availability(variant, quantity) {
        Availability::Untracked | Availability::Available => Ok(()),
        Availability::OutOfStock { available } => Err(LineError::OutOfStock {
            variant,
            requested: quantity.value(),
            available,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use hypha_core::{
        BasisPoints, CatalogId, Currency, InventoryItemId, LocationId, MarketId, PriceListId,
        TaxClassId, TaxRate, TaxRegionId,
    };
    use hypha_inventory::{InventoryItem, Location};
    use hypha_markets::{Catalog, CatalogContext, Market, PriceList};
    use hypha_pricing::PricingError;
    use hypha_products::Handle;
    use hypha_tax::{TaxRegion, TaxSource};
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
            market: Market::new(mid(), "Eurozone".to_string(), vec![hr()], Currency::Eur).unwrap(),
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

    fn hr() -> CountryCode {
        CountryCode::new("HR").unwrap()
    }

    /// No inventory items: every variant is untracked, so stock never limits a line.
    fn no_stock() -> StockContext {
        StockContext::default()
    }

    fn no_tax() -> TaxContext {
        TaxContext {
            region: None,
            prices_include_tax: false,
            product_classes: HashMap::new(),
        }
    }

    fn region_id() -> TaxRegionId {
        TaxRegionId::from_uuid(Uuid::from_u128(500))
    }

    /// Croatia at 25%.
    fn croatia(prices_include_tax: bool) -> TaxContext {
        TaxContext {
            region: Some(TaxRegion::new(
                region_id(),
                hr(),
                TaxRate::new(250_000).unwrap(),
            )),
            prices_include_tax,
            product_classes: HashMap::new(),
        }
    }

    fn cart(lines: &[(u128, u32)]) -> Cart {
        let mut c = Cart::new(CartId::from_uuid(Uuid::from_u128(300)), mid(), hr());
        for (n, q) in lines {
            c.add_line(pid(*n), vid(n + 10), qty(*q)).unwrap();
        }
        c
    }

    fn money(amount: i64) -> Money {
        Money::new(amount, Currency::Eur).unwrap()
    }

    fn priced(unit: i64, total: i64) -> LineStatus {
        LineStatus::Priced {
            unit_price: eur(unit),
            total: money(total),
            tax: None,
        }
    }

    fn statuses(cart: &PricedCart) -> Vec<LineStatus> {
        cart.lines.iter().map(|l| l.status.clone()).collect()
    }

    #[test]
    fn empty_cart_totals_zero_in_market_currency() {
        let priced = price_cart(&cart(&[]), &[], &pricing(&[]), &no_tax(), &no_stock());
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
            &pricing(&[1, 2]),
            &no_tax(),
            &no_stock(),
        );
        assert_eq!(
            statuses(&result),
            vec![priced(1000, 2000), priced(250, 750)]
        );
        assert_eq!(result.subtotal, money(2750));
        assert_eq!(result.tax_total, money(0));
        assert_eq!(result.total, money(2750));
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
            &no_tax(),
            &no_stock(),
        );

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
            &no_tax(),
            &no_stock(),
        );
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
        let result = price_cart(
            &cart(&[(1, 1)]),
            &[],
            &pricing(&[1]),
            &no_tax(),
            &no_stock(),
        );
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
            &no_tax(),
            &no_stock(),
        );
        assert_eq!(
            statuses(&result),
            vec![LineStatus::Unavailable(LineError::Money(
                MoneyError::Overflow
            ))]
        );
    }

    #[test]
    fn line_that_would_overflow_the_cart_total_is_unavailable() {
        let products = [product(1, eur(Money::MAX_AMOUNT)), product(2, eur(1))];
        let result = price_cart(
            &cart(&[(1, 1), (2, 1)]),
            &products,
            &pricing(&[1, 2]),
            &no_tax(),
            &no_stock(),
        );
        assert_eq!(
            statuses(&result),
            vec![
                priced(Money::MAX_AMOUNT, Money::MAX_AMOUNT),
                LineStatus::Unavailable(LineError::Money(MoneyError::Overflow)),
            ]
        );
        assert_eq!(
            result.total,
            Money::new(Money::MAX_AMOUNT, Currency::Eur).unwrap()
        );
    }

    // ---- tax ----

    fn taxed(unit: i64, total: i64, tax: i64, class: Option<TaxClassId>) -> LineStatus {
        LineStatus::Priced {
            unit_price: eur(unit),
            total: money(total),
            tax: Some(TaxLine {
                rate: TaxRate::new(250_000).unwrap(),
                class,
                amount: money(tax),
                source: TaxSource::RateTable {
                    region: region_id(),
                },
            }),
        }
    }

    #[test]
    fn tax_is_added_on_top_when_prices_exclude_it() {
        let products = [product(1, eur(1000)), product(2, eur(250))];
        let result = price_cart(
            &cart(&[(1, 2), (2, 3)]),
            &products,
            &pricing(&[1, 2]),
            &croatia(false),
            &no_stock(),
        );

        // Per line: 2000 × 25% = 500, 750 × 25% = 187.5 -> 188.
        assert_eq!(
            statuses(&result),
            vec![taxed(1000, 2000, 500, None), taxed(250, 750, 188, None)]
        );
        assert!(!result.taxes_included);
        assert_eq!(result.subtotal, money(2750));
        assert_eq!(result.tax_total, money(688));
        assert_eq!(result.total, money(3438));
    }

    #[test]
    fn tax_is_extracted_when_prices_include_it() {
        let result = price_cart(
            &cart(&[(1, 2)]),
            &[product(1, eur(1000))],
            &pricing(&[1]),
            &croatia(true),
            &no_stock(),
        );

        // 2000 includes 25%: 2000 × 0.25 / 1.25 = 400.
        assert_eq!(statuses(&result), vec![taxed(1000, 2000, 400, None)]);
        assert!(result.taxes_included);
        assert_eq!(result.subtotal, money(2000));
        assert_eq!(result.tax_total, money(400));
        assert_eq!(result.total, money(2000));
    }

    #[test]
    fn unavailable_line_is_not_taxed() {
        let result = price_cart(
            &cart(&[(1, 1), (2, 1)]),
            &[product(1, eur(1000))],
            &pricing(&[1, 2]),
            &croatia(false),
            &no_stock(),
        );
        assert_eq!(result.tax_total, money(250));
        assert_eq!(result.total, money(1250));
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

    // ---- stock ----

    /// Variant 11 (product 1) has `on_hand` at the market's one location.
    fn stock(on_hand: i64) -> StockContext {
        let location = LocationId::from_uuid(Uuid::from_u128(600));
        let mut item = InventoryItem::new(
            InventoryItemId::from_uuid(Uuid::from_u128(601)),
            vid(11),
            true,
            false,
        );
        item.set_on_hand(location, on_hand, None).unwrap();
        StockContext {
            items: HashMap::from([(vid(11), item)]),
            locations: vec![Location::new(location, "Zagreb".to_string(), vec![mid()], 0).unwrap()],
        }
    }

    #[test]
    fn a_line_beyond_the_stock_is_unavailable_and_left_out_of_the_totals() {
        let products = [product(1, eur(1000)), product(2, eur(250))];
        let result = price_cart(
            &cart(&[(1, 3), (2, 1)]),
            &products,
            &pricing(&[1, 2]),
            &no_tax(),
            &stock(2),
        );
        assert_eq!(
            statuses(&result),
            vec![
                LineStatus::Unavailable(LineError::OutOfStock {
                    variant: vid(11),
                    requested: 3,
                    available: 2
                }),
                priced(250, 250)
            ]
        );
        assert_eq!(result.total, money(250));
        assert!(!result.is_fully_available());
    }

    #[test]
    fn a_line_within_the_stock_is_priced() {
        let products = [product(1, eur(1000))];
        let result = price_cart(
            &cart(&[(1, 2)]),
            &products,
            &pricing(&[1]),
            &no_tax(),
            &stock(2),
        );
        assert_eq!(statuses(&result), vec![priced(1000, 2000)]);
    }

    #[test]
    fn a_pricing_problem_is_reported_before_a_stock_problem() {
        // Not in the catalog and out of stock: the catalog reason wins.
        let products = [product(1, eur(1000))];
        let result = price_cart(
            &cart(&[(1, 5)]),
            &products,
            &pricing(&[]),
            &no_tax(),
            &stock(0),
        );
        assert!(matches!(
            &result.lines[0].status,
            LineStatus::Unavailable(LineError::Pricing(_))
        ));
    }
}
