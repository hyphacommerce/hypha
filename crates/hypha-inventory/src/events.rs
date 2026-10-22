//! What happens to stock, as events. Contract types: a reshaped field is a new `VERSION`,
//! never an edit.

use hypha_core::{Event, InventoryItemId, LocationId, VariantId};
use serde::Serialize;

pub const INVENTORY_ITEM: &str = "inventory_item";
pub const LOCATION: &str = "location";

/// `location.changed`: created, or which of `name`, `markets`, `priority` and `address`
/// changed (all four when created). Its markets decide where its stock counts, so search
/// rebuilds when they change.
#[derive(Debug, Serialize)]
pub struct LocationChanged {
    pub location: LocationId,
    pub fields: Vec<&'static str>,
}

impl Event for LocationChanged {
    const TYPE: &'static str = "location.changed";
    const VERSION: u16 = 1;
}

/// `inventory_item.changed`: stock counted or adjusted, reserved or released, or the
/// item's settings changed. Carries availability after the change, so a consumer
/// (search's "in stock") needn't fetch the item. Fires on every checkout of a tracked
/// item: take the latest per item (the subject version orders them).
#[derive(Debug, Serialize)]
pub struct InventoryItemChanged {
    pub inventory_item: InventoryItemId,
    pub variant: VariantId,
    pub tracked: bool,
    pub continue_selling: bool,
    /// `on_hand − reserved` per location with a level or a reservation.
    pub available: Vec<Available>,
}

#[derive(Debug, Serialize)]
pub struct Available {
    pub location: LocationId,
    pub available: i64,
}

impl Event for InventoryItemChanged {
    const TYPE: &'static str = "inventory_item.changed";
    const VERSION: u16 = 1;
}

/// `inventory_item.deleted`: its variant was removed, and nothing was reserved.
#[derive(Debug, Serialize)]
pub struct InventoryItemDeleted {
    pub inventory_item: InventoryItemId,
    pub variant: VariantId,
}

impl Event for InventoryItemDeleted {
    const TYPE: &'static str = "inventory_item.deleted";
    const VERSION: u16 = 1;
}
