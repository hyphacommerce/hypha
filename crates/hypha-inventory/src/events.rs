//! What happens to stock, as events (design note 0016, section 4). Contract types: a
//! reshaped field is a new `VERSION`, never an edit.

use hypha_core::{Event, InventoryItemId, LocationId, VariantId};
use serde::Serialize;

pub const INVENTORY_ITEM: &str = "inventory_item";

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
