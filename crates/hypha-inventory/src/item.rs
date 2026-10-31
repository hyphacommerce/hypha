use std::collections::HashSet;

use hypha_core::{
    InventoryItemId, LocationId, PendingEvent, Quantity, Recorded, Recorder, Stamped, Stamps,
    Subject, VariantId, Version, Versioned,
};

use crate::{
    InventoryError, Location, ReservationRef, RestoreError,
    events::{Available, INVENTORY_ITEM, InventoryItemChanged, InventoryItemDeleted},
};

/// Largest stock count, either way: 2⁵³ − 1, like `Money::MAX_AMOUNT`, so JSON clients
/// and D1 represent every count exactly.
pub const MAX_STOCK: i64 = (1 << 53) - 1;

/// How many units are physically at one location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    pub location: LocationId,
    /// Can go below zero once stock sold past zero (`continue_selling`) is fulfilled.
    pub on_hand: i64,
}

/// Units set aside at one location for `reference` (later an order line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    pub reference: ReservationRef,
    pub location: LocationId,
    pub quantity: Quantity,
}

/// The stock of one variant across locations.
///
/// The item owns its levels and reservations, the way a product owns its variants, so they
/// change together under one version. That version is what prevents overselling: two
/// checkouts that both read "1 left" can't both write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryItem {
    id: InventoryItemId,
    version: Version,
    /// When it was stored and last changed, set by the adapter.
    stamps: Option<Stamps>,
    variant: VariantId,
    tracked: bool,
    continue_selling: bool,
    levels: Vec<Level>,
    reservations: Vec<Reservation>,
    /// Its variant was removed while units were still reserved: it goes once the last
    /// reservation is fulfilled or released.
    retired: bool,
    /// Whether anything changed since it was loaded: its event is built from the item as it
    /// is when written.
    changed: Changed,
    /// Its deletion, once `delete` was called.
    deleted: Recorder,
}

/// A flag that compares as always equal, so it never makes two items unequal.
#[derive(Debug, Clone, Copy, Default)]
struct Changed(bool);

impl PartialEq for Changed {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changed {}

impl Recorded for InventoryItem {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let deleted = self.deleted.take();
        if !deleted.is_empty() {
            return deleted;
        }
        if !std::mem::take(&mut self.changed).0 {
            return Vec::new();
        }
        let mut locations: Vec<LocationId> = self
            .levels
            .iter()
            .map(|l| l.location)
            .chain(self.reservations.iter().map(|r| r.location))
            .collect();
        locations.sort();
        locations.dedup();
        let event = InventoryItemChanged {
            inventory_item: self.id,
            variant: self.variant,
            tracked: self.tracked,
            continue_selling: self.continue_selling,
            available: locations
                .into_iter()
                .map(|location| Available {
                    location,
                    available: self.available(location),
                })
                .collect(),
        };
        vec![PendingEvent::new(
            Subject::new(INVENTORY_ITEM, self.id.as_uuid()),
            &event,
        )]
    }
}

impl Versioned for InventoryItem {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for InventoryItem {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl InventoryItem {
    pub fn new(
        id: InventoryItemId,
        variant: VariantId,
        tracked: bool,
        continue_selling: bool,
    ) -> Self {
        Self {
            id,
            version: Version::NEW,
            stamps: None,
            variant,
            tracked,
            continue_selling,
            levels: Vec::new(),
            reservations: Vec::new(),
            retired: false,
            changed: Changed(true),
            deleted: Recorder::default(),
        }
    }

    /// Untracked stock is always available and never reserved. Existing levels and
    /// reservations are kept, so turning tracking back on picks up where it was.
    pub fn set_tracked(&mut self, tracked: bool) {
        if self.tracked != tracked {
            self.tracked = tracked;
            self.changed = Changed(true);
        }
    }

    /// Keep selling when stock runs out.
    pub fn set_continue_selling(&mut self, continue_selling: bool) {
        if self.continue_selling != continue_selling {
            self.continue_selling = continue_selling;
            self.changed = Changed(true);
        }
    }

    pub fn on_hand(&self, location: LocationId) -> i64 {
        self.levels
            .iter()
            .find(|l| l.location == location)
            .map_or(0, |l| l.on_hand)
    }

    pub fn reserved(&self, location: LocationId) -> i64 {
        self.reservations
            .iter()
            .filter(|r| r.location == location)
            .map(|r| i64::from(r.quantity.value()))
            .sum()
    }

    /// `on_hand − reserved`. Negative when more is reserved than is there.
    pub fn available(&self, location: LocationId) -> i64 {
        self.on_hand(location) - self.reserved(location)
    }

