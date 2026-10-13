use hypha_core::{ErrorCode, LocationId, MarketId, VariantId};
use thiserror::Error;

use crate::{MAX_STOCK, ReservationRef};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum InventoryError {
    #[error("location name must not be empty")]
    EmptyLocationName,
    #[error("market {0} is listed more than once")]
    DuplicateMarket(MarketId),
    #[error("stock must be between -{MAX_STOCK} and {MAX_STOCK}, got {0}")]
    StockOutOfRange(i128),
    #[error("on hand at this location is {actual}, not the expected {expected}")]
    OnHandChanged { expected: i64, actual: i64 },
    #[error("quantity must be at least 1")]
    ZeroQuantity,
    #[error("reservation reference must be 1 to {max} characters with no control characters", max = ReservationRef::MAX_LEN)]
    InvalidReference,
    #[error("{0} is already reserved for a different quantity")]
    ReferenceInUse(ReservationRef),
    #[error("only {available} of {requested} in stock")]
    OutOfStock { requested: u32, available: i64 },
    #[error("{reference} for variant {variant} is listed more than once")]
    DuplicateLine {
        reference: ReservationRef,
        variant: VariantId,
    },
}

/// Why stored data can't be rebuilt into an inventory item. Only
/// `InventoryItem::restore` returns it and adapters report it as a backend failure, so it
/// has no API code (review plan D4).
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RestoreError {
    #[error(transparent)]
    Inventory(#[from] InventoryError),
    #[error("location {0} has more than one stock level")]
    DuplicateLevel(LocationId),
    #[error("{reference} has more than one reservation at location {location}")]
    DuplicateReservation {
        reference: ReservationRef,
        location: LocationId,
    },
}

impl ErrorCode for InventoryError {
    fn code(&self) -> &'static str {
        match self {
            InventoryError::EmptyLocationName => "empty_location_name",
            InventoryError::DuplicateMarket(_) => "duplicate_market",
            InventoryError::StockOutOfRange(_) => "stock_out_of_range",
            InventoryError::OnHandChanged { .. } => "on_hand_changed",
            InventoryError::ZeroQuantity => "invalid_quantity",
            InventoryError::InvalidReference => "invalid_reservation_reference",
            InventoryError::ReferenceInUse(_) => "reservation_reference_in_use",
            InventoryError::OutOfStock { .. } => "out_of_stock",
            InventoryError::DuplicateLine { .. } => "duplicate_reservation_line",
        }
    }
}
