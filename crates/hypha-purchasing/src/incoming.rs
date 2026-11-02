//! Incoming stock: what ordered purchase orders still bring, per variant and location.
//! Derived from the orders whenever it's read, never kept on the stock record: every
//! edit, close or order would otherwise touch the records it names and race checkouts on
//! their version.

use std::collections::BTreeMap;

use hypha_core::{LocationId, Timestamp, VariantId};

use crate::{PurchaseOrder, PurchaseOrderStatus};

/// What open purchase orders still bring of `variant` to `location`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncomingStock {
    pub variant: VariantId,
    pub location: LocationId,
    /// Units still outstanding: more than zero.
    pub quantity: u64,
    /// The soonest an order bringing some of them is expected; `None` when none says.
    pub next_expected_at: Option<Timestamp>,
}

/// The rule every adapter follows: over the `ordered` orders with a destination, each
/// line of one of `variants` with units outstanding counts them at that destination, and
/// the soonest `expected_at` among those orders comes with it. Sorted by variant, then
/// location.
pub fn incoming<'a>(
    orders: impl IntoIterator<Item = &'a PurchaseOrder>,
    variants: &[VariantId],
) -> Vec<IncomingStock> {
    let mut found: BTreeMap<(VariantId, LocationId), IncomingStock> = BTreeMap::new();
    for po in orders {
        if po.status() != PurchaseOrderStatus::Ordered {
            continue;
        }
        let Some(location) = po.destination() else {
            continue;
        };
        for line in po.lines() {
            let outstanding = line.outstanding();
            if outstanding == 0 || !variants.contains(&line.variant()) {
                continue;
            }
            let entry = found
                .entry((line.variant(), location))
                .or_insert(IncomingStock {
                    variant: line.variant(),
                    location,
                    quantity: 0,
                    next_expected_at: None,
                });
            entry.quantity += u64::from(outstanding);
            entry.next_expected_at = Timestamp::earliest(entry.next_expected_at, po.expected_at());
        }
    }
    found.into_values().collect()
}
