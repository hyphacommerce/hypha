//! What happens to shipping settings, as events. Contract types: a reshaped field is a new
//! `VERSION`, never an edit.

use hypha_core::{Event, ShippingOptionId, ShippingZoneId};
use serde::Serialize;

pub const SHIPPING_ZONE: &str = "shipping_zone";

/// `shipping_zone.changed`: created, renamed, its countries or options changed.
#[derive(Debug, Serialize)]
pub struct ShippingZoneChanged {
    pub shipping_zone: ShippingZoneId,
}

impl Event for ShippingZoneChanged {
    const TYPE: &'static str = "shipping_zone.changed";
    const VERSION: u16 = 1;
}

/// `shipping_zone.deleted`.
#[derive(Debug, Serialize)]
pub struct ShippingZoneDeleted {
    pub shipping_zone: ShippingZoneId,
}

impl Event for ShippingZoneDeleted {
    const TYPE: &'static str = "shipping_zone.deleted";
    const VERSION: u16 = 1;
}

/// `shipping_option.deleted`: an option removed from its zone, or deleted with it (one each,
/// beside the zone's `shipping_zone.deleted`). About the zone, whose write it is.
#[derive(Debug, Serialize)]
pub struct ShippingOptionDeleted {
    pub shipping_option: ShippingOptionId,
    pub shipping_zone: ShippingZoneId,
}

impl Event for ShippingOptionDeleted {
    const TYPE: &'static str = "shipping_option.deleted";
    const VERSION: u16 = 1;
}
