//! What orders placed in a period add up to, in buckets of time: the sales report's
//! figures. A read model like `OrderStats`: adapters compute it in storage, and the
//! repository contract checks it equals `SalesFigures::of` the stored orders (and, for
//! refunds, their payments).
//!
//! The buckets' starts come from the caller (a workflow that knows the store's time zone),
//! so storage only compares instants. Amounts are added per currency, never across
//! currencies. Cancelled orders are counted apart and add nothing.

use std::collections::BTreeMap;

use hypha_core::{ChannelId, Currency, MarketId, Money, OrderId, ProductId, Timestamp};

use crate::{FinancialStatus, Order};

/// Which orders, and how they're bucketed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesQuery {
    /// Where each bucket starts, ascending, with no repeats: the first is the period's
    /// start, and each bucket runs until the next start (the last until `end`).
    pub starts: Vec<Timestamp>,
    /// The period's end, after the last start; orders placed at it are left out.
    pub end: Timestamp,
    pub channel: Option<ChannelId>,
    pub market: Option<MarketId>,
    /// How many products each top list holds.
    pub top: u32,
}

impl SalesQuery {
    /// At least one start, ascending, all before `end`. Adapters refuse any other query
    /// with a `Backend` error.
    pub fn is_valid(&self) -> bool {
        !self.starts.is_empty()
            && self.starts.windows(2).all(|w| w[0] < w[1])
            && self.starts.last().is_some_and(|last| *last < self.end)
    }

    /// The bucket an order placed at `at` falls in, if it's in the period.
    pub fn bucket(&self, at: Timestamp) -> Option<usize> {
        if at >= self.end {
            return None;
        }
        self.starts.iter().rposition(|start| *start <= at)
    }

    /// Whether `order` is one this query counts (cancelled or not).
    pub fn counts(&self, order: &Order) -> bool {
        self.bucket(order.placed_at()).is_some()
            && self.channel.is_none_or(|c| c == order.channel())
            && self.market.is_none_or(|m| m == order.market())
    }
}

/// One currency's sales in a bucket. Every amount is the orders' own, added up: `subtotal`
/// is the line totals (tax included when the market's prices include it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrencySales {
    pub currency: Currency,
    pub orders_count: u64,
    pub subtotal: Money,
    pub shipping: Money,
    pub taxes: Money,
    pub total: Money,
    /// What the orders' payments gave back, added up (`partially_refunded` and
    /// `refunded` orders).
    pub refunded: Money,
}

/// One bucket: the currencies with any order that wasn't cancelled, by currency code,
/// and how many were cancelled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BucketFigures {
    pub sales: Vec<CurrencySales>,
    pub cancelled_count: u64,
}

/// A product's place in a top list. `title` is its line's title in the newest order
/// counted (titles are copies, so a renamed product shows its latest name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductUnits {
    pub product: ProductId,
    pub title: String,
    pub units: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductRevenue {
    pub product: ProductId,
    pub title: String,
    pub units: u64,
    /// Its line totals, added up.
    pub revenue: Money,
}

/// The products that sold most in one currency, by revenue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrencyTop {
    pub currency: Currency,
    pub products: Vec<ProductRevenue>,
}

/// Orders that weren't cancelled, in the period: summed per bucket and currency, with the
/// top products. A sum past `Money::MAX_AMOUNT` is capped there, like `OrderStats`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SalesFigures {
    /// One per start, in order, empty buckets too.
    pub buckets: Vec<BucketFigures>,
    /// Most units first, then by product ID; at most `top`. Units count in any currency.
    pub top_by_units: Vec<ProductUnits>,
    /// Per currency, by currency code: most revenue first, then by product ID; at most
    /// `top` each.
    pub top_by_revenue: Vec<CurrencyTop>,
}

/// Sums in `i128`, capped at `Money::MAX_AMOUNT` when read.
#[derive(Default)]
struct Sums {
    orders_count: u64,
    subtotal: i128,
    shipping: i128,
    taxes: i128,
    total: i128,
    refunded: i128,
}

