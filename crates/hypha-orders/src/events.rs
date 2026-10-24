//! What happens to orders, as events. Contract types: a reshaped field is a new `VERSION`,
//! never an edit.

use hypha_core::{ChannelId, CustomerId, Event, Money, OrderId};
use serde::Serialize;

pub const ORDER: &str = "order";

/// `order.placed`: a cart was checked out. When is the event's `occurred_at`.
#[derive(Debug, Serialize)]
pub struct OrderPlaced {
    pub order: OrderId,
    pub number: i64,
    pub customer: CustomerId,
    /// Where it was placed: an added field, so still version 1.
    pub channel: ChannelId,
    pub total: Money,
    /// The discount codes it used, normalised: an added field.
    pub discount_codes: Vec<String>,
    /// What discounts took off it: an added field.
    pub discount_total: Money,
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

/// `order.fulfillment_status_changed`: a fulfilment (or its cancellation) moved the order
/// between `unfulfilled`, `partially_fulfilled` and `fulfilled`.
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
    /// Whether staff chose to email the buyer about it: the `Mailer` sends
    /// `order_cancelled` only then. An added field, so still version 1.
    pub notify_customer: bool,
}

impl Event for OrderCancelled {
    const TYPE: &'static str = "order.cancelled";
    const VERSION: u16 = 1;
}
