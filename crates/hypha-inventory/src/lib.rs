//! Stock per location, and reservations against it (design note 0004).
//!
//! A leaf domain: it depends on core only and links to variants and markets by ID.

pub mod commands;
pub mod error;
pub mod item;
pub mod location;
pub mod reference;
pub mod repo;
pub mod stock;

pub use commands::{
    AdjustOnHand, CommandError, CreateInventoryItem, CreateLocation, MAX_ATTEMPTS, ReleaseLine,
    ReleaseStock, ReserveLine, ReserveStock, SetOnHand, Shortage, UpdateInventoryItem,
    UpdateLocation, adjust_on_hand, create_inventory_item, create_location, get_inventory_item,
    get_location, get_variant_inventory, release_stock, reserve_lines, reserve_stock, set_on_hand,
    update_inventory_item, update_location,
};
pub use error::{InventoryError, RestoreError};
pub use item::{InventoryItem, InventoryItemSnapshot, Level, MAX_STOCK, Reservation};
pub use location::{Location, LocationSnapshot};
pub use reference::ReservationRef;
pub use repo::{
    HasInventoryItems, HasLocations, InventoryItemRepo, LocationRepo, StageInventoryItems,
};
pub use stock::{Availability, StockContext};