/// A bucket's sums by currency code, and how many of its orders were cancelled.
type BucketSums = (BTreeMap<&'static str, (Currency, Sums)>, u64);

#[derive(Default)]
struct ProductSums {
    units: u64,
    revenue: i128,
    /// The newest order's title: (placed at, order) decides.
    title: Option<(Timestamp, OrderId, String)>,
}

impl ProductSums {
    fn add(&mut self, order: &Order, units: u64, revenue: i64, title: &str) {
        self.units += units;
        self.revenue += i128::from(revenue);
        let at = (order.placed_at(), order.id());
        if self.title.as_ref().is_none_or(|(t, o, _)| at > (*t, *o)) {
            self.title = Some((at.0, at.1, title.to_string()));
        }
    }

    fn title(&self) -> String {
        self.title
            .as_ref()
            .map(|(_, _, title)| title.clone())
            .unwrap_or_default()
    }
}

/// `amount` minor units of `currency`, capped at `Money::MAX_AMOUNT`. The sums are of
/// non-negative stored amounts, so the cap is the only way out of range.
pub fn capped(amount: i128, currency: Currency) -> Money {
    let amount = amount.clamp(0, i128::from(Money::MAX_AMOUNT));
    let amount = i64::try_from(amount).unwrap_or(Money::MAX_AMOUNT);
    Money::new(amount, currency).unwrap_or_else(|_| Money::zero(currency))
}

impl SalesFigures {
    /// What `query` reads from `orders`, given what each order's payments gave back
    /// (`refunded`, in its currency's minor units): the reference every adapter is held
    /// to.
    pub fn of<'a>(
        orders: impl IntoIterator<Item = &'a Order>,
        refunded: impl Fn(OrderId) -> i64,
        query: &SalesQuery,
    ) -> Self {
        let mut buckets: Vec<BucketSums> = (0..query.starts.len())
            .map(|_| Default::default())
            .collect();
        let mut units: BTreeMap<ProductId, ProductSums> = BTreeMap::new();
        let mut revenue: BTreeMap<(&'static str, ProductId), (Currency, ProductSums)> =
            BTreeMap::new();
        for order in orders {
            let Some(bucket) = query.bucket(order.placed_at()) else {
                continue;
            };
            if !query.counts(order) {
                continue;
            }
            let (sums, cancelled) = &mut buckets[bucket];
            if order.cancelled().is_some() {
                *cancelled += 1;
                continue;
            }
            let currency = order.total().currency();
            let (_, s) = sums
                .entry(currency.as_str())
                .or_insert_with(|| (currency, Sums::default()));
            s.orders_count += 1;
            s.subtotal += i128::from(order.subtotal().amount());
            s.shipping += i128::from(order.shipping_total().amount());
            s.taxes += i128::from(order.tax_total().amount());
            s.total += i128::from(order.total().amount());
            for line in order.lines() {
                let quantity = u64::from(line.quantity.value());
                let amount = line.total.amount();
                units.entry(line.product).or_default().add(
                    order,
                    quantity,
                    amount,
                    &line.product_title,
                );
                revenue
                    .entry((currency.as_str(), line.product))
                    .or_insert_with(|| (currency, ProductSums::default()))
                    .1
                    .add(order, quantity, amount, &line.product_title);
            }
            if matches!(
                order.financial_status(),
                FinancialStatus::PartiallyRefunded | FinancialStatus::Refunded
            ) {
                s.refunded += i128::from(refunded(order.id()));
            }
        }

        let top = query.top as usize;
        let mut by_units: Vec<ProductUnits> = units
            .iter()
            .map(|(product, sums)| ProductUnits {
                product: *product,
                title: sums.title(),
                units: sums.units,
            })
            .collect();
        by_units.sort_by(|a, b| b.units.cmp(&a.units).then(a.product.cmp(&b.product)));
        by_units.truncate(top);

        let mut top_by_revenue: Vec<CurrencyTop> = Vec::new();
        for ((_, product), (currency, sums)) in &revenue {
            let entry = ProductRevenue {
                product: *product,
                title: sums.title(),
                units: sums.units,
                revenue: capped(sums.revenue, *currency),
            };
            match top_by_revenue.last_mut() {
                Some(top) if top.currency == *currency => top.products.push(entry),
                _ => top_by_revenue.push(CurrencyTop {
                    currency: *currency,
                    products: vec![entry],
                }),
            }
        }
        for currency in &mut top_by_revenue {
            currency.products.sort_by(|a, b| {
                b.revenue
                    .amount()
                    .cmp(&a.revenue.amount())
                    .then(a.product.cmp(&b.product))
            });
            currency.products.truncate(top);
        }

        SalesFigures {
            buckets: buckets
                .into_iter()
                .map(|(sums, cancelled_count)| BucketFigures {
                    sales: sums
                        .into_values()
                        .map(|(currency, s)| CurrencySales {
                            currency,
                            orders_count: s.orders_count,
                            subtotal: capped(s.subtotal, currency),
                            shipping: capped(s.shipping, currency),
                            taxes: capped(s.taxes, currency),
                            total: capped(s.total, currency),
                            refunded: capped(s.refunded, currency),
                        })
                        .collect(),
                    cancelled_count,
                })
                .collect(),
            top_by_units: by_units,
            top_by_revenue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(us: i64) -> Timestamp {
        Timestamp::from_unix_micros(us)
    }

    fn query(starts: &[i64], end: i64) -> SalesQuery {
        SalesQuery {
            starts: starts.iter().map(|s| at(*s)).collect(),
            end: at(end),
            channel: None,
            market: None,
            top: 5,
        }
    }

    #[test]
    fn a_query_needs_ascending_starts_before_its_end() {
        assert!(query(&[0, 10], 20).is_valid());
        assert!(!query(&[], 20).is_valid());
        assert!(!query(&[10, 10], 20).is_valid());
        assert!(!query(&[10, 0], 20).is_valid());
        assert!(!query(&[0, 20], 20).is_valid());
    }

    #[test]
    fn a_bucket_starts_at_its_start_and_ends_before_the_next() {
        let q = query(&[0, 10], 20);
        assert_eq!(q.bucket(at(-1)), None);
        assert_eq!(q.bucket(at(0)), Some(0));
        assert_eq!(q.bucket(at(9)), Some(0));
        assert_eq!(q.bucket(at(10)), Some(1));
        assert_eq!(q.bucket(at(19)), Some(1));
        assert_eq!(q.bucket(at(20)), None);
    }

    #[test]
    fn sums_past_the_largest_amount_are_capped() {
        let eur = Currency::new("EUR").unwrap();
        assert_eq!(
            capped(i128::from(Money::MAX_AMOUNT) + 1, eur),
            Money::new(Money::MAX_AMOUNT, eur).unwrap()
        );
        assert_eq!(capped(5, eur), Money::new(5, eur).unwrap());
    }
}
