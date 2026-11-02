//! Stock per location, and reservations against it.
//!
//! A leaf domain: it depends on core only and links to variants and markets by ID.

pub mod commands;
pub mod error;
pub mod events;
pub mod item;
pub mod location;
pub mod movement;
pub mod reference;
pub mod repo;
pub mod stock;
pub mod summary;
pub mod transfer;
pub mod transfer_commands;

pub use commands::{
    AdjustOnHand, CommandError, CreateInventoryItem, CreateLocation, DEFAULT_LOCATION_NAME,
    SetOnHand, SetUnitCost, Shortage, UpdateInventoryItem, UpdateLocation, adjust_on_hand,
    check_cost_currency, create_inventory_item, create_location, ensure_default_location,
    get_inventory_item, get_location, get_variant_inventory, get_variants_inventory,
    list_inventory_items, list_locations, list_stock_movements, reserve_lines, set_on_hand,
    set_unit_cost, update_inventory_item, update_location,
};
pub use error::{InventoryError, RestoreError};
pub use events::{
    Available, CostChange, InventoryItemChanged, InventoryItemCostChanged, InventoryItemDeleted,
    LocationChanged, TransferCancelled, TransferCreated, TransferLineReceived, TransferReceived,
    TransferShipped, TransferUpdated,
};
pub use item::{InventoryItem, InventoryItemSnapshot, Level, MAX_STOCK, Reservation};
pub use location::{Location, LocationSnapshot};
pub use movement::{
    AdjustmentReason, Cause, MovementFilter, MovementNote, StockMovement, StockMovementEntry,
    StockReason, StockSource,
};
pub use reference::ReservationRef;
pub use repo::{
    HasInventoryItems, HasLocations, HasStockMovements, HasTransfers, InventoryItemRepo,
    LocationRepo, StageInventoryItems, StageTransfers, StockMovementRepo, TransferRepo,
};
pub use stock::{Availability, StockContext};
pub use summary::{InventoryFilter, InventoryItemSummary, LevelSummary, stock_by_location};
pub use transfer::{
    InTransit, IncomingTransfer, MAX_TRANSFER_LINES, MAX_TRANSFER_NOTE_CHARS,
    MAX_TRANSFER_QUANTITY, Transfer, TransferArrival, TransferDraft, TransferFilter, TransferLine,
    TransferLineDraft, TransferLineSnapshot, TransferNumber, TransferSnapshot, TransferStatus,
    TransferSummary, incoming_transfers,
};
pub use transfer_commands::{
    CreateTransfer, RECEIVE_TRANSFER, SHIP_TRANSFER, TransferPorts, UpdateTransfer,
    cancel_transfer, create_transfer, delete_transfer, get_transfer, list_transfers,
    receive_transfer, ship_transfer, update_transfer,
};
