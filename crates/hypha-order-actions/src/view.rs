//! An order with what's left to ship, by location: derived from the lines' reservations,
//! not stored.

use hypha_core::{Ctx, LocationId, OrderId, OrderLineId, Scope};
use hypha_inventory::{HasInventoryItems, ReservationRef};
use hypha_orders::{HasOrders, Order, OrderRepo};
use hypha_payments::{HasPayments, Payment, PaymentRepo};

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
    /// Its payments, newest first.
    pub payments: Vec<Payment>,
}

/// The order, what's left to ship and its payments (`orders:read`; the stock and
/// payments it reads are the order's own, so no inventory or payments scope).
pub async fn order_view<S: HasOrders + HasInventoryItems + HasPayments>(
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
    // In series: one store runs one query at a time (Postgres: a connection per
    // request), so joining these would gain nothing.
    let payments = ports.payments().list_for_order(id).await?;
    view_of(ports, order, payments).await
}

/// The view of an order and its payments a command just returned (no need to read
/// them again): only the stock it waits in is read. The caller checked `orders:read`
/// (or more) already.
pub async fn view_of<S: HasInventoryItems>(
    ports: &S,
    order: Order,
    payments: Vec<Payment>,
) -> Result<OrderView, CommandError> {
    if order.cancelled().is_some() {
        return Ok(OrderView {
            order,
            unfulfilled: Vec::new(),
            payments,
        });
    }
    let items = crate::commands::order_items(ports, &order).await?;

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
    Ok(OrderView {
        order,
        unfulfilled,
        payments,
    })
}
