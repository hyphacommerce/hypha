//! The stock ledger: one row per change to a count, recorded by the item that changed and
//! written by the adapter in the same transaction, the way events are. No write can change
//! stock without leaving a row, so "why is this 4 and not 12?" always has an answer, and
//! the deltas at a location add up to its `on_hand`.

use std::fmt;

use hypha_core::{
    Actor, FulfillmentId, InventoryItemId, LocationId, OrderId, PurchaseOrderId, ReceiptId,
    Timestamp, TransferId, UnitCost, VariantId,
};

use crate::InventoryError;

/// Why stock moved. A closed list: reports and accounting read it, so a value is added,
/// never renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StockReason {
    /// Goods arrived from a supplier (negative when a receipt is taken back).
    Received,
    TransferOut,
    TransferIn,
    /// Units left in a fulfilment.
    Sold,
    /// A cancelled fulfilment's units are back.
    Restocked,
    /// The shelf was counted: the delta is the new count less the old.
    Counted,
    Correction,
    Damaged,
    /// Theft, loss, or lost in transit.
    Shrinkage,
    /// Samples and gifts.
    Promotion,
    Other,
    /// The unit cost changed; no units moved.
    CostAdjustment,
    /// What a level held when the ledger started.
    Opening,
}

impl StockReason {
    pub const ALL: [StockReason; 13] = [
        StockReason::Received,
        StockReason::TransferOut,
        StockReason::TransferIn,
        StockReason::Sold,
        StockReason::Restocked,
        StockReason::Counted,
        StockReason::Correction,
        StockReason::Damaged,
        StockReason::Shrinkage,
        StockReason::Promotion,
        StockReason::Other,
        StockReason::CostAdjustment,
        StockReason::Opening,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            StockReason::Received => "received",
            StockReason::TransferOut => "transfer_out",
            StockReason::TransferIn => "transfer_in",
            StockReason::Sold => "sold",
            StockReason::Restocked => "restocked",
            StockReason::Counted => "counted",
            StockReason::Correction => "correction",
            StockReason::Damaged => "damaged",
            StockReason::Shrinkage => "shrinkage",
            StockReason::Promotion => "promotion",
            StockReason::Other => "other",
            StockReason::CostAdjustment => "cost_adjustment",
            StockReason::Opening => "opening",
        }
    }

    /// `as_str`'s inverse; `None` for anything else.
    pub fn parse(name: &str) -> Option<StockReason> {
        StockReason::ALL.into_iter().find(|r| r.as_str() == name)
    }
}

impl fmt::Display for StockReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The reasons a person may give for adding or removing units by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdjustmentReason {
    #[default]
    Correction,
    Damaged,
    Shrinkage,
    Promotion,
    Other,
}

impl AdjustmentReason {
    pub const ALL: [AdjustmentReason; 5] = [
        AdjustmentReason::Correction,
        AdjustmentReason::Damaged,
        AdjustmentReason::Shrinkage,
        AdjustmentReason::Promotion,
        AdjustmentReason::Other,
    ];

    pub fn reason(self) -> StockReason {
        match self {
            AdjustmentReason::Correction => StockReason::Correction,
            AdjustmentReason::Damaged => StockReason::Damaged,
            AdjustmentReason::Shrinkage => StockReason::Shrinkage,
            AdjustmentReason::Promotion => StockReason::Promotion,
            AdjustmentReason::Other => StockReason::Other,
        }
    }
}

/// What caused a movement, by the IDs of the documents behind it, so a row links to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockSource {
    /// Someone set or adjusted the count.
    Manual,
    Fulfillment {
        order: OrderId,
        fulfillment: FulfillmentId,
    },
    Receipt {
        purchase_order: PurchaseOrderId,
        receipt: ReceiptId,
    },
    /// A cost added to a purchase order after its goods arrived.
    PurchaseOrder(PurchaseOrderId),
    Transfer(TransferId),
    /// Written by a database migration (the opening rows).
    Migration,
}

/// A person's note on a movement: trimmed, 1 to `MAX_CHARS` characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovementNote(String);

impl MovementNote {
    pub const MAX_CHARS: usize = 500;

