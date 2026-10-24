//! The cart's promotions on the discounts domain: `hypha_cart::Promotions` implemented by
//! reading what a cart's discounts are weighed with, running the engine
//! (`hypha_discounts::apply`) on the lines the cart hands over, and mapping the outcome
//! back into the cart's types.

use std::collections::{HashMap, HashSet};

use futures_util::future::try_join3;
use hypha_cart::{
    Cart, PromotionReading, Promotions,
    promotions::{
        Application as CartApplication, Class as CartClass, CodeOutcome as CartCodeOutcome,
        CodeState as CartCodeState, Needed as CartNeeded, PromotionInput, PromotionOutcome,
        Share as CartShare, ShippingShares,
    },
};
use hypha_core::{CategoryId, CustomerGroupId, ProductId, RepoError, Timestamp};
use hypha_customers::{CustomerRepo, HasCustomers};
use hypha_discounts::{
    Application, Buyer, Class, CodeOutcome, CodeState, DiscountContext, DiscountInput,
    DiscountRepo, HasDiscounts, Line, NameRequest, Names, Needed, Share, ShippingGroup, apply,
    find_live_code,
    json::{target_json, value_json},
    normalize,
};
use hypha_markets::{HasPriceLists, PriceListRepo};

/// What a host's ports name as their `hypha_cart::HasPromotions::Promotions`.
pub struct CartPromotions;

impl<P: HasDiscounts + HasCustomers + HasPriceLists> Promotions<P> for CartPromotions {
    type Reading = DiscountReading;

    fn normalize_code(raw: &str) -> Option<String> {
        normalize(raw).ok()
    }

    async fn code_is_live(ports: &P, code: &str, at: Timestamp) -> Result<bool, RepoError> {
        Ok(find_live_code(ports, code, at).await?.is_some())
    }

    async fn read(
        ports: &P,
        cart: &Cart,
        groups: Vec<CustomerGroupId>,
        at: Timestamp,
    ) -> Result<DiscountReading, RepoError> {
        load_discounts(ports, cart, groups, at).await
    }
}

/// What a cart's discounts are weighed with: the discounts that may apply, the buyer,
/// each product's categories (with their ancestors) and the moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscountReading {
    pub context: DiscountContext,
    pub buyer: Buyer,
    /// Loaded only when a discount names categories.
    pub categories: HashMap<ProductId, HashSet<CategoryId>>,
    pub at: Timestamp,
    /// What the discounts' summaries call what they name, as shoppers may see it.
    pub names: Names,
}

/// Nothing to weigh: no discount applies.
impl Default for DiscountReading {
    fn default() -> Self {
        Self {
            context: DiscountContext::default(),
            buyer: Buyer::default(),
            categories: HashMap::new(),
            at: Timestamp::UNIX_EPOCH,
            names: Names::new(),
        }
    }
}

impl PromotionReading for DiscountReading {
    fn apply(&self, input: &PromotionInput<'_>) -> PromotionOutcome {
        let engine = DiscountInput {
            at: self.at,
            market: input.market,
            channel: input.channel,
            currency: input.currency,
            buyer: self.buyer.clone(),
            lines: input
                .lines
                .iter()
                .map(|l| Line {
                    product: l.product,
                    variant: l.variant,
                    categories: self.categories.get(&l.product).cloned().unwrap_or_default(),
                    unit_price: l.unit_price,
                    quantity: l.quantity,
                    on_sale: l.on_sale,
                })
                .collect(),
            shipping: input
                .shipping
                .iter()
                .map(|g| ShippingGroup {
                    lines: g.lines.clone(),
                    rate: g.rate,
                })
                .collect(),
            codes: input.codes.to_vec(),
            names: &self.names,
        };
        let outcome = apply(&engine, &self.context.candidates());
        PromotionOutcome {
            applications: outcome
                .applications
                .iter()
                .map(|a| self.application(a))
                .collect(),
            lines: outcome.lines.iter().map(|l| shares(l)).collect(),
            shipping: outcome
                .shipping
                .iter()
                .map(|g| ShippingShares {
                    subtotal: g.subtotal,
                    shares: shares(&g.shares),
                })
                .collect(),
            codes: outcome.codes.iter().map(code_outcome).collect(),
            total: outcome.total,
        }
    }

