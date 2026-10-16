//! An order with what's left to ship, by location (design note 0018, FU4): derived from
//! the lines' reservations, not stored.

use hypha_core::{Ctx, LocationId, OrderId, OrderLineId, Scope};
use hypha_inventory::{HasInventoryItems, InventoryItemRepo, ReservationRef};
use hypha_orders::{HasOrders, Order, OrderRepo};

use crate::CommandError;

/// Units of one line still to ship, and where they're waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unfulfilled {
    pub line: OrderLineId,
    /// The location holding them for the line; `None` for units held nowhere (an
    /// untracked item, or tracking turned on after the order was placed): any location.
    pub location: Option<LocationId>,
    pub quantity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderView {
    pub order: Order,
    /// For each line with something left, in line order, then location by the
    /// reservations' order; nothing for a cancelled order.
    pub unfulfilled: Vec<Unfulfilled>,
}

/// The order and what's left to ship (`orders:read`; the stock it reads is the order's
/// own, so no inventory scope).
pub async fn order_view<S: HasOrders + HasInventoryItems>(
    ctx: &Ctx,
    ports: &S,
    id: OrderId,
) -> Result<OrderView, CommandError> {
    ctx.require(Scope::OrdersRead)?;
    let order = ports
        .orders()
        .get(id)
        .await?
        .ok_or(CommandError::OrderNotFound(id))?;
    if order.cancelled().is_some() {
        return Ok(OrderView {
            order,
            unfulfilled: Vec::new(),
        });
    }
    let mut variants: Vec<_> = order.lines().iter().map(|l| l.variant).collect();
    variants.sort_by_key(|v| v.as_uuid());
    variants.dedup();
    let items = ports.inventory_items().for_variants(&variants).await?;

    let mut unfulfilled = Vec::new();
    for line in order.lines() {
        let mut left = line.unfulfilled_quantity();
        let reference = ReservationRef::new(&line.id.to_string())?;
        let held = items
            .iter()
            .filter(|item| item.variant() == line.variant)
            .flat_map(|item| item.reservations())
            .filter(|r| r.reference == reference);
        for r in held {
            let quantity = r.quantity.value().min(left);
            if quantity > 0 {
                unfulfilled.push(Unfulfilled {
                    line: line.id,
                    location: Some(r.location),
                    quantity,
                });
                left -= quantity;
            }
        }
        if left > 0 {
            unfulfilled.push(Unfulfilled {
                line: line.id,
                location: None,
                quantity: left,
            });
        }
    }
    Ok(OrderView { order, unfulfilled })
}