    /// `None` for blank text: there's no note.
    pub fn new(note: &str) -> Result<Option<Self>, InventoryError> {
        let note = note.trim();
        if note.is_empty() {
            return Ok(None);
        }
        if note.chars().count() > Self::MAX_CHARS {
            return Err(InventoryError::NoteTooLong {
                max: Self::MAX_CHARS,
            });
        }
        Ok(Some(Self(note.to_string())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a write changes stock and what caused it: what each row it records says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cause {
    pub reason: StockReason,
    pub source: StockSource,
    pub note: Option<MovementNote>,
}

impl Cause {
    pub fn new(reason: StockReason, source: StockSource) -> Self {
        Self {
            reason,
            source,
            note: None,
        }
    }

    pub fn with_note(self, note: Option<MovementNote>) -> Self {
        Self { note, ..self }
    }
}

/// One change to a count or to the unit cost, recorded by the item and not yet written:
/// the adapter stamps it with its position, the actor and the time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockMovement {
    pub item: InventoryItemId,
    pub variant: VariantId,
    /// `None` for a change that moved no units (a cost).
    pub location: Option<LocationId>,
    /// Non-zero when units moved; 0 for a cost change.
    pub delta: i64,
    /// `on_hand` at the location after it; `None` with no location.
    pub after: Option<i64>,
    pub reason: StockReason,
    pub source: StockSource,
    pub note: Option<MovementNote>,
    /// What these units cost: the item's average when they moved (`None` while it has
    /// none). For a cost change, the cost before it.
    pub unit_cost: Option<UnitCost>,
    /// The item's unit cost after it.
    pub average_after: Option<UnitCost>,
}

/// A row of the ledger as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockMovementEntry {
    /// The order rows were written in: an item's rows in the order its writes committed.
    pub position: i64,
    pub item: InventoryItemId,
    pub variant: VariantId,
    /// `None` for a change that moved no units (a cost).
    pub location: Option<LocationId>,
    pub delta: i64,
    /// `on_hand` at the location after it; `None` with no location.
    pub after: Option<i64>,
    pub reason: StockReason,
    pub source: StockSource,
    pub note: Option<String>,
    /// As `StockMovement::unit_cost`.
    pub unit_cost: Option<UnitCost>,
    pub average_after: Option<UnitCost>,
    pub actor: Actor,
    pub at: Timestamp,
}

/// Which of an item's rows a list wants: all that match every field set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MovementFilter {
    pub location: Option<LocationId>,
    pub reason: Option<StockReason>,
    /// At or after.
    pub from: Option<Timestamp>,
    /// Before.
    pub to: Option<Timestamp>,
}

impl MovementFilter {
    /// The rule every adapter follows.
    pub fn matches(&self, row: &StockMovementEntry) -> bool {
        self.location.is_none_or(|l| row.location == Some(l))
            && self.reason.is_none_or(|r| row.reason == r)
            && self.from.is_none_or(|from| row.at >= from)
            && self.to.is_none_or(|to| row.at < to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_read_back_by_name() {
        for reason in StockReason::ALL {
            assert_eq!(StockReason::parse(reason.as_str()), Some(reason));
        }
        assert_eq!(StockReason::parse("stolen"), None);
    }

    #[test]
    fn adjustment_reasons_are_stock_reasons_by_the_same_name() {
        let names: Vec<_> = AdjustmentReason::ALL
            .iter()
            .map(|r| r.reason().as_str())
            .collect();
        assert_eq!(
            names,
            ["correction", "damaged", "shrinkage", "promotion", "other"]
        );
        assert_eq!(AdjustmentReason::default(), AdjustmentReason::Correction);
    }

    #[test]
    fn a_note_is_trimmed_and_blank_is_none() {
        assert_eq!(
            MovementNote::new("  broke in the van ").map(|n| n.map(|n| n.0)),
            Ok(Some("broke in the van".to_string()))
        );
        assert_eq!(MovementNote::new("   "), Ok(None));
    }

    #[test]
    fn a_note_holds_at_most_500_characters() {
        assert!(MovementNote::new(&"é".repeat(500)).is_ok());
        assert_eq!(
            MovementNote::new(&"é".repeat(501)),
            Err(InventoryError::NoteTooLong { max: 500 })
        );
    }
}
