//! The dispatcher's own handler (design note 0016, section 6). Handlers are
//! `hypha_core::EventHandler`s; the host lists the set it runs (design note 0019,
//! section 5), so other crates and spores add theirs without this crate knowing.
//!
//! Handlers must be idempotent: a lease can run out mid-run and the delivery runs again
//! (EV7). Each one's writes are commands of their own, run as `Actor::System`.

use hypha_core::{Ctx, Event, EventHandler, HasBatch, StageEvents, StoredEvent, VariantId};
use hypha_inventory::{HasInventoryItems, InventoryError, InventoryItemRepo};
use hypha_markets::{HasPriceLists, PriceListRepo, StageFixedPrices, remove_fixed_prices};
use hypha_products::{ProductDeleted, VariantRemoved};

/// Cleans up what other domains keep for a removed variant (design note 0014, §4), and
/// for each variant of a deleted product (design note 0033, §6.3).
#[derive(Debug, Clone, Copy, Default)]
pub struct VariantCleanup;

impl<P> EventHandler<P> for VariantCleanup
where
    P: HasPriceLists + HasInventoryItems + HasBatch<Batch: StageFixedPrices + StageEvents>,
{
    fn name(&self) -> &'static str {
        "variant_cleanup"
    }

    fn wants(&self, event_type: &str) -> bool {
        event_type == VariantRemoved::TYPE || event_type == ProductDeleted::TYPE
    }

    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String> {
        variant_cleanup(ports, event).await
    }
}

/// `product.variant_removed` cleans up its variant, `product.deleted` each listed one.
async fn variant_cleanup<P>(ports: &P, event: &StoredEvent) -> Result<(), String>
where
    P: HasPriceLists + HasInventoryItems + HasBatch<Batch: StageFixedPrices + StageEvents>,
{
    let named = || format!("event {} names no variant", event.id);
    let variants: Vec<VariantId> = if event.event_type == ProductDeleted::TYPE {
        let listed = event.data["variants"].as_array().ok_or_else(named)?;
        listed
            .iter()
            .map(|v| v.as_str().and_then(|v| v.parse().ok()).ok_or_else(named))
            .collect::<Result<_, _>>()?
    } else {
        let variant = event.data["variant"].as_str().and_then(|v| v.parse().ok());
        vec![variant.ok_or_else(named)?]
    };
    for variant in variants {
        clean_variant(ports, variant).await?;
    }
    Ok(())
}

/// Removes the variant's fixed prices from every price list,
/// and deletes its inventory item. While units are still reserved on it (open orders
/// count on them) the item is retired instead, and goes with its last reservation when
/// the orders are fulfilled or cancelled (design note 0016, Q5; 0018). Each write
/// records its own events. Running again finds nothing left to do; a write that lost a
/// race fails the run, and the retry sees the item as it is then.
async fn clean_variant<P>(ports: &P, variant: VariantId) -> Result<(), String>
where
    P: HasPriceLists + HasInventoryItems + HasBatch<Batch: StageFixedPrices + StageEvents>,
{
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
            // Stays until the order actions use up or release the reservations.
            Err(InventoryError::StillReserved) if item.retired() => {}
            Err(InventoryError::StillReserved) => {
                item.retire();
                items.update(&mut item).await.map_err(|e| e.to_string())?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