    /// Sets the count at `location`, e.g. after counting the shelf.
    ///
    /// With `expected`, the change only happens if `on_hand` is still what the caller
    /// last saw (Shopify's `changeFromQuantity`): a sale in between isn't silently
    /// overwritten. A location without a level counts as 0.
    pub fn set_on_hand(
        &mut self,
        location: LocationId,
        quantity: i64,
        expected: Option<i64>,
    ) -> Result<(), InventoryError> {
        check_range(i128::from(quantity))?;
        let actual = self.on_hand(location);
        if let Some(expected) = expected
            && expected != actual
        {
            return Err(InventoryError::OnHandChanged { expected, actual });
        }
        self.write_level(location, quantity);
        Ok(())
    }

    /// Adds `delta` (negative to remove) to the count at `location`.
    pub fn adjust_on_hand(
        &mut self,
        location: LocationId,
        delta: i64,
    ) -> Result<(), InventoryError> {
        let quantity = check_range(i128::from(self.on_hand(location)) + i128::from(delta))?;
        self.write_level(location, quantity);
        Ok(())
    }

    fn write_level(&mut self, location: LocationId, on_hand: i64) {
        match self.levels.iter_mut().find(|l| l.location == location) {
            Some(level) if level.on_hand == on_hand => return,
            Some(level) => level.on_hand = on_hand,
            None => self.levels.push(Level { location, on_hand }),
        }
        self.changed = Changed(true);
    }

    /// Where `quantity` would be reserved from `locations`: `Ok` with the allocation, or
    /// `Err` with how much is available there in total.
    ///
    /// `locations` are the ones serving the buyer's market; their order doesn't matter.
    /// Allocation: the first location by priority that has the whole quantity; otherwise
    /// the locations in priority order, each giving what it has. With `continue_selling`,
    /// whatever is still missing goes to the first location.
    fn allocate(
        &self,
        quantity: u32,
        locations: &[Location],
    ) -> Result<Vec<(LocationId, u32)>, i64> {
        let mut ordered: Vec<&Location> = locations.iter().collect();
        ordered.sort_by_key(|l| (l.priority(), l.id().as_uuid()));
        // A location listed twice must not count its stock twice; keep its first entry.
        let mut seen = HashSet::new();
        ordered.retain(|l| seen.insert(l.id()));
        // Clamped to u32: a location with more than u32::MAX free covers any request,
        // and the total below is summed in u64, so no decision changes.
        let free: Vec<(LocationId, u32)> = ordered
            .iter()
            .map(|l| {
                let free = u32::try_from(self.available(l.id()).max(0)).unwrap_or(u32::MAX);
                (l.id(), free)
            })
            .collect();

        if let Some((location, _)) = free.iter().find(|(_, free)| *free >= quantity) {
            return Ok(vec![(*location, quantity)]);
        }
        let total: u64 = free.iter().map(|(_, free)| u64::from(*free)).sum();
        let Some(&(first, _)) = free.first() else {
            return Err(0);
        };
        if total < u64::from(quantity) && !self.continue_selling {
            return Err(i64::try_from(total).unwrap_or(MAX_STOCK));
        }

        let mut left = quantity;
        let mut allocation = Vec::new();
        for (location, free) in free {
            let take = free.min(left);
            if take > 0 {
                allocation.push((location, take));
                left -= take;
            }
        }
        if left > 0 {
            // Sold past zero: the rest waits at the highest-priority location.
            match allocation.iter_mut().find(|(l, _)| *l == first) {
                Some((_, taken)) => *taken += left,
                None => allocation.insert(0, (first, left)),
            }
        }
        Ok(allocation)
    }

    /// `Ok` if `quantity` could be reserved from `locations` right now (always, if
    /// untracked); otherwise `Err` with how many are available. Reserves nothing.
    ///
    /// Returns only the shortage, not an `InventoryError`: a shortage is the one way it can
    /// fail, so callers can match it exhaustively. How many `locations` can supply now
    /// (each location once), or `None` without a limit: the item isn't tracked, or it keeps
    /// selling when out of stock. What a storefront caps its quantity inputs at.
    pub fn sellable(&self, locations: &[Location]) -> Option<u64> {
        if !self.tracked || self.continue_selling {
            return None;
        }
        let mut seen = HashSet::new();
        Some(
            locations
                .iter()
                .filter(|l| seen.insert(l.id()))
                .map(|l| u64::try_from(self.available(l.id()).max(0)).unwrap_or(0))
                .sum(),
        )
    }

    pub fn can_supply(&self, quantity: Quantity, locations: &[Location]) -> Result<(), i64> {
        if !self.tracked {
            return Ok(());
        }
        self.allocate(quantity.value(), locations).map(|_| ())
    }

