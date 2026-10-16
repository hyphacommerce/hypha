//! What a list of orders shows (design note 0012, L7): no lines, so a page costs the
//! same however many lines its orders have. A read model, not an aggregate: adapters
//! may compute it in storage, and the repository contract checks it equals
//! `Order::summary`.

use hypha_core::{ChannelId, CountryCode, CustomerId, Email, Money, OrderId, Timestamp};

use crate::{FinancialStatus, FulfillmentStatus, Order, OrderNumber};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderSummary {
    pub id: OrderId,
    pub number: OrderNumber,
    pub placed_at: Timestamp,
    /// The sales channel it was placed through (design note 0020).
    pub channel: ChannelId,
    pub customer: CustomerId,
    pub email: Email,
    pub country: CountryCode,
    pub financial_status: FinancialStatus,
    pub fulfillment_status: FulfillmentStatus,
    /// When it was cancelled, if it was (design note 0018).
    pub cancelled_at: Option<Timestamp>,
    pub total: Money,
    /// How many items, adding up line quantities.
    pub item_count: u64,
}

impl Order {
    pub fn summary(&self) -> OrderSummary {
        OrderSummary {
            id: self.id(),
            number: self.number(),
            placed_at: self.placed_at(),
            channel: self.channel(),
            customer: self.customer(),
            email: self.email().clone(),
            country: self.country(),
            financial_status: self.financial_status(),
            fulfillment_status: self.fulfillment_status(),
            cancelled_at: self.cancelled().map(|c| c.at),
            total: self.total(),
            item_count: self
                .lines()
                .iter()
                .map(|l| u64::from(l.quantity.value()))
                .sum(),
        }
    }
}
