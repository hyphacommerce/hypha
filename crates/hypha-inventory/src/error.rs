use hypha_core::{
    AddressError, ErrorCode, LocationId, MarketId, QuantityError, TextError, VariantId,
};
use thiserror::Error;

use crate::{MAX_STOCK, ReservationRef};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum InventoryError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Address(#[from] AddressError),
    #[error("market {0} is listed more than once")]
    DuplicateMarket(MarketId),
    #[error("stock must be between -{MAX_STOCK} and {MAX_STOCK}, got {0}")]
    StockOutOfRange(i128),
    #[error("on hand at this location is {actual}, not the expected {expected}")]
    OnHandChanged { expected: i64, actual: i64 },
    #[error(transparent)]
    Quantity(#[from] QuantityError),
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
    /// Deleting an item open orders have units reserved on.
    #[error("units are still reserved")]
    StillReserved,
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
            InventoryError::Text(e) => e.code(),
            InventoryError::Address(e) => e.code(),
            InventoryError::DuplicateMarket(_) => "duplicate_market",
            InventoryError::StockOutOfRange(_) => "stock_out_of_range",
            InventoryError::OnHandChanged { .. } => "on_hand_changed",
            InventoryError::Quantity(e) => e.code(),
            InventoryError::InvalidReference => "invalid_reservation_reference",
            InventoryError::ReferenceInUse(_) => "reservation_reference_in_use",
            InventoryError::OutOfStock { .. } => "out_of_stock",
            InventoryError::DuplicateLine { .. } => "duplicate_reservation_line",
            InventoryError::StillReserved => "inventory_still_reserved",
        }
    }
}