    /// Reserves `quantity` for `reference` from `locations` (the ones serving the
    /// buyer's market). Returns whether anything changed.
    ///
    /// Idempotent: if `reference` already holds exactly `quantity`, nothing changes, so a
    /// retried checkout doesn't reserve twice. Untracked items reserve nothing.
    pub fn reserve(
        &mut self,
        reference: &ReservationRef,
        quantity: Quantity,
        locations: &[Location],
    ) -> Result<bool, InventoryError> {
        if !self.tracked {
            return Ok(false);
        }
        let held: u64 = self
            .reservations
            .iter()
            .filter(|r| r.reference == *reference)
            .map(|r| u64::from(r.quantity.value()))
            .sum();
        if held > 0 {
            return if held == u64::from(quantity.value()) {
                Ok(false)
            } else {
                Err(InventoryError::ReferenceInUse(reference.clone()))
            };
        }

        let allocation = self
            .allocate(quantity.value(), locations)
            .map_err(|available| InventoryError::OutOfStock {
                requested: quantity.value(),
                available,
            })?;
        for (location, share) in allocation {
            // `allocate` never hands out an empty share.
            let Ok(quantity) = Quantity::new(share) else {
                continue;
            };
            self.reservations.push(Reservation {
                reference: reference.clone(),
                location,
                quantity,
            });
        }
        self.changed = Changed(true);
        Ok(true)
    }

    /// Records that the item is being deleted (its variant was removed); the repository's
    /// `delete` writes it. Refused while anything is reserved: open orders still count on
    /// those units.
    pub fn delete(&mut self) -> Result<(), InventoryError> {
        if !self.reservations.is_empty() {
            return Err(InventoryError::StillReserved);
        }
        // Whatever else changed is moot now.
        self.changed = Changed(false);
        let event = InventoryItemDeleted {
            inventory_item: self.id,
            variant: self.variant,
        };
        self.deleted
            .record(Subject::new(INVENTORY_ITEM, self.id.as_uuid()), &event);
        Ok(())
    }

    /// Records that `quantity` for `reference` left `location`. It comes out of what
    /// `reference` holds at `location` first; whatever else it holds elsewhere is released
    /// for the rest, since the units shipped from here instead. On hand at `location` drops
    /// by `quantity`, which may take it below zero (the count was off, or stock was sold
    /// past zero). Untracked items change no counts, but stale reservations are still given
    /// back.
    ///
    /// All or nothing; returns whether anything changed.
    pub fn fulfil(
        &mut self,
        reference: &ReservationRef,
        location: LocationId,
        quantity: Quantity,
    ) -> Result<bool, InventoryError> {
        let on_hand = match self.tracked {
            true => Some(check_range(
                i128::from(self.on_hand(location)) - i128::from(quantity.value()),
            )?),
            false => None,
        };
        // Here first, then the other locations in the order they were reserved.
        let mut held: Vec<usize> = (0..self.reservations.len())
            .filter(|&i| self.reservations[i].reference == *reference)
            .collect();
        held.sort_by_key(|&i| self.reservations[i].location != location);
        let mut left = quantity.value();
        let mut remaining = vec![None; self.reservations.len()];
        for i in held {
            let r = &self.reservations[i];
            let take = r.quantity.value().min(left);
            left -= take;
            remaining[i] = Some(r.quantity.value() - take);
        }
        let mut released = false;
        let mut kept = Vec::with_capacity(self.reservations.len());
        for (r, remaining) in std::mem::take(&mut self.reservations)
            .into_iter()
            .zip(remaining)
        {
            match remaining.map(Quantity::new) {
                None => kept.push(r),
                Some(Ok(quantity)) => {
                    released |= quantity != r.quantity;
                    kept.push(Reservation { quantity, ..r });
                }
                // Nothing left of it.
                Some(Err(_)) => released = true,
            }
        }
        self.reservations = kept;
        if released {
            self.changed = Changed(true);
        }
        // A tracked count always moves: `quantity` is at least 1.
        if let Some(on_hand) = on_hand {
            self.write_level(location, on_hand);
        }
        Ok(released || on_hand.is_some())
    }

    /// Reverses `fulfil` for a cancelled fulfilment whose units came off this item's
    /// stock (the caller knows: `FulfillmentLine::stock_taken`): `quantity` is back on
    /// hand at `location`, even if tracking was turned off since, so the count is right
    /// when it's turned on again. While tracked, the units are reserved there for
    /// `reference` again; untracked items reserve nothing.
    ///
    /// All or nothing: a count or reservation past its range changes nothing.
    pub fn restock(
        &mut self,
        reference: &ReservationRef,
        location: LocationId,
        quantity: Quantity,
    ) -> Result<(), InventoryError> {
        let on_hand =
            check_range(i128::from(self.on_hand(location)) + i128::from(quantity.value()))?;
        if self.tracked {
            match self
                .reservations
                .iter_mut()
                .find(|r| r.reference == *reference && r.location == location)
            {
                Some(r) => r.quantity = r.quantity.checked_add(quantity)?,
                None => self.reservations.push(Reservation {
                    reference: reference.clone(),
                    location,
                    quantity,
                }),
            }
            self.changed = Changed(true);
        }
        self.write_level(location, on_hand);
        Ok(())
    }

