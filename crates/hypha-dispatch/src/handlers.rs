//! In-process handlers (design note 0016, section 6): destinations in code, named
//! `handler:<name>` in deliveries. Spores will add theirs to the same list.
//!
//! Handlers must be idempotent: a lease can run out mid-run and the delivery runs again
//! (EV7). Each one's writes are commands of their own, run as `Actor::System`.

use hypha_core::{Ctx, Event, StoredEvent, VariantId};
use hypha_inventory::{InventoryError, InventoryItemRepo};
use hypha_markets::{PriceListRepo, remove_fixed_prices};
use hypha_products::VariantRemoved;

use crate::DispatchPorts;

/// A destination in code: its name, and the event types it wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handler {
    pub name: &'static str,
    pub events: &'static [&'static str],
}

/// Cleans up what other domains keep for a removed variant (design note 0014, §4).
pub const VARIANT_CLEANUP: Handler = Handler {
    name: "variant_cleanup",
    events: &[VariantRemoved::TYPE],
};

/// Every handler this build has.
pub const HANDLERS: &[Handler] = &[VARIANT_CLEANUP];

/// The destinations an event of `event_type` goes to: `fan_out` asks this for each
/// event. Apps subscribed to it join with webhooks (step 5 of 0016).
pub fn destinations(event_type: &str) -> Vec<String> {
    HANDLERS
        .iter()
        .filter(|h| h.events.contains(&event_type))
        .map(|h| format!("handler:{}", h.name))
        .collect()
}

/// Runs handler `name` on `event`. `Err` with why, for the delivery's retry.
pub(crate) async fn run_handler<S: DispatchPorts>(
    ports: &S,
    name: &str,
    event: &StoredEvent,
) -> Result<(), String> {
    match name {
        "variant_cleanup" => variant_cleanup(ports, event).await,
        unknown => Err(format!("no handler {unknown:?} in this build")),
    }
}

/// `product.variant_removed`: removes the variant's fixed prices from every price list,
/// and deletes its inventory item unless units are still reserved on it (open orders
/// count on them; fulfilment, 0017, decides when it goes, Q5). Each write records its
/// own events. Running again finds nothing left to do.
async fn variant_cleanup<S: DispatchPorts>(ports: &S, event: &StoredEvent) -> Result<(), String> {
    let variant: VariantId = event.data["variant"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("event {} names no variant", event.id))?;
    let ctx = Ctx::system();
    let lists = ports
        .price_lists()
        .lists_pricing(variant)
        .await
        .map_err(|e| e.to_string())?;
    for list in lists {
        remove_fixed_prices(&ctx, ports, list, vec![variant])
            .await
            .map_err(|e| e.to_string())?;
    }
    let items = ports.inventory_items();
    if let Some(mut item) = items
        .get_by_variant(variant)
        .await
        .map_err(|e| e.to_string())?
    {
        match item.delete() {
            Ok(()) => items.delete(&mut item).await.map_err(|e| e.to_string())?,
            // Stays until fulfilment consumes the reservations.
            Err(InventoryError::StillReserved) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
