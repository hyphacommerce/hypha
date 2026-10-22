//! What a list of inventory items shows: each item's counts per location added up, without
//! its reservations one by one. A read model, not an aggregate: adapters may compute it in
//! storage, and the repository contract checks it equals `InventoryItem::summary`.

use hypha_core::{InventoryItemId, LocationId, SortKey, VariantId};

use crate::{InventoryItem, Level, Reservation};

/// One location's stock of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelSummary {
    pub location: LocationId,
    pub on_hand: i64,
    /// Set aside for orders, not yet shipped.
    pub reserved: i64,
    /// `on_hand − reserved`; negative when more is reserved than is there.
    pub available: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryItemSummary {
    pub id: InventoryItemId,
    pub variant: VariantId,
    pub tracked: bool,
    pub continue_selling: bool,
    /// Every location with a count or a reservation, by location ID.
    pub levels: Vec<LevelSummary>,
    /// Available across those locations.
    pub available: i64,
}

impl InventoryItemSummary {
    /// On hand across locations.
    pub fn on_hand(&self) -> i64 {
        clamp(self.levels.iter().map(|l| i128::from(l.on_hand)).sum())
    }

    /// Reserved across locations.
    pub fn reserved(&self) -> i64 {
        clamp(self.levels.iter().map(|l| i128::from(l.reserved)).sum())
    }

    /// The level at `location`, if the item has stock or a reservation there.
    pub fn level(&self, location: LocationId) -> Option<&LevelSummary> {
        self.levels.iter().find(|l| l.location == location)
    }
}

/// A sum past `i64` (only reachable with many locations at the stock limits) is held at
/// the bound rather than failing: a list shouldn't stop reading over one item.
fn clamp(n: i128) -> i64 {
    i64::try_from(n).unwrap_or(if n < 0 { i64::MIN } else { i64::MAX })
}

/// Each location's stock and the total available, from an item's levels and reservations:
/// what `InventoryItem::summary` reports and what adapters store to sort and filter by.
pub fn stock_by_location(
    levels: &[Level],
    reservations: &[Reservation],
) -> (Vec<LevelSummary>, i64) {
    let mut locations: Vec<LocationId> = levels
        .iter()
        .map(|l| l.location)
        .chain(reservations.iter().map(|r| r.location))
        .collect();
    locations.sort();
    locations.dedup();
    let summaries: Vec<LevelSummary> = locations
        .into_iter()
        .map(|location| {
            let on_hand = levels
                .iter()
                .find(|l| l.location == location)
                .map_or(0, |l| l.on_hand);
            let reserved: i64 = reservations
                .iter()
                .filter(|r| r.location == location)
                .map(|r| i64::from(r.quantity.value()))
                .sum();
            LevelSummary {
                location,
                on_hand,
                reserved,
                // Within ±2⁵³ on hand and a count of u32 reservations: no overflow.
                available: on_hand - reserved,
            }
        })
        .collect();
    let available = clamp(summaries.iter().map(|l| i128::from(l.available)).sum());
    (summaries, available)
}

impl InventoryItem {
    pub fn summary(&self) -> InventoryItemSummary {
        let (levels, available) = stock_by_location(self.levels(), self.reservations());
        InventoryItemSummary {
            id: self.id(),
            variant: self.variant(),
            tracked: self.tracked(),
            continue_selling: self.continue_selling(),
            levels,
            available,
        }
    }
}

/// What a list of inventory items is narrowed to. Items whose variant was removed
/// (retired, waiting for their last reservation) are never listed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InventoryFilter {
    /// Only tracked items with less than this available (at `location` when given,
    /// otherwise across locations). Untracked stock never runs low.
    pub below: Option<i64>,
    /// Only items with stock or a reservation here; `below` and the order then count this
    /// location alone.
    pub location: Option<LocationId>,
    /// Only the items of these variants (a product's, or one SKU's).
    pub variants: Option<Vec<VariantId>>,
}

impl InventoryFilter {
    /// The number an item is listed by: available at `location`, or across locations.
    pub fn available(&self, item: &InventoryItemSummary) -> i64 {
        match self.location {
            Some(location) => item.level(location).map_or(0, |l| l.available),
            None => item.available,
        }
    }

    /// The list's sort key: lowest available first, then by ID.
    pub fn key(&self, item: &InventoryItemSummary) -> SortKey {
        SortKey::Int(self.available(item))
    }