    /// Marks the item to go once nothing is reserved on it any more: its variant was
    /// removed while open orders still counted on these units.
    pub fn retire(&mut self) {
        self.retired = true;
    }

    /// Retired, and nothing is reserved on it any more: the caller deletes it.
    pub fn is_spent(&self) -> bool {
        self.retired && self.reservations.is_empty()
    }

    /// Gives back everything reserved for `reference`. Returns whether anything was;
    /// releasing an unknown reference is not an error, so a retry is harmless.
    pub fn release(&mut self, reference: &ReservationRef) -> bool {
        let before = self.reservations.len();
        self.reservations.retain(|r| r.reference != *reference);
        let released = self.reservations.len() != before;
        if released {
            self.changed = Changed(true);
        }
        released
    }

    pub fn id(&self) -> InventoryItemId {
        self.id
    }

    pub fn variant(&self) -> VariantId {
        self.variant
    }

    pub fn tracked(&self) -> bool {
        self.tracked
    }

    pub fn continue_selling(&self) -> bool {
        self.continue_selling
    }

    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    pub fn reservations(&self) -> &[Reservation] {
        &self.reservations
    }

    pub fn retired(&self) -> bool {
        self.retired
    }
}

fn check_range(quantity: i128) -> Result<i64, InventoryError> {
    if quantity.abs() > i128::from(MAX_STOCK) {
        return Err(InventoryError::StockOutOfRange(quantity));
    }
    // In range, so it fits.
    i64::try_from(quantity).map_err(|_| InventoryError::StockOutOfRange(quantity))
}

/// Plain-data form of an `InventoryItem` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryItemSnapshot {
    pub id: InventoryItemId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub variant: VariantId,
    pub tracked: bool,
    pub continue_selling: bool,
    pub levels: Vec<Level>,
    pub reservations: Vec<Reservation>,
    pub retired: bool,
}

impl InventoryItem {
    pub fn snapshot(&self) -> InventoryItemSnapshot {
        InventoryItemSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            variant: self.variant,
            tracked: self.tracked,
            continue_selling: self.continue_selling,
            levels: self.levels.clone(),
            reservations: self.reservations.clone(),
            retired: self.retired,
        }
    }

