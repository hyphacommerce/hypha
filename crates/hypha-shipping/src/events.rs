//! What happens to shipping settings, as events (design note 0016, section 4).
//! Contract types: a reshaped field is a new `VERSION`, never an edit.

use hypha_core::{Event, ShippingZoneId};
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
