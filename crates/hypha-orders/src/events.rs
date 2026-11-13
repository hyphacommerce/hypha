//! What happens to orders, as events. Contract types: a reshaped field is a new `VERSION`,
//! never an edit.

use hypha_core::{Address, ChannelId, CustomerId, Event, Language, Money, OrderId};
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
    /// The language the buyer checked out in (a BCP 47 tag): an added field.
    pub language: Option<Language>,
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

/// `order.address_marked_valid`: staff looked at the shipping address and said it's fine.
/// `by` is the actor's prefixed ID.
#[derive(Debug, Serialize)]
pub struct OrderAddressMarkedValid {
    pub order: OrderId,
    pub by: String,
}

impl Event for OrderAddressMarkedValid {
    const TYPE: &'static str = "order.address_marked_valid";
    const VERSION: u16 = 1;
}

/// An address in an event: every field, which webhooks redact per the receiving app's
/// grant like any other.
#[derive(Debug, Serialize)]
pub struct ShippingAddressPayload {
    pub first_name: Option<String>,
    pub last_name: String,
    pub company: Option<String>,
    pub line1: String,
    pub line2: Option<String>,
    pub city: String,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub country: String,
    pub phone: Option<String>,
}

impl From<&Address> for ShippingAddressPayload {
    fn from(a: &Address) -> Self {
        let p = a.parts();
        Self {
            first_name: p.first_name.clone(),
            last_name: p.last_name.clone(),
            company: p.company.clone(),
            line1: p.line1.clone(),
            line2: p.line2.clone(),
            city: p.city.clone(),
            province: p.province.clone(),
            postal_code: p.postal_code.clone(),
            country: p.country.clone(),
            phone: p.phone.clone(),
        }
    }
}

/// `order.shipping_address_changed`: staff changed where the order ships, before any of
/// it did. Shipping wasn't priced again.
#[derive(Debug, Serialize)]
pub struct OrderShippingAddressChanged {
    pub order: OrderId,
    pub address: ShippingAddressPayload,
}

impl Event for OrderShippingAddressChanged {
    const TYPE: &'static str = "order.shipping_address_changed";
    const VERSION: u16 = 1;
}
