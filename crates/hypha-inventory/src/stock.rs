use std::collections::HashMap;

use hypha_core::{MarketId, Quantity, RepoError, VariantId};

use crate::{
    HasInventoryItems, HasLocations, InventoryItem, InventoryItemRepo, Location, LocationRepo,
};

/// Whether a quantity of a variant can be sold in a market right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// Stock isn't tracked for it (no item, or `tracked: false`): always sellable.
    Untracked,
    Available,
    /// `available` is the total over the market's locations (can be 0).
    OutOfStock {
        available: i64,
    },
}

/// Everything needed to check stock for one market, as plain data, like `TaxContext`.
///
/// The caller loads it in two batched queries (`InventoryItemRepo::for_variants`,
/// `LocationRepo::serving`) for exactly the variants it is about to check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StockContext {
    /// Items of the variants being checked. A variant with none is untracked (I3).
    pub items: HashMap<VariantId, InventoryItem>,
    /// The locations that serve the market.
    pub locations: Vec<Location>,
}

impl StockContext {
    /// The stock of `variants` in the locations serving `market`, in 2 queries (design
    /// note 0019, section 8: carts and search documents load it the same way).
    pub async fn load<S: HasInventoryItems + HasLocations>(
        ports: &S,
        market: MarketId,
        variants: &[VariantId],
    ) -> Result<Self, RepoError> {
        let items = ports.inventory_items().for_variants(variants).await?;
        Ok(Self {
            items: items.into_iter().map(|i| (i.variant(), i)).collect(),
            locations: ports.locations().serving(market).await?,
        })
    }

    pub fn availability(&self, variant: VariantId, quantity: Quantity) -> Availability {
        let Some(item) = self.items.get(&variant) else {
            return Availability::Untracked;
        };
        if !item.tracked() {
            return Availability::Untracked;
        }
        match item.can_supply(quantity, &self.locations) {
            Ok(()) => Availability::Available,
            Err(available) => Availability::OutOfStock { available },
        }
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{InventoryItemId, LocationId, MarketId};
    use uuid::Uuid;

    use super::*;

    fn qty(n: u32) -> Quantity {
        Quantity::new(n).unwrap()
    }

    fn vid(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn loc() -> LocationId {
        LocationId::from_uuid(Uuid::from_u128(10))
    }

    /// Variant 1 has 2 in stock at the market's one location.
    fn context(tracked: bool) -> StockContext {
        let mut item = InventoryItem::new(
            InventoryItemId::from_uuid(Uuid::from_u128(20)),
            vid(1),
            tracked,
            false,
        );
        item.set_on_hand(loc(), 2, None).unwrap();
        let location = Location::new(
            loc(),
            "Zagreb".to_string(),
            vec![MarketId::from_uuid(Uuid::from_u128(30))],
            0,
        )
        .unwrap();
        StockContext {
            items: HashMap::from([(vid(1), item)]),
            locations: vec![location],
        }
    }

    #[test]
    fn available_up_to_the_stock() {
        assert_eq!(
            context(true).availability(vid(1), qty(2)),
            Availability::Available
        );
    }

    #[test]
    fn out_of_stock_beyond_it() {
        assert_eq!(
            context(true).availability(vid(1), qty(3)),
            Availability::OutOfStock { available: 2 }
        );
    }

    #[test]
    fn a_variant_without_an_item_is_untracked() {
        assert_eq!(
            context(true).availability(vid(2), qty(99)),
            Availability::Untracked
        );
    }

    #[test]
    fn an_untracked_item_is_untracked() {
        assert_eq!(
            context(false).availability(vid(1), qty(99)),
            Availability::Untracked
        );
    }
}