    /// Whether a page position is one this list hands out.
    pub fn fits(key: &SortKey) -> bool {
        matches!(key, SortKey::Int(_))
    }

    pub fn matches(&self, item: &InventoryItem) -> bool {
        if item.retired() {
            return false;
        }
        if let Some(variants) = &self.variants
            && !variants.contains(&item.variant())
        {
            return false;
        }
        let summary = item.summary();
        if let Some(location) = self.location
            && summary.level(location).is_none()
        {
            return false;
        }
        match self.below {
            Some(below) => item.tracked() && self.available(&summary) < below,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::Quantity;

    use super::*;
    use crate::{Location, ReservationRef};

    fn location(n: u128) -> LocationId {
        LocationId::from_uuid(uuid::Uuid::from_u128(n))
    }

    fn item() -> InventoryItem {
        InventoryItem::new(
            InventoryItemId::from_uuid(uuid::Uuid::from_u128(1)),
            VariantId::from_uuid(uuid::Uuid::from_u128(2)),
            true,
            false,
        )
    }

    fn reserve(item: &mut InventoryItem, at: u128, quantity: u32) {
        let at = Location::new(location(at), "L".to_string(), vec![], 0).unwrap();
        item.reserve(
            &ReservationRef::new("order-1").unwrap(),
            Quantity::new(quantity).unwrap(),
            &[at],
        )
        .unwrap();
    }

    #[test]
    fn a_summary_counts_each_location_and_the_total() {
        let mut i = item();
        i.set_on_hand(location(2), 5, None).unwrap();
        i.set_on_hand(location(1), 3, None).unwrap();
        reserve(&mut i, 2, 2);
        let s = i.summary();
        assert_eq!(
            s.levels,
            vec![
                LevelSummary {
                    location: location(1),
                    on_hand: 3,
                    reserved: 0,
                    available: 3
                },
                LevelSummary {
                    location: location(2),
                    on_hand: 5,
                    reserved: 2,
                    available: 3
                },
            ]
        );
        assert_eq!((s.on_hand(), s.reserved(), s.available), (8, 2, 6));
    }

    #[test]
    fn a_reservation_alone_makes_a_location_with_nothing_on_hand() {
        let mut i = item();
        i.set_continue_selling(true);
        reserve(&mut i, 3, 4);
        let s = i.summary();
        assert_eq!(
            s.levels,
            vec![LevelSummary {
                location: location(3),
                on_hand: 0,
                reserved: 4,
                available: -4
            }]
        );
        assert_eq!(s.available, -4);
    }

    #[test]
    fn totals_past_i64_are_held_at_the_bound() {
        assert_eq!(clamp(i128::from(i64::MAX) + 1), i64::MAX);
        assert_eq!(clamp(i128::from(i64::MIN) - 1), i64::MIN);
        assert_eq!(clamp(-3), -3);
    }

    #[test]
    fn below_is_strict_and_skips_untracked_items() {
        let mut i = item();
        i.set_on_hand(location(1), 3, None).unwrap();
        let below = |n| InventoryFilter {
            below: Some(n),
            ..InventoryFilter::default()
        };
        assert!(!below(3).matches(&i));
        assert!(below(4).matches(&i));
        i.set_tracked(false);
        assert!(!below(4).matches(&i));
    }

    #[test]
    fn a_location_counts_alone_and_lists_only_what_it_stocks() {
        let mut i = item();
        i.set_on_hand(location(1), 1, None).unwrap();
        i.set_on_hand(location(2), 10, None).unwrap();
        let at = |n, below| InventoryFilter {
            location: Some(location(n)),
            below,
            variants: None,
        };
        assert!(at(1, Some(2)).matches(&i));
        assert!(!at(2, Some(2)).matches(&i));
        assert!(!at(3, None).matches(&i));
        assert_eq!(at(2, None).key(&i.summary()), SortKey::Int(10));
        assert_eq!(
            InventoryFilter::default().key(&i.summary()),
            SortKey::Int(11)
        );
    }

    #[test]
    fn variants_narrow_and_retired_items_never_show() {
        let mut i = item();
        let only = |n| InventoryFilter {
            variants: Some(vec![VariantId::from_uuid(uuid::Uuid::from_u128(n))]),
            ..InventoryFilter::default()
        };
        assert!(only(2).matches(&i));
        assert!(!only(3).matches(&i));
        i.retire();
        assert!(!InventoryFilter::default().matches(&i));
    }
}