    fn reduces_shipping(&self) -> bool {
        self.context
            .candidates()
            .iter()
            .any(|c| c.discount.classes().contains(Class::Shipping))
    }
}

impl DiscountReading {
    fn application(&self, a: &Application) -> CartApplication {
        CartApplication {
            discount: a.discount,
            code: a.code.clone(),
            title: a.title.clone(),
            summary: a.summary.clone(),
            class: match a.class {
                Class::Product => CartClass::Product,
                Class::Order => CartClass::Order,
                Class::Shipping => CartClass::Shipping,
            },
            value: value_json(&a.value),
            target: target_json(&a.target),
            amount: a.amount,
            once_per_customer: self
                .context
                .find(a.discount)
                .is_some_and(|d| d.limits().once_per_customer),
            code_limit: a
                .code
                .as_deref()
                .and_then(|c| self.context.code(c))
                .and_then(|row| row.usage_limit),
        }
    }
}

fn shares(shares: &[Share]) -> Vec<CartShare> {
    shares
        .iter()
        .map(|s| CartShare {
            application: s.application,
            amount: s.amount,
            units: s.units,
        })
        .collect()
}

fn code_outcome(c: &CodeOutcome) -> CartCodeOutcome {
    CartCodeOutcome {
        code: c.code.clone(),
        state: match c.state {
            CodeState::Applied => CartCodeState::Applied,
            CodeState::NotEligible { condition } => CartCodeState::NotEligible { condition },
            CodeState::MinimumNotMet { needed } => CartCodeState::MinimumNotMet {
                needed: match needed {
                    Needed::Amount(m) => CartNeeded::Amount(m),
                    Needed::Quantity(n) => CartNeeded::Quantity(n),
                },
            },
            CodeState::NoMatchingItems => CartCodeState::NoMatchingItems,
            CodeState::BlockedByOther { discount } => CartCodeState::BlockedByOther { discount },
            CodeState::NoSaving => CartCodeState::NoSaving,
            CodeState::LimitReached => CartCodeState::LimitReached,
            CodeState::NotAvailableInMarket => CartCodeState::NotAvailableInMarket,
            CodeState::Expired => CartCodeState::Expired,
            CodeState::Invalid => CartCodeState::Invalid,
        },
    }
}

/// The discounts that may apply to `cart` at `at`, and its buyer: the linked customer, or
/// for a guest the customer their email belongs to (once-per-customer counts by email
/// too; looked up only when a discount here is once per customer). Signed in means the
/// cart has a customer. The discounts come in one round of reads; their usage, the
/// products' categories and the summaries' names in the next.
async fn load_discounts<S: HasDiscounts + HasCustomers + HasPriceLists>(
    ports: &S,
    cart: &Cart,
    groups: Vec<CustomerGroupId>,
    at: Timestamp,
) -> Result<DiscountReading, RepoError> {
    let mut context = DiscountContext::load(ports, at, cart.discount_codes()).await?;
    let per_customer = context.counts_per_customer();
    let usage = async {
        let customer = match (cart.customer(), cart.email()) {
            (Some(customer), _) => Some(customer),
            (None, Some(email)) if per_customer => {
                ports.customers().get_by_email(email).await?.map(|c| c.id())
            }
            _ => None,
        };
        let usage = context.usage(ports, customer).await?;
        Ok::<_, RepoError>((customer, usage))
    };
    let products = product_ids(cart);
    let categories = async {
        if context.names_categories() && !products.is_empty() {
            ports.price_lists().product_categories(&products).await
        } else {
            Ok(Default::default())
        }
    };
    // Summaries go into the cart and the order, so they name only what shoppers may see.
    let request = NameRequest::for_shoppers(context.candidates().iter().map(|l| l.discount));
    let names = async {
        if request.is_empty() {
            Ok(Names::new())
        } else {
            ports.discounts().names(&request).await
        }
    };
    let ((customer, usage), categories, names) = try_join3(usage, categories, names).await?;
    context.set_usage(usage);
    Ok(DiscountReading {
        context,
        buyer: Buyer {
            customer,
            groups,
            signed_in: cart.customer().is_some(),
        },
        categories,
        at,
        names,
    })
}

