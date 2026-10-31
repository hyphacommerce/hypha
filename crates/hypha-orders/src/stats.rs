//! What a customer's orders add up to: shown on the customer's page and, later, what
//! segments filter on. A read model like `OrderSummary`: adapters may compute it in
//! storage, and the repository contract checks it equals `OrderStats::of` the stored
//! orders.

use hypha_core::{Money, Timestamp};

use crate::Order;

/// A customer's orders that weren't cancelled, added up. Totals are per currency, since
/// markets mean a customer may have paid in several; they come sorted by currency code.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderStats {
    pub orders_count: u64,
    /// Cancelled orders, counted apart: they add up to nothing, but a customer with any
    /// order can't be deleted, only erased.
    pub cancelled_count: u64,
    pub first_order_at: Option<Timestamp>,
    pub last_order_at: Option<Timestamp>,
    pub amount_spent: Vec<Money>,
}

impl OrderStats {
    /// The statistics over `orders` (a customer's), leaving cancelled ones out. A sum
    /// past `Money::MAX_AMOUNT` is capped there rather than failing: a statistic
    /// shouldn't make the customer unreadable.
    pub fn of<'a>(orders: impl IntoIterator<Item = &'a Order>) -> Self {
        let mut stats = Self::default();
        for order in orders {
            if order.cancelled().is_some() {
                stats.cancelled_count += 1;
                continue;
            }
            stats.orders_count += 1;
            let at = order.placed_at();
            stats.first_order_at = Some(stats.first_order_at.map_or(at, |t| t.min(at)));
            stats.last_order_at = Some(stats.last_order_at.map_or(at, |t| t.max(at)));
            let total = order.total();
            match stats
                .amount_spent
                .iter_mut()
                .find(|m| m.currency() == total.currency())
            {
                Some(sum) => {
                    *sum = sum.checked_add(total).unwrap_or_else(|_| {
                        Money::new(Money::MAX_AMOUNT, sum.currency()).unwrap_or(*sum)
                    });
                }
                None => stats.amount_spent.push(total),
            }
        }
        stats
            .amount_spent
            .sort_by(|a, b| a.currency().as_str().cmp(b.currency().as_str()));
        stats
    }
}
