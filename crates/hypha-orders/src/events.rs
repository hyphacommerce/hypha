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