    /// Rebuilds an item from storage: one level per location, one reservation per
    /// reference and location, counts in range.
    pub fn restore(s: InventoryItemSnapshot) -> Result<InventoryItem, RestoreError> {
        let mut locations = HashSet::new();
        for level in &s.levels {
            check_range(i128::from(level.on_hand))?;
            if !locations.insert(level.location) {
                return Err(RestoreError::DuplicateLevel(level.location));
            }
        }
        let mut held = HashSet::new();
        for r in &s.reservations {
            if !held.insert((&r.reference, r.location)) {
                return Err(RestoreError::DuplicateReservation {
                    reference: r.reference.clone(),
                    location: r.location,
                });
            }
        }
        Ok(InventoryItem {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            variant: s.variant,
            tracked: s.tracked,
            continue_selling: s.continue_selling,
            levels: s.levels,
            reservations: s.reservations,
            retired: s.retired,
            changed: Changed(false),
            deleted: Recorder::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{MarketId, QuantityError};
    use uuid::Uuid;

    use super::*;

    fn loc_id(n: u128) -> LocationId {
        LocationId::from_uuid(Uuid::from_u128(n))
    }

    /// Location `n` with the given priority, serving one market.
    fn location(n: u128, priority: u32) -> Location {
        Location::new(
            loc_id(n),
            format!("Location {n}"),
            vec![MarketId::from_uuid(Uuid::from_u128(100))],
            priority,
        )
        .unwrap()
    }

    fn reference(s: &str) -> ReservationRef {
        ReservationRef::new(s).unwrap()
    }

    fn qty(n: u32) -> Quantity {
        Quantity::new(n).unwrap()
    }

    /// A tracked item with `stock` (location, on hand) pairs.
    fn item(stock: &[(u128, i64)]) -> InventoryItem {
        let mut item = InventoryItem::new(
            InventoryItemId::from_uuid(Uuid::from_u128(1)),
            VariantId::from_uuid(Uuid::from_u128(2)),
            true,
            false,
        );
        for (location, on_hand) in stock {
            item.set_on_hand(loc_id(*location), *on_hand, None).unwrap();
        }
        item
    }

    fn held(item: &InventoryItem) -> Vec<(LocationId, u32)> {
        item.reservations()
            .iter()
            .map(|r| (r.location, r.quantity.value()))
            .collect()
    }

    // ---- counting ----

    #[test]
    fn a_location_without_a_level_has_nothing() {
        assert_eq!(item(&[]).on_hand(loc_id(1)), 0);
    }

    #[test]
    fn set_on_hand_replaces_the_count() {
        let mut i = item(&[(1, 5)]);
        i.set_on_hand(loc_id(1), 2, None).unwrap();
        assert_eq!(i.on_hand(loc_id(1)), 2);
        assert_eq!(i.levels().len(), 1);
    }

    #[test]
    fn set_on_hand_with_the_expected_count_succeeds() {
        let mut i = item(&[(1, 5)]);
        assert_eq!(i.set_on_hand(loc_id(1), 7, Some(5)), Ok(()));
        assert_eq!(i.on_hand(loc_id(1)), 7);
    }

    #[test]
    fn set_on_hand_refuses_when_the_count_moved() {
        let mut i = item(&[(1, 4)]);
        assert_eq!(
            i.set_on_hand(loc_id(1), 10, Some(5)),
            Err(InventoryError::OnHandChanged {
                expected: 5,
                actual: 4
            })
        );
        assert_eq!(i.on_hand(loc_id(1)), 4);
    }

    #[test]
    fn expected_count_of_a_new_level_is_zero() {
        let mut i = item(&[]);
        assert_eq!(i.set_on_hand(loc_id(1), 3, Some(0)), Ok(()));
    }

    #[test]
    fn adjust_adds_and_removes() {
        let mut i = item(&[(1, 5)]);
        i.adjust_on_hand(loc_id(1), -7).unwrap();
        assert_eq!(i.on_hand(loc_id(1)), -2);
        i.adjust_on_hand(loc_id(2), 4).unwrap();
        assert_eq!(i.on_hand(loc_id(2)), 4);
    }

    #[test]
    fn stock_is_capped_at_max_stock_either_way() {
        let mut i = item(&[]);
        assert_eq!(i.set_on_hand(loc_id(1), MAX_STOCK, None), Ok(()));
        assert_eq!(
            i.adjust_on_hand(loc_id(1), 1),
            Err(InventoryError::StockOutOfRange(i128::from(MAX_STOCK) + 1))
        );
        assert_eq!(i.set_on_hand(loc_id(2), -MAX_STOCK, None), Ok(()));
        assert_eq!(
            i.set_on_hand(loc_id(2), -MAX_STOCK - 1, None),
            Err(InventoryError::StockOutOfRange(-i128::from(MAX_STOCK) - 1))
        );
    }

    #[test]
    fn max_stock_is_the_largest_exact_json_number() {
        assert_eq!(MAX_STOCK, hypha_core::Money::MAX_AMOUNT);
    }

    #[test]
    fn adjust_reports_overflow_instead_of_wrapping() {
        let mut i = item(&[(1, -MAX_STOCK)]);
        assert_eq!(
            i.adjust_on_hand(loc_id(1), i64::MIN),
            Err(InventoryError::StockOutOfRange(
                -i128::from(MAX_STOCK) + i128::from(i64::MIN)
            ))
        );
    }

    // ---- allocation ----

    #[test]
    fn takes_the_whole_line_from_the_first_location_that_has_it() {
        // Priority 0 has 2, priority 1 has 5: 3 units come from priority 1 alone.
        let mut i = item(&[(1, 2), (2, 5)]);
        i.reserve(&reference("a"), qty(3), &[location(2, 1), location(1, 0)])
            .unwrap();
        assert_eq!(held(&i), vec![(loc_id(2), 3)]);
    }

    #[test]
    fn prefers_higher_priority_when_both_have_enough() {
        let mut i = item(&[(1, 5), (2, 5)]);
        i.reserve(&reference("a"), qty(3), &[location(1, 9), location(2, 0)])
            .unwrap();
        assert_eq!(held(&i), vec![(loc_id(2), 3)]);
    }

    #[test]
    fn splits_in_priority_order_when_no_location_has_enough() {
        let mut i = item(&[(1, 2), (2, 1), (3, 4)]);
        i.reserve(
            &reference("a"),
            qty(5),
            &[location(1, 0), location(2, 1), location(3, 2)],
        )
        .unwrap();
        assert_eq!(
            held(&i),
            vec![(loc_id(1), 2), (loc_id(2), 1), (loc_id(3), 2)]
        );
    }

    #[test]
    fn equal_priorities_are_ordered_by_location_id() {
        let mut i = item(&[(1, 5), (2, 5)]);
        i.reserve(&reference("a"), qty(1), &[location(2, 0), location(1, 0)])
            .unwrap();
        assert_eq!(held(&i), vec![(loc_id(1), 1)]);
    }

    #[test]
    fn only_counts_the_locations_it_is_given() {
        // Location 2 has stock but doesn't serve this market.
        let mut i = item(&[(1, 1), (2, 10)]);
        assert_eq!(
            i.reserve(&reference("a"), qty(2), &[location(1, 0)]),
            Err(InventoryError::OutOfStock {
                requested: 2,
                available: 1
            })
        );
    }

    #[test]
    fn reservations_reduce_what_is_available() {
        let mut i = item(&[(1, 3)]);
        i.reserve(&reference("a"), qty(2), &[location(1, 0)])
            .unwrap();
        assert_eq!(i.available(loc_id(1)), 1);
        assert_eq!(
            i.reserve(&reference("b"), qty(2), &[location(1, 0)]),
            Err(InventoryError::OutOfStock {
                requested: 2,
                available: 1
            })
        );
    }

    #[test]
    fn exactly_the_available_quantity_can_be_reserved() {
        let mut i = item(&[(1, 2)]);
        assert_eq!(
            i.reserve(&reference("a"), qty(2), &[location(1, 0)]),
            Ok(true)
        );
        assert_eq!(i.available(loc_id(1)), 0);
    }

    #[test]
    fn negative_stock_counts_as_none_available() {
        let mut i = item(&[(1, -3), (2, 1)]);
        assert_eq!(
            i.reserve(&reference("a"), qty(2), &[location(1, 0), location(2, 1)]),
            Err(InventoryError::OutOfStock {
                requested: 2,
                available: 1
            })
        );
    }

    #[test]
    fn no_serving_location_means_nothing_available() {
        let mut i = item(&[(1, 5)]);
        i.set_continue_selling(true);
        assert_eq!(
            i.reserve(&reference("a"), qty(1), &[]),
            Err(InventoryError::OutOfStock {
                requested: 1,
                available: 0
            })
        );
    }

    // ---- continue selling ----

    #[test]
    fn continue_selling_puts_the_shortfall_on_the_first_location() {
        let mut i = item(&[(1, 0), (2, 2)]);
        i.set_continue_selling(true);
        i.reserve(&reference("a"), qty(5), &[location(1, 0), location(2, 1)])
            .unwrap();
        assert_eq!(held(&i), vec![(loc_id(1), 3), (loc_id(2), 2)]);
        assert_eq!(i.available(loc_id(1)), -3);
    }

    #[test]
    fn continue_selling_adds_the_shortfall_to_a_location_already_used() {
        let mut i = item(&[(1, 1)]);
        i.set_continue_selling(true);
        i.reserve(&reference("a"), qty(4), &[location(1, 0)])
            .unwrap();
        assert_eq!(held(&i), vec![(loc_id(1), 4)]);
    }

    // ---- tracking ----

    #[test]
    fn untracked_items_reserve_nothing_and_never_run_out() {
        let mut i = item(&[]);
        i.set_tracked(false);
        assert_eq!(i.can_supply(qty(1000), &[]), Ok(()));
        assert_eq!(i.reserve(&reference("a"), qty(1000), &[]), Ok(false));
        assert!(i.reservations().is_empty());
    }

    /// A location listed twice must not count its stock twice.
    #[test]
    fn a_location_listed_twice_counts_once() {
        let i = item(&[(1, 1)]);
        assert_eq!(
            i.can_supply(qty(2), &[location(1, 0), location(1, 0)]),
            Err(1)
        );
    }

    #[test]
    fn can_supply_reports_the_shortage_without_reserving() {
        let i = item(&[(1, 1)]);
        assert_eq!(i.can_supply(qty(2), &[location(1, 0)]), Err(1));
        assert_eq!(i.can_supply(qty(1), &[location(1, 0)]), Ok(()));
    }

    // ---- idempotency ----

    #[test]
    fn reserving_the_same_reference_again_changes_nothing() {
        let mut i = item(&[(1, 2), (2, 2)]);
        let locations = [location(1, 0), location(2, 1)];
        assert_eq!(i.reserve(&reference("a"), qty(3), &locations), Ok(true));
        let before = i.clone();
        assert_eq!(i.reserve(&reference("a"), qty(3), &locations), Ok(false));
        assert_eq!(i, before);
    }

    #[test]
    fn the_same_reference_for_another_quantity_is_refused() {
        let mut i = item(&[(1, 5)]);
        i.reserve(&reference("a"), qty(2), &[location(1, 0)])
            .unwrap();
        assert_eq!(
            i.reserve(&reference("a"), qty(3), &[location(1, 0)]),
            Err(InventoryError::ReferenceInUse(reference("a")))
        );
    }

    #[test]
    fn release_gives_back_every_location_of_a_reference() {
        let mut i = item(&[(1, 1), (2, 1)]);
        let locations = [location(1, 0), location(2, 1)];
        i.reserve(&reference("a"), qty(2), &locations).unwrap();
        assert!(i.release(&reference("a")));
        assert_eq!(i.available(loc_id(1)), 1);
        assert_eq!(i.available(loc_id(2)), 1);
        assert!(!i.release(&reference("a")), "second release is a no-op");
    }

    // ---- fulfilment ----

    fn held_by(item: &InventoryItem, r: &str) -> Vec<(LocationId, u32)> {
        item.reservations()
            .iter()
            .filter(|h| h.reference.as_str() == r)
            .map(|h| (h.location, h.quantity.value()))
            .collect()
    }

    #[test]
    fn fulfilling_takes_the_reservation_and_the_stock_at_the_location() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(3), &[location(1, 0)])
            .unwrap();
        assert_eq!(item.fulfil(&reference("a"), loc_id(1), qty(2)), Ok(true));
        assert_eq!(item.on_hand(loc_id(1)), 3);
        assert_eq!(held_by(&item, "a"), [(loc_id(1), 1)]);
        assert_eq!(item.available(loc_id(1)), 2);
        assert_eq!(item.fulfil(&reference("a"), loc_id(1), qty(1)), Ok(true));
        assert_eq!(held_by(&item, "a"), []);
        assert_eq!(item.on_hand(loc_id(1)), 2);
    }

    #[test]
    fn fulfilling_from_another_location_releases_the_reservation_there() {
        let mut item = item(&[(1, 5), (2, 5)]);
        item.reserve(&reference("a"), qty(2), &[location(1, 0)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(2), qty(2)).unwrap();
        assert_eq!(held_by(&item, "a"), []);
        assert_eq!((item.on_hand(loc_id(1)), item.on_hand(loc_id(2))), (5, 3));
    }

    #[test]
    fn fulfilling_a_split_line_takes_here_first_then_elsewhere() {
        // 2 reserved at 1 and 2 at 2 (a split line); 3 leave from location 2.
        let mut item = item(&[(1, 2), (2, 2)]);
        item.reserve(&reference("a"), qty(4), &[location(1, 0), location(2, 1)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(2), qty(3)).unwrap();
        assert_eq!(held_by(&item, "a"), [(loc_id(1), 1)]);
        assert_eq!(item.on_hand(loc_id(2)), -1);
    }

    #[test]
    fn fulfilling_a_whole_split_line_here_releases_the_rest_elsewhere() {
        let mut item = item(&[(1, 2), (2, 3)]);
        item.reserve(&reference("a"), qty(5), &[location(1, 0), location(2, 1)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(1), qty(5)).unwrap();
        assert_eq!(held_by(&item, "a"), []);
    }

    #[test]
    fn fulfilling_leaves_other_references_alone() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(1), &[location(1, 0)])
            .unwrap();
        item.reserve(&reference("b"), qty(2), &[location(1, 0)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(1), qty(1)).unwrap();
        assert_eq!(held_by(&item, "b"), [(loc_id(1), 2)]);
    }

    #[test]
    fn fulfilling_unreserved_units_still_counts_them_out() {
        // Tracking was turned on after the order was placed: nothing was reserved.
        let mut item = item(&[(1, 1)]);
        assert_eq!(item.fulfil(&reference("a"), loc_id(1), qty(3)), Ok(true));
        assert_eq!(item.on_hand(loc_id(1)), -2);
    }

    #[test]
    fn fulfilling_an_untracked_item_changes_no_count() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(1), &[location(1, 0)])
            .unwrap();
        item.set_tracked(false);
        assert_eq!(item.fulfil(&reference("a"), loc_id(1), qty(1)), Ok(true));
        assert_eq!(item.on_hand(loc_id(1)), 5);
        assert_eq!(held_by(&item, "a"), []);
        assert_eq!(item.fulfil(&reference("a"), loc_id(1), qty(1)), Ok(false));
    }

    #[test]
    fn fulfilling_part_of_an_untracked_items_reservation_gives_that_part_back() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(3), &[location(1, 0)])
            .unwrap();
        item.set_tracked(false);
        assert_eq!(item.fulfil(&reference("a"), loc_id(1), qty(1)), Ok(true));
        assert_eq!(held_by(&item, "a"), [(loc_id(1), 2)]);
    }

    #[test]
    fn fulfilling_past_the_stock_range_changes_nothing() {
        let mut item = item(&[(1, -MAX_STOCK)]);
        let before = item.clone();
        assert_eq!(
            item.fulfil(&reference("a"), loc_id(1), qty(1)),
            Err(InventoryError::StockOutOfRange(-i128::from(MAX_STOCK) - 1))
        );
        assert_eq!(item, before);
    }

    #[test]
    fn restocking_puts_the_units_back_and_reserves_them_again() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(3), &[location(1, 0)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(1), qty(2)).unwrap();
        assert_eq!(item.restock(&reference("a"), loc_id(1), qty(2)), Ok(()));
        assert_eq!(item.on_hand(loc_id(1)), 5);
        assert_eq!(held_by(&item, "a"), [(loc_id(1), 3)]);
    }

    #[test]
    fn restocking_leaves_another_references_reservation_alone() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(1), &[location(1, 0)])
            .unwrap();
        item.reserve(&reference("b"), qty(1), &[location(1, 0)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(1), qty(1)).unwrap();
        item.restock(&reference("a"), loc_id(1), qty(1)).unwrap();
        assert_eq!(held_by(&item, "a"), [(loc_id(1), 1)]);
        assert_eq!(held_by(&item, "b"), [(loc_id(1), 1)]);
    }

    #[test]
    fn restocking_where_nothing_was_left_reserved_reserves_anew() {
        let mut item = item(&[(1, 5), (2, 5)]);
        item.reserve(&reference("a"), qty(1), &[location(1, 0)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(2), qty(1)).unwrap();
        item.restock(&reference("a"), loc_id(2), qty(1)).unwrap();
        assert_eq!(held_by(&item, "a"), [(loc_id(2), 1)]);
        assert_eq!(item.on_hand(loc_id(2)), 5);
    }

    #[test]
    fn restocking_an_item_untracked_since_puts_the_count_back_and_reserves_nothing() {
        // Shipped while tracked (4 left), then tracking was turned off.
        let mut item = item(&[(1, 4)]);
        item.set_tracked(false);
        assert_eq!(item.restock(&reference("a"), loc_id(1), qty(1)), Ok(()));
        assert_eq!(item.on_hand(loc_id(1)), 5);
        assert_eq!(held_by(&item, "a"), []);
    }

    #[test]
    fn a_reservation_past_its_range_is_refused_and_changes_nothing() {
        let mut item = item(&[(1, 5)]);
        let at = [location(1, 0)];
        item.set_continue_selling(true);
        item.reserve(&reference("a"), qty(u32::MAX), &at).unwrap();
        let before = item.clone();
        assert_eq!(
            item.restock(&reference("a"), loc_id(1), qty(1)),
            Err(InventoryError::Quantity(QuantityError::Overflow))
        );
        assert_eq!(item, before);
    }

    #[test]
    fn a_retired_item_is_spent_once_nothing_is_reserved() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(1), &[location(1, 0)])
            .unwrap();
        assert!(!item.is_spent());
        item.retire();
        assert!(!item.is_spent());
        item.fulfil(&reference("a"), loc_id(1), qty(1)).unwrap();
        assert!(item.is_spent());
    }

    #[test]
    fn an_item_with_reservations_cant_be_deleted() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(1), &[location(1, 0)])
            .unwrap();
        assert_eq!(item.delete(), Err(InventoryError::StillReserved));
        item.release(&reference("a"));
        assert_eq!(item.delete(), Ok(()));
    }

    #[test]
    fn an_item_that_isnt_retired_is_never_spent() {
        assert!(!item(&[(1, 5)]).is_spent());
    }

    #[test]
    fn retired_survives_storage() {
        let mut item = item(&[(1, 5)]);
        item.retire();
        assert!(InventoryItem::restore(item.snapshot()).unwrap().retired());
    }

    // ---- storage ----

    #[test]
    fn restore_round_trips() {
        let mut i = item(&[(1, 4), (2, -1)]);
        i.reserve(&reference("a"), qty(2), &[location(1, 0)])
            .unwrap();
        i.set_version(Version::new(7));
        assert_eq!(InventoryItem::restore(i.snapshot()), Ok(i));
    }

    #[test]
    fn restore_rejects_two_levels_for_one_location() {
        let mut s = item(&[(1, 4)]).snapshot();
        s.levels.push(Level {
            location: loc_id(1),
            on_hand: 1,
        });
        assert_eq!(
            InventoryItem::restore(s),
            Err(RestoreError::DuplicateLevel(loc_id(1)))
        );
    }

    #[test]
    fn restore_rejects_two_reservations_for_one_reference_and_location() {
        let mut s = item(&[(1, 4)]).snapshot();
        let r = Reservation {
            reference: reference("a"),
            location: loc_id(1),
            quantity: qty(1),
        };
        s.reservations = vec![r.clone(), r];
        assert_eq!(
            InventoryItem::restore(s),
            Err(RestoreError::DuplicateReservation {
                reference: reference("a"),
                location: loc_id(1)
            })
        );
    }
}