/// Each product in the cart once.
fn product_ids(cart: &Cart) -> Vec<ProductId> {
    let mut ids: Vec<ProductId> = cart.lines().iter().map(|l| l.product()).collect();
    ids.sort_by_key(|p| p.as_uuid());
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use hypha_cart::{
        Cart, LineStatus, PricedCart, Publication, ShippingContext, price_cart_with_shipping,
    };
    use hypha_core::{
        BasisPoints, CartId, CatalogId, ChannelId, CountryCode, Currency, MarketId, Money, Price,
        ProductId, Quantity, TaxRate, TaxRegionId, VariantId,
    };
    use hypha_discounts::{Discount, DiscountDraft, ItemSet, Kind, Method, Reward, Target, Value};
    use hypha_inventory::StockContext;
    use hypha_markets::{Catalog, CatalogContext, Market};
    use hypha_pricing::PricingContext;
    use hypha_products::{Handle, Product, ProductStatus};
    use hypha_tax::{TaxContext, TaxRegion};
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
        Price::new(money(amount)).unwrap()
    }

    fn money(amount: i64) -> Money {
        Money::new(amount, Currency::EUR).unwrap()
    }

    fn hr() -> CountryCode {
        CountryCode::new("HR").unwrap()
    }

    /// Active simple product `n` with variant `n + 10`.
    fn product(n: u128, base: Price) -> Product {
        let mut p = Product::new_simple(
            pid(n),
            Handle::new(&format!("p{n}")).unwrap(),
            format!("Product {n}"),
            vid(n + 10),
            base,
        )
        .unwrap();
        p.set_status(ProductStatus::Active).unwrap();
        p
    }

    /// An EUR market whose catalog (no price list) includes `products`.
    fn pricing(products: &[u128]) -> PricingContext {
        PricingContext {
            market: Market::new(mid(), "Eurozone".to_string(), vec![hr()], Currency::EUR).unwrap(),
            catalog: Some(Catalog::new(
                CatalogId::from_uuid(Uuid::from_u128(200)),
                CatalogContext::Market(mid()),
                None,
            )),
            price_lists: Vec::new(),
            included_products: products.iter().map(|n| pid(*n)).collect::<HashSet<_>>(),
            fixed_prices: HashMap::new(),
            product_categories: HashMap::new(),
            at: Timestamp::UNIX_EPOCH,
            groups: Vec::new(),
        }
    }

    fn no_tax() -> TaxContext {
        TaxContext {
            region: None,
            prices_include_tax: false,
            product_classes: HashMap::new(),
        }
    }

    /// Croatia at 25%.
    fn croatia(prices_include_tax: bool) -> TaxContext {
        TaxContext {
            region: Some(TaxRegion::new(
                TaxRegionId::from_uuid(Uuid::from_u128(500)),
                hr(),
                TaxRate::new(250_000).unwrap(),
            )),
            prices_include_tax,
            product_classes: HashMap::new(),
        }
    }

    fn cart(lines: &[(u128, u32)]) -> Cart {
        let mut c = Cart::new(
            CartId::from_uuid(Uuid::from_u128(300)),
            mid(),
            hr(),
            ChannelId::from_uuid(Uuid::from_u128(500)),
        );
        for (n, q) in lines {
            c.add_line(pid(*n), vid(n + 10), Quantity::new(*q).unwrap())
                .unwrap();
        }
        c
    }

    fn discounted(discount: Discount) -> DiscountReading {
        DiscountReading {
            context: DiscountContext {
                automatic: vec![discount],
                ..DiscountContext::default()
            },
            ..DiscountReading::default()
        }
    }

    fn percent_off(target: Target, bp: i32) -> Discount {
        let kind = Kind::Standard {
            rewards: vec![Reward {
                target,
                value: Value::Percent(BasisPoints::new(bp).unwrap()),
            }],
        };
        Discount::new(
            hypha_core::DiscountId::from_uuid(Uuid::from_u128(700)),
            DiscountDraft::new("Offer", Method::Automatic, kind),
        )
        .unwrap()
    }

    fn priced_with(
        cart: &Cart,
        products: &[Product],
        tax: &TaxContext,
        discounts: &DiscountReading,
    ) -> PricedCart {
        let published = Publication::new(products.iter().map(|p| p.id()).collect());
        price_cart_with_shipping(
            cart,
            products,
            &pricing(&[1, 2]),
            tax,
            &StockContext::default(),
            &ShippingContext::default(),
            &published,
            discounts,
        )
    }

    #[test]
    fn an_order_discount_lowers_each_lines_total_and_tax_is_on_whats_left() {
        let products = [product(1, eur(1000)), product(2, eur(250))];
        let result = priced_with(
            &cart(&[(1, 2), (2, 3)]),
            &products,
            &croatia(false),
            &discounted(percent_off(Target::Subtotal, 1000)),
        );
        // 10% of 27.50 is 2.75, spread 20.00 : 7.50 as 2.00 and 0.75.
        let totals: Vec<(Money, Money, Option<Money>)> = result
            .lines
            .iter()
            .map(|l| match &l.status {
                LineStatus::Priced {
                    subtotal,
                    total,
                    tax,
                    ..
                } => (*subtotal, *total, tax.map(|t| t.amount)),
                LineStatus::Unavailable(_) => panic!("unavailable"),
            })
            .collect();
        // 25% of 18.00 is 4.50; of 6.75, 1.6875 → 1.69.
        assert_eq!(
            totals,
            vec![
                (money(2000), money(1800), Some(money(450))),
                (money(750), money(675), Some(money(169)))
            ]
        );
        assert_eq!(
            (
                result.discount_total,
                result.subtotal,
                result.tax_total,
                result.total
            ),
            (money(275), money(2475), money(619), money(3094))
        );
        assert_eq!(result.discounts.len(), 1);
    }

    #[test]
    fn included_tax_is_backed_out_of_the_discounted_amount() {
        let result = priced_with(
            &cart(&[(1, 2)]),
            &[product(1, eur(1000))],
            &croatia(true),
            &discounted(percent_off(Target::Items(ItemSet::all()), 1000)),
        );
        // 20.00 less 10% is 18.00, of which 25% included tax is 3.60.
        assert_eq!(
            (result.subtotal, result.tax_total, result.total),
            (money(1800), money(360), money(1800))
        );
    }

    #[test]
    fn an_unavailable_line_takes_no_discount() {
        let mut archived = product(2, eur(250));
        archived.set_status(ProductStatus::Archived).unwrap();
        let result = priced_with(
            &cart(&[(1, 1), (2, 1)]),
            &[product(1, eur(1000)), archived],
            &no_tax(),
            &discounted(percent_off(Target::Subtotal, 1000)),
        );
        assert_eq!(result.discount_total, money(100));
        assert_eq!(result.total, money(900));
    }

    #[test]
    fn a_code_nothing_knows_reads_as_invalid() {
        let mut c = cart(&[(1, 1)]);
        c.add_discount_code("GONE".to_string()).unwrap();
        let result = priced_with(
            &c,
            &[product(1, eur(1000))],
            &no_tax(),
            &DiscountReading::default(),
        );
        assert_eq!(
            result.discount_codes,
            vec![CartCodeOutcome {
                code: "GONE".to_string(),
                state: CartCodeState::Invalid
            }]
        );
        assert_eq!(result.total, money(1000));
    }
}
