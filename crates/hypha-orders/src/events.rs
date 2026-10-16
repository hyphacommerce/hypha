//! What happens to orders, as events (design note 0016, section 4). Contract types: a
//! reshaped field is a new `VERSION`, never an edit.

use hypha_core::{CustomerId, Event, Money, OrderId};
use serde::Serialize;

pub const ORDER: &str = "order";

/// `order.placed`: a cart was checked out. When is the event's `occurred_at`.
#[derive(Debug, Serialize)]
pub struct OrderPlaced {
    pub order: OrderId,
    pub number: i64,
    pub customer: CustomerId,
    pub total: Money,
}

impl Event for OrderPlaced {
    const TYPE: &'static str = "order.placed";
    const VERSION: u16 = 1;
}

/// `order.paid`: the order's whole total was paid. Payment details (a reference, the
/// provider) come with payment spores, as a new version.
#[derive(Debug, Serialize)]
pub struct OrderPaid {
    pub order: OrderId,
    pub amount: Money,
}

impl Event for OrderPaid {
    const TYPE: &'static str = "order.paid";
    const VERSION: u16 = 1;
}

/// `order.fulfillment_status_changed`: a fulfilment (or its cancellation) moved the
/// order between `unfulfilled`, `partially_fulfilled` and `fulfilled` (design note
/// 0018).
#[derive(Debug, Serialize)]
pub struct OrderFulfillmentStatusChanged {
    pub order: OrderId,
    pub from: &'static str,
    pub to: &'static str,
}

impl Event for OrderFulfillmentStatusChanged {
    const TYPE: &'static str = "order.fulfillment_status_changed";
    const VERSION: u16 = 1;
}

/// `order.cancelled`: the order won't ship; its stock was given back. `reason` is one
/// of `customer`, `inventory`, `fraud`, `declined`, `staff`, `other`.
#[derive(Debug, Serialize)]
pub struct OrderCancelled {
    pub order: OrderId,
    pub reason: &'static str,
}

impl Event for OrderCancelled {
    const TYPE: &'static str = "order.cancelled";
    const VERSION: u16 = 1;
}
