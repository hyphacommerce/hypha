//! What happens to fulfilments, as events. Contract types: a reshaped field is a new
//! `VERSION`, never an edit.

use hypha_core::{Event, FulfillmentId, LocationId, OrderId, OrderLineId};
use serde::Serialize;

pub const FULFILLMENT: &str = "fulfillment";

/// A line and how many of it left.
#[derive(Debug, Serialize)]
pub struct ShippedLine {
    pub line: OrderLineId,
    pub quantity: u32,
}

/// A tracking number, with its carrier and URL when known.
#[derive(Debug, Serialize)]
pub struct TrackingData {
    pub carrier: Option<String>,
    pub number: String,
    pub url: Option<String>,
}

/// `fulfillment.created`: these lines of the order left `location`.
#[derive(Debug, Serialize)]
pub struct FulfillmentCreated {
    pub fulfillment: FulfillmentId,
    pub order: OrderId,
    pub location: LocationId,
    pub lines: Vec<ShippedLine>,
    pub tracking: Vec<TrackingData>,
    /// Whether staff chose to email the buyer a shipping confirmation: the `Mailer` sends
    /// `order_shipped` only then. An added field, so still version 1.
    pub notify_customer: bool,
}

impl Event for FulfillmentCreated {
    const TYPE: &'static str = "fulfillment.created";
    const VERSION: u16 = 1;
}

/// `fulfillment.updated`: its tracking changed; `tracking` is all of it now.
#[derive(Debug, Serialize)]
pub struct FulfillmentUpdated {
    pub fulfillment: FulfillmentId,
    pub order: OrderId,
    pub tracking: Vec<TrackingData>,
    /// Whether staff chose to email the buyer the new tracking: the `Mailer` sends
    /// `order_tracking_updated` only then. An added field, so still version 1.
    pub notify_customer: bool,
}

impl Event for FulfillmentUpdated {
    const TYPE: &'static str = "fulfillment.updated";
    const VERSION: u16 = 1;
}

/// `fulfillment.cancelled`: it was recorded by mistake; its units are back in stock and
/// its lines count as unfulfilled again.
#[derive(Debug, Serialize)]
pub struct FulfillmentCancelled {
    pub fulfillment: FulfillmentId,
    pub order: OrderId,
}

impl Event for FulfillmentCancelled {
    const TYPE: &'static str = "fulfillment.cancelled";
    const VERSION: u16 = 1;
}
