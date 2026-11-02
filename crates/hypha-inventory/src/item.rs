use std::collections::HashSet;

use hypha_core::{
    Currency, InventoryItemId, LocationId, PendingEvent, Quantity, Recorded, Recorder, Stamped,
    Stamps, Subject, UnitCost, VariantId, Version, Versioned,
};

use crate::{
    AdjustmentReason, Cause, InventoryError, Location, MovementNote, ReservationRef, RestoreError,
    StockMovement, StockReason, StockSource,
    events::{
        Available, CostChange, INVENTORY_ITEM, InventoryItemChanged, InventoryItemCostChanged,
        InventoryItemDeleted,
    },
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
    /// What one unit cost, in the store's currency: the moving average over what came in,
    /// or what was set by hand. `None` until known.
    unit_cost: Option<UnitCost>,
    /// Whether anything changed since it was loaded: its event is built from the item as it
    /// is when written.
    changed: Changed,
    /// Its deletion, once `delete` was called.
    deleted: Recorder,
    /// `inventory_item.cost_changed`, apart from `changed`: its readers differ.
    cost_changes: Recorder,
    /// Changes to its counts not written yet: the adapter writes them with the item.
    movements: Unwritten,
}

/// Movements waiting to be written. Like `Changed`, they never make two items unequal: an
/// item compares by its state.
#[derive(Debug, Clone, Default)]
struct Unwritten(Vec<StockMovement>);

impl PartialEq for Unwritten {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Unwritten {}

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
        let mut events = Vec::new();
        if std::mem::take(&mut self.changed).0 {
            events.push(self.changed_event());
        }
        events.extend(self.cost_changes.take());
        events
    }
}

impl InventoryItem {
    /// `inventory_item.changed` for the item as it is now.
    fn changed_event(&self) -> PendingEvent {
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
        PendingEvent::new(Subject::new(INVENTORY_ITEM, self.id.as_uuid()), &event)
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
            unit_cost: None,
            changed: Changed(true),
            deleted: Recorder::default(),
            cost_changes: Recorder::default(),
            movements: Unwritten::default(),
        }
    }

    /// The movements recorded since the last call, oldest first: an adapter takes them as
    /// it writes the item, in the same transaction, as it takes its events. Take them
    /// before copying the item into a batch, so the stored copy carries none.
    pub fn take_movements(&mut self) -> Vec<StockMovement> {
        std::mem::take(&mut self.movements.0)
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

    /// Sets the count at `location`, e.g. after counting the shelf: a `counted` movement
    /// by hand, with `note`.
    ///
    /// With `expected`, the change only happens if `on_hand` is still what the caller
    /// last saw (Shopify's `changeFromQuantity`): a sale in between isn't silently
    /// overwritten. A location without a level counts as 0.
    pub fn set_on_hand(
        &mut self,
        location: LocationId,
        quantity: i64,
        expected: Option<i64>,
        note: Option<MovementNote>,
    ) -> Result<(), InventoryError> {
        check_range(i128::from(quantity))?;
        let actual = self.on_hand(location);
        if let Some(expected) = expected
            && expected != actual
        {
            return Err(InventoryError::OnHandChanged { expected, actual });
        }
        let cause = Cause::new(StockReason::Counted, StockSource::Manual).with_note(note);
        self.write_level(location, quantity, cause);
        Ok(())
    }

    /// Adds `delta` (negative to remove) to the count at `location`, by hand, for
    /// `reason`.
    pub fn adjust_on_hand(
        &mut self,
        location: LocationId,
        delta: i64,
        reason: AdjustmentReason,
        note: Option<MovementNote>,
    ) -> Result<(), InventoryError> {
        let quantity = check_range(i128::from(self.on_hand(location)) + i128::from(delta))?;
        let cause = Cause::new(reason.reason(), StockSource::Manual).with_note(note);
        self.write_level(location, quantity, cause);
        Ok(())
    }

    /// A count changes at the item's cost, which stays: the movement records it.
    fn write_level(&mut self, location: LocationId, on_hand: i64, cause: Cause) {
        let cost = self.unit_cost;
        self.write_level_at(location, on_hand, cause, cost, cost);
    }

    /// The one place a count changes: it records the movement, so none goes unrecorded.
    /// `unit_cost` is what the units moving cost and `average_after` the item's cost
    /// after, which the caller sets.
    fn write_level_at(
        &mut self,
        location: LocationId,
        on_hand: i64,
        cause: Cause,
        unit_cost: Option<UnitCost>,
        average_after: Option<UnitCost>,
    ) {
        let before = match self.levels.iter_mut().find(|l| l.location == location) {
            Some(level) if level.on_hand == on_hand => return,
            Some(level) => std::mem::replace(&mut level.on_hand, on_hand),
            None => {
                self.levels.push(Level { location, on_hand });
                0
            }
        };
        self.changed = Changed(true);
        // Both counts are within ±MAX_STOCK, so their difference fits.
        let delta = on_hand - before;
        if delta != 0 {
            self.movements.0.push(StockMovement {
                item: self.id,
                variant: self.variant,
                location: Some(location),
                delta,
                after: Some(on_hand),
                reason: cause.reason,
                source: cause.source,
                note: cause.note,
                unit_cost,
                average_after,
            });
        }
    }

    /// Sets the unit cost by hand (`None` clears it): a `cost_adjustment` row, with the
    /// cost before as its `unit_cost` and the new one as its `average_after`, and
    /// `inventory_item.cost_changed`. The caller checks the currency is the store's.
    /// Returns whether anything changed.
    pub fn set_unit_cost(&mut self, cost: Option<UnitCost>, note: Option<MovementNote>) -> bool {
        if self.unit_cost == cost {
            return false;
        }
        let cause = Cause::new(StockReason::CostAdjustment, StockSource::Manual).with_note(note);
        self.change_cost(cost, cause, CostChange::Manual);
        true
    }

    /// A cost that came late (an extra cost added to, changed on or removed from a
    /// purchase order after its goods arrived): `value` in the store's `currency`, in
    /// `UnitCost`'s scaled units and of either sign, spread over the units on hand across
    /// every location. The part that belongs to units already gone isn't recovered: with
    /// nothing on hand, or nothing to change, nothing happens. The cost never goes below
    /// zero. A `cost_adjustment` row caused by `source` and `inventory_item.cost_changed`
    /// (`late_cost`). Returns whether the cost changed.
    pub fn revalue(
        &mut self,
        value: i128,
        currency: Currency,
        source: StockSource,
    ) -> Result<bool, InventoryError> {
        let held = self.total_on_hand();
        if value == 0 || held <= 0 {
            return Ok(false);
        }
        let before = self.unit_cost.unwrap_or_else(|| UnitCost::zero(currency));
        if before.currency() != currency {
            return Err(InventoryError::CostCurrency {
                expected: before.currency(),
                got: currency,
            });
        }
        let after = before.revalued(value, held)?;
        if Some(after) == self.unit_cost || (self.unit_cost.is_none() && after.scaled() == 0) {
            return Ok(false);
        }
        let cause = Cause::new(StockReason::CostAdjustment, source);
        self.change_cost(Some(after), cause, CostChange::LateCost);
        Ok(true)
    }

    /// The one place the unit cost changes: it records the row and the event, so no
    /// change goes unrecorded.
    fn change_cost(&mut self, cost: Option<UnitCost>, cause: Cause, why: CostChange) {
        let before = std::mem::replace(&mut self.unit_cost, cost);
        self.movements.0.push(StockMovement {
            item: self.id,
            variant: self.variant,
            location: None,
            delta: 0,
            after: None,
            reason: cause.reason,
            source: cause.source,
            note: cause.note,
            unit_cost: before,
            average_after: cost,
        });
        self.record_cost_change(why);
    }

    /// `inventory_item.cost_changed` for the cost the item has now.
    fn record_cost_change(&mut self, why: CostChange) {
        let event = InventoryItemCostChanged {
            inventory_item: self.id,
            variant: self.variant,
            unit_cost: self.unit_cost,
            why,
        };
        self.cost_changes
            .record(Subject::new(INVENTORY_ITEM, self.id.as_uuid()), &event);
    }

    /// Goods arrived from a supplier: `quantity` more on hand at `location`, a `received`
    /// movement caused by `source` at `landed` (what each unit cost, in the store's
    /// currency). The cost becomes the moving average over the units on hand before
    /// (across every location; none when that's 0 or less) and those arriving:
    /// `(Q·A + q·c) / (Q + q)`, or `landed` itself without a cost before or units on hand.
    /// Untracked items count it too, so the count is right when tracking is turned on.
    /// Arriving without a cost leaves the average as it is. All or nothing.
    pub fn receive(
        &mut self,
        location: LocationId,
        quantity: Quantity,
        landed: Option<UnitCost>,
        source: StockSource,
    ) -> Result<(), InventoryError> {
        let q = i128::from(quantity.value());
        let on_hand = check_range(i128::from(self.on_hand(location)) + q)?;
        let average = match (landed, self.unit_cost) {
            (None, kept) => kept,
            (Some(cost), None) => Some(cost),
            (Some(cost), Some(average)) => {
                same_currency(average, cost)?;
                let held = self.total_on_hand().max(0);
                if held == 0 {
                    Some(cost)
                } else {
                    Some(weighted(
                        held * i128::from(average.scaled()) + q * i128::from(cost.scaled()),
                        held + q,
                        cost,
                    )?)
                }
            }
        };
        self.move_with_cost(
            location,
            on_hand,
            source,
            landed,
            average,
            CostChange::Received,
        );
        Ok(())
    }

    /// Takes back a receipt recorded by mistake: `quantity` fewer on hand at `location`
    /// (which may go below zero), a `received` movement with a negative delta. The
    /// receipt's value comes out of the average, `(Q·A − q·c) / (Q − q)`, when units are
    /// left and so is value; otherwise the average stays. All or nothing.
    pub fn unreceive(
        &mut self,
        location: LocationId,
        quantity: Quantity,
        landed: Option<UnitCost>,
        source: StockSource,
    ) -> Result<(), InventoryError> {
        let q = i128::from(quantity.value());
        let on_hand = check_range(i128::from(self.on_hand(location)) - q)?;
        let average = match (landed, self.unit_cost) {
            (Some(cost), Some(average)) => {
                same_currency(average, cost)?;
                let held = self.total_on_hand();
                let value = held * i128::from(average.scaled()) - q * i128::from(cost.scaled());
                if held - q > 0 && value > 0 {
                    Some(weighted(value, held - q, cost)?)
                } else {
                    Some(average)
                }
            }
            (_, kept) => kept,
        };
        self.move_with_cost(
            location,
            on_hand,
            source,
            landed,
            average,
            CostChange::ReceiptCancelled,
        );
        Ok(())
    }

    /// A `received` movement to `on_hand` at `location`, its units at `landed` (the
    /// item's cost when unknown), leaving the item at `average`: one row, and the cost
    /// event when the cost changed.
    fn move_with_cost(
        &mut self,
        location: LocationId,
        on_hand: i64,
        source: StockSource,
        landed: Option<UnitCost>,
        average: Option<UnitCost>,
        why: CostChange,
    ) {
        let cause = Cause::new(StockReason::Received, source);
        let unit_cost = landed.or(self.unit_cost);
        self.write_level_at(location, on_hand, cause, unit_cost, average);
        if average != self.unit_cost {
            self.unit_cost = average;
            self.record_cost_change(why);
        }
    }

    /// Units of a transfer leaving `location`, its origin: `quantity` fewer on hand there
    /// (which may go below zero, as fulfilling can), a `transfer_out` movement caused by
    /// `source`. The cost stays: it's the item's, wherever the units are. Untracked items
    /// count it too. All or nothing.
    pub fn transfer_out(
        &mut self,
        location: LocationId,
        quantity: Quantity,
        source: StockSource,
    ) -> Result<(), InventoryError> {
        let on_hand =
            check_range(i128::from(self.on_hand(location)) - i128::from(quantity.value()))?;
        let cause = Cause::new(StockReason::TransferOut, source);
        self.write_level(location, on_hand, cause);
        Ok(())
    }

    /// Units of a transfer arriving at `location`, its destination: `quantity` more on
    /// hand there, a `transfer_in` movement caused by `source`. All or nothing.
    pub fn transfer_in(
        &mut self,
        location: LocationId,
        quantity: Quantity,
        source: StockSource,
    ) -> Result<(), InventoryError> {
        let on_hand =
            check_range(i128::from(self.on_hand(location)) + i128::from(quantity.value()))?;
        let cause = Cause::new(StockReason::TransferIn, source);
        self.write_level(location, on_hand, cause);
        Ok(())
    }

    /// Units of a transfer lost on the way to `location`: the origin gave them up when
    /// they shipped, so no count changes, but a `shrinkage` row with no delta at the
    /// destination says where they went, its note saying how many.
    pub fn lose_in_transit(
        &mut self,
        location: LocationId,
        quantity: Quantity,
        source: StockSource,
    ) {
        let note = MovementNote::new(&format!("{} lost in transit", quantity.value()))
            .ok()
            .flatten();
        let cost = self.unit_cost;
        self.movements.0.push(StockMovement {
            item: self.id,
            variant: self.variant,
            location: Some(location),
            delta: 0,
            after: Some(self.on_hand(location)),
            reason: StockReason::Shrinkage,
            source,
            note,
            unit_cost: cost,
            average_after: cost,
        });
    }

    /// On hand summed over every location.
    fn total_on_hand(&self) -> i128 {
        self.levels.iter().map(|l| i128::from(l.on_hand)).sum()
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
    /// Units leaving are `sold` movements caused by `source` (the fulfilment).
    ///
    /// All or nothing; returns whether anything changed.
    pub fn fulfil(
        &mut self,
        reference: &ReservationRef,
        location: LocationId,
        quantity: Quantity,
        source: StockSource,
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
            self.write_level(location, on_hand, Cause::new(StockReason::Sold, source));
        }
        Ok(released || on_hand.is_some())
    }

    /// Reverses `fulfil` for a cancelled fulfilment whose units came off this item's
    /// stock (the caller knows: `FulfillmentLine::stock_taken`): `quantity` is back on
    /// hand at `location`, even if tracking was turned off since, so the count is right
    /// when it's turned on again. While tracked, the units are reserved there for
    /// `reference` again; untracked items reserve nothing.
    ///
    /// The units coming back are a `restocked` movement caused by `source`.
    ///
    /// All or nothing: a count or reservation past its range changes nothing.
    pub fn restock(
        &mut self,
        reference: &ReservationRef,
        location: LocationId,
        quantity: Quantity,
        source: StockSource,
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
        self.write_level(
            location,
            on_hand,
            Cause::new(StockReason::Restocked, source),
        );
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

    /// What one unit cost, in the store's currency; `None` until known.
    pub fn unit_cost(&self) -> Option<UnitCost> {
        self.unit_cost
    }
}

/// Costs averaged together are in one currency, the store's.
fn same_currency(average: UnitCost, cost: UnitCost) -> Result<(), InventoryError> {
    if average.currency() == cost.currency() {
        Ok(())
    } else {
        Err(InventoryError::CostCurrency {
            expected: average.currency(),
            got: cost.currency(),
        })
    }
}

/// `value / units` (`units` positive, `value` in `UnitCost`'s scaled units) as a cost in
/// `like`'s currency, rounded half away from zero.
fn weighted(value: i128, units: i128, like: UnitCost) -> Result<UnitCost, InventoryError> {
    let rounded = hypha_core::money::div_round(value, units);
    let scaled = i64::try_from(rounded).map_err(|_| hypha_core::MoneyError::Overflow)?;
    Ok(UnitCost::new(scaled, like.currency())?)
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
    pub unit_cost: Option<UnitCost>,
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
            unit_cost: self.unit_cost,
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
            unit_cost: s.unit_cost,
            changed: Changed(false),
            deleted: Recorder::default(),
            cost_changes: Recorder::default(),
            movements: Unwritten::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{FulfillmentId, MarketId, OrderId, QuantityError};
    use uuid::Uuid;

    use super::*;

    /// What units left or came back for, in the fulfilment tests.
    fn source() -> StockSource {
        StockSource::Fulfillment {
            order: OrderId::from_uuid(Uuid::from_u128(80)),
            fulfillment: FulfillmentId::from_uuid(Uuid::from_u128(81)),
        }
    }

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
            item.set_on_hand(loc_id(*location), *on_hand, None, None)
                .unwrap();
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
        i.set_on_hand(loc_id(1), 2, None, None).unwrap();
        assert_eq!(i.on_hand(loc_id(1)), 2);
        assert_eq!(i.levels().len(), 1);
    }

    #[test]
    fn set_on_hand_with_the_expected_count_succeeds() {
        let mut i = item(&[(1, 5)]);
        assert_eq!(i.set_on_hand(loc_id(1), 7, Some(5), None), Ok(()));
        assert_eq!(i.on_hand(loc_id(1)), 7);
    }

    #[test]
    fn set_on_hand_refuses_when_the_count_moved() {
        let mut i = item(&[(1, 4)]);
        assert_eq!(
            i.set_on_hand(loc_id(1), 10, Some(5), None),
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
        assert_eq!(i.set_on_hand(loc_id(1), 3, Some(0), None), Ok(()));
    }

    #[test]
    fn adjust_adds_and_removes() {
        let mut i = item(&[(1, 5)]);
        i.adjust_on_hand(loc_id(1), -7, AdjustmentReason::Correction, None)
            .unwrap();
        assert_eq!(i.on_hand(loc_id(1)), -2);
        i.adjust_on_hand(loc_id(2), 4, AdjustmentReason::Correction, None)
            .unwrap();
        assert_eq!(i.on_hand(loc_id(2)), 4);
    }

    #[test]
    fn stock_is_capped_at_max_stock_either_way() {
        let mut i = item(&[]);
        assert_eq!(i.set_on_hand(loc_id(1), MAX_STOCK, None, None), Ok(()));
        assert_eq!(
            i.adjust_on_hand(loc_id(1), 1, AdjustmentReason::Correction, None),
            Err(InventoryError::StockOutOfRange(i128::from(MAX_STOCK) + 1))
        );
        assert_eq!(i.set_on_hand(loc_id(2), -MAX_STOCK, None, None), Ok(()));
        assert_eq!(
            i.set_on_hand(loc_id(2), -MAX_STOCK - 1, None, None),
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
            i.adjust_on_hand(loc_id(1), i64::MIN, AdjustmentReason::Correction, None),
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
        assert_eq!(
            item.fulfil(&reference("a"), loc_id(1), qty(2), source()),
            Ok(true)
        );
        assert_eq!(item.on_hand(loc_id(1)), 3);
        assert_eq!(held_by(&item, "a"), [(loc_id(1), 1)]);
        assert_eq!(item.available(loc_id(1)), 2);
        assert_eq!(
            item.fulfil(&reference("a"), loc_id(1), qty(1), source()),
            Ok(true)
        );
        assert_eq!(held_by(&item, "a"), []);
        assert_eq!(item.on_hand(loc_id(1)), 2);
    }

    #[test]
    fn fulfilling_from_another_location_releases_the_reservation_there() {
        let mut item = item(&[(1, 5), (2, 5)]);
        item.reserve(&reference("a"), qty(2), &[location(1, 0)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(2), qty(2), source())
            .unwrap();
        assert_eq!(held_by(&item, "a"), []);
        assert_eq!((item.on_hand(loc_id(1)), item.on_hand(loc_id(2))), (5, 3));
    }

    #[test]
    fn fulfilling_a_split_line_takes_here_first_then_elsewhere() {
        // 2 reserved at 1 and 2 at 2 (a split line); 3 leave from location 2.
        let mut item = item(&[(1, 2), (2, 2)]);
        item.reserve(&reference("a"), qty(4), &[location(1, 0), location(2, 1)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(2), qty(3), source())
            .unwrap();
        assert_eq!(held_by(&item, "a"), [(loc_id(1), 1)]);
        assert_eq!(item.on_hand(loc_id(2)), -1);
    }

    #[test]
    fn fulfilling_a_whole_split_line_here_releases_the_rest_elsewhere() {
        let mut item = item(&[(1, 2), (2, 3)]);
        item.reserve(&reference("a"), qty(5), &[location(1, 0), location(2, 1)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(1), qty(5), source())
            .unwrap();
        assert_eq!(held_by(&item, "a"), []);
    }

    #[test]
    fn fulfilling_leaves_other_references_alone() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(1), &[location(1, 0)])
            .unwrap();
        item.reserve(&reference("b"), qty(2), &[location(1, 0)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(1), qty(1), source())
            .unwrap();
        assert_eq!(held_by(&item, "b"), [(loc_id(1), 2)]);
    }

    #[test]
    fn fulfilling_unreserved_units_still_counts_them_out() {
        // Tracking was turned on after the order was placed: nothing was reserved.
        let mut item = item(&[(1, 1)]);
        assert_eq!(
            item.fulfil(&reference("a"), loc_id(1), qty(3), source()),
            Ok(true)
        );
        assert_eq!(item.on_hand(loc_id(1)), -2);
    }

    #[test]
    fn fulfilling_an_untracked_item_changes_no_count() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(1), &[location(1, 0)])
            .unwrap();
        item.set_tracked(false);
        assert_eq!(
            item.fulfil(&reference("a"), loc_id(1), qty(1), source()),
            Ok(true)
        );
        assert_eq!(item.on_hand(loc_id(1)), 5);
        assert_eq!(held_by(&item, "a"), []);
        assert_eq!(
            item.fulfil(&reference("a"), loc_id(1), qty(1), source()),
            Ok(false)
        );
    }

    #[test]
    fn fulfilling_part_of_an_untracked_items_reservation_gives_that_part_back() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(3), &[location(1, 0)])
            .unwrap();
        item.set_tracked(false);
        assert_eq!(
            item.fulfil(&reference("a"), loc_id(1), qty(1), source()),
            Ok(true)
        );
        assert_eq!(held_by(&item, "a"), [(loc_id(1), 2)]);
    }

    #[test]
    fn fulfilling_past_the_stock_range_changes_nothing() {
        let mut item = item(&[(1, -MAX_STOCK)]);
        let before = item.clone();
        assert_eq!(
            item.fulfil(&reference("a"), loc_id(1), qty(1), source()),
            Err(InventoryError::StockOutOfRange(-i128::from(MAX_STOCK) - 1))
        );
        assert_eq!(item, before);
    }

    #[test]
    fn restocking_puts_the_units_back_and_reserves_them_again() {
        let mut item = item(&[(1, 5)]);
        item.reserve(&reference("a"), qty(3), &[location(1, 0)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(1), qty(2), source())
            .unwrap();
        assert_eq!(
            item.restock(&reference("a"), loc_id(1), qty(2), source()),
            Ok(())
        );
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
        item.fulfil(&reference("a"), loc_id(1), qty(1), source())
            .unwrap();
        item.restock(&reference("a"), loc_id(1), qty(1), source())
            .unwrap();
        assert_eq!(held_by(&item, "a"), [(loc_id(1), 1)]);
        assert_eq!(held_by(&item, "b"), [(loc_id(1), 1)]);
    }

    #[test]
    fn restocking_where_nothing_was_left_reserved_reserves_anew() {
        let mut item = item(&[(1, 5), (2, 5)]);
        item.reserve(&reference("a"), qty(1), &[location(1, 0)])
            .unwrap();
        item.fulfil(&reference("a"), loc_id(2), qty(1), source())
            .unwrap();
        item.restock(&reference("a"), loc_id(2), qty(1), source())
            .unwrap();
        assert_eq!(held_by(&item, "a"), [(loc_id(2), 1)]);
        assert_eq!(item.on_hand(loc_id(2)), 5);
    }

    #[test]
    fn restocking_an_item_untracked_since_puts_the_count_back_and_reserves_nothing() {
        // Shipped while tracked (4 left), then tracking was turned off.
        let mut item = item(&[(1, 4)]);
        item.set_tracked(false);
        assert_eq!(
            item.restock(&reference("a"), loc_id(1), qty(1), source()),
            Ok(())
        );
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
            item.restock(&reference("a"), loc_id(1), qty(1), source()),
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
        item.fulfil(&reference("a"), loc_id(1), qty(1), source())
            .unwrap();
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

    /// Each recorded movement of units as (location, delta, after, reason), taking them.
    fn moved(item: &mut InventoryItem) -> Vec<(LocationId, i64, i64, StockReason)> {
        item.take_movements()
            .into_iter()
            .filter_map(|m| Some((m.location?, m.delta, m.after?, m.reason)))
            .collect()
    }

    fn note(text: &str) -> Option<MovementNote> {
        MovementNote::new(text).unwrap()
    }

    #[test]
    fn a_count_records_the_difference_by_hand() {
        let mut i = item(&[(1, 4)]);
        assert_eq!(moved(&mut i), [(loc_id(1), 4, 4, StockReason::Counted)]);
        i.set_on_hand(loc_id(1), 1, None, note("shelf count"))
            .unwrap();
        let movements = i.take_movements();
        assert_eq!(movements.len(), 1);
        let m = &movements[0];
        assert_eq!(
            (m.item, m.variant, m.delta, m.after, m.source),
            (i.id(), i.variant(), -3, Some(1), StockSource::Manual)
        );
        assert_eq!(
            m.note.as_ref().map(MovementNote::as_str),
            Some("shelf count")
        );
    }

    #[test]
    fn a_count_that_changes_nothing_records_nothing() {
        let mut i = item(&[(1, 4)]);
        i.take_movements();
        i.set_on_hand(loc_id(1), 4, None, None).unwrap();
        i.set_on_hand(loc_id(2), 0, None, None).unwrap();
        i.adjust_on_hand(loc_id(1), 0, AdjustmentReason::Other, None)
            .unwrap();
        assert_eq!(moved(&mut i), []);
    }

    #[test]
    fn an_adjustment_records_its_reason() {
        let mut i = item(&[(1, 4)]);
        i.take_movements();
        i.adjust_on_hand(loc_id(1), -2, AdjustmentReason::Damaged, None)
            .unwrap();
        i.adjust_on_hand(loc_id(2), 6, AdjustmentReason::Correction, None)
            .unwrap();
        assert_eq!(
            moved(&mut i),
            [
                (loc_id(1), -2, 2, StockReason::Damaged),
                (loc_id(2), 6, 6, StockReason::Correction)
            ]
        );
    }

    #[test]
    fn a_refused_change_records_nothing() {
        let mut i = item(&[(1, 4)]);
        i.take_movements();
        assert!(i.set_on_hand(loc_id(1), 9, Some(5), None).is_err());
        assert!(
            i.adjust_on_hand(loc_id(1), MAX_STOCK, AdjustmentReason::Other, None)
                .is_err()
        );
        assert_eq!(moved(&mut i), []);
    }

    #[test]
    fn fulfilling_and_restocking_record_sold_and_restocked_by_their_fulfilment() {
        let mut i = item(&[(1, 5)]);
        i.reserve(&reference("a"), qty(3), &[location(1, 0)])
            .unwrap();
        i.take_movements();
        i.fulfil(&reference("a"), loc_id(1), qty(3), source())
            .unwrap();
        i.restock(&reference("a"), loc_id(1), qty(1), source())
            .unwrap();
        let movements = i.take_movements();
        assert_eq!(
            movements
                .iter()
                .map(|m| (m.delta, m.after, m.reason, m.source))
                .collect::<Vec<_>>(),
            [
                (-3, Some(2), StockReason::Sold, source()),
                (1, Some(3), StockReason::Restocked, source())
            ]
        );
    }

    #[test]
    fn an_untracked_items_fulfilment_records_nothing_but_its_restock_counts() {
        let mut i = item(&[(1, 5)]);
        i.set_tracked(false);
        i.take_movements();
        i.fulfil(&reference("a"), loc_id(1), qty(2), source())
            .unwrap();
        assert_eq!(moved(&mut i), []);
        i.restock(&reference("a"), loc_id(1), qty(2), source())
            .unwrap();
        assert_eq!(moved(&mut i), [(loc_id(1), 2, 7, StockReason::Restocked)]);
    }

    #[test]
    fn movements_are_taken_once_and_never_stored() {
        let mut i = item(&[(1, 4)]);
        assert_eq!(i.take_movements().len(), 1);
        assert_eq!(i.take_movements(), []);
        i.adjust_on_hand(loc_id(1), 1, AdjustmentReason::Other, None)
            .unwrap();
        let mut restored = InventoryItem::restore(i.snapshot()).unwrap();
        assert_eq!(restored.take_movements(), []);
    }

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

    // ---- cost ----

    fn eur(minor_units: &str) -> UnitCost {
        UnitCost::parse(minor_units, hypha_core::Currency::EUR).unwrap()
    }

    #[test]
    fn a_new_item_has_no_cost() {
        assert_eq!(item(&[]).unit_cost(), None);
    }

    #[test]
    fn setting_a_cost_records_the_cost_before_and_after() {
        let mut i = item(&[(1, 4)]);
        i.take_movements();
        assert!(i.set_unit_cost(Some(eur("250")), note("invoice")));
        assert!(i.set_unit_cost(Some(eur("300.5")), None));
        let rows = i.take_movements();
        assert_eq!(
            rows.iter()
                .map(|m| (
                    m.location,
                    m.delta,
                    m.after,
                    m.reason,
                    m.unit_cost,
                    m.average_after
                ))
                .collect::<Vec<_>>(),
            [
                (
                    None,
                    0,
                    None,
                    StockReason::CostAdjustment,
                    None,
                    Some(eur("250"))
                ),
                (
                    None,
                    0,
                    None,
                    StockReason::CostAdjustment,
                    Some(eur("250")),
                    Some(eur("300.5"))
                ),
            ]
        );
        assert_eq!(
            rows[0].note.as_ref().map(MovementNote::as_str),
            Some("invoice")
        );
    }

    #[test]
    fn the_same_cost_again_records_nothing() {
        let mut i = item(&[]);
        i.set_unit_cost(Some(eur("1")), None);
        i.take_movements();
        i.take_events();
        assert!(!i.set_unit_cost(Some(eur("1")), None));
        assert_eq!(i.take_movements(), []);
        assert_eq!(i.take_events(), []);
    }

    #[test]
    fn units_moving_carry_the_cost_at_the_time() {
        let mut i = item(&[(1, 4)]);
        i.set_unit_cost(Some(eur("99")), None);
        i.take_movements();
        i.adjust_on_hand(loc_id(1), -1, AdjustmentReason::Damaged, None)
            .unwrap();
        let row = i.take_movements().remove(0);
        assert_eq!(
            (row.unit_cost, row.average_after),
            (Some(eur("99")), Some(eur("99")))
        );
    }

    #[test]
    fn a_cost_change_is_its_own_event() {
        let mut i = InventoryItem::restore(item(&[(1, 4)]).snapshot()).unwrap();
        i.set_unit_cost(Some(eur("12")), None);
        let types: Vec<&str> = i.take_events().iter().map(|e| e.event_type).collect();
        assert_eq!(types, ["inventory_item.cost_changed"]);
    }

    #[test]
    fn the_cost_survives_storage() {
        let mut i = item(&[]);
        i.set_unit_cost(Some(eur("0.0035")), None);
        let restored = InventoryItem::restore(i.snapshot()).unwrap();
        assert_eq!(restored.unit_cost(), Some(eur("0.0035")));
    }

    // ---- receiving ----

    fn receipt() -> StockSource {
        StockSource::Receipt {
            purchase_order: hypha_core::PurchaseOrderId::from_uuid(Uuid::from_u128(90)),
            receipt: hypha_core::ReceiptId::from_uuid(Uuid::from_u128(91)),
        }
    }

    /// An item with `stock` and `cost`, its history so far taken.
    fn costed(stock: &[(u128, i64)], cost: Option<&str>) -> InventoryItem {
        let mut i = item(stock);
        i.set_unit_cost(cost.map(eur), None);
        i.take_movements();
        i.take_events();
        i
    }

    fn cost_whys(i: &mut InventoryItem) -> Vec<String> {
        i.take_events()
            .iter()
            .filter(|e| e.event_type == "inventory_item.cost_changed")
            .map(|e| e.data["why"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    #[test]
    fn the_first_receipt_sets_the_cost() {
        let mut i = costed(&[], None);
        i.receive(loc_id(1), qty(10), Some(eur("500")), receipt())
            .unwrap();
        assert_eq!(
            (i.on_hand(loc_id(1)), i.unit_cost()),
            (10, Some(eur("500")))
        );
        let row = i.take_movements().remove(0);
        assert_eq!(
            (
                row.reason,
                row.delta,
                row.after,
                row.source,
                row.unit_cost,
                row.average_after
            ),
            (
                StockReason::Received,
                10,
                Some(10),
                receipt(),
                Some(eur("500")),
                Some(eur("500"))
            )
        );
        assert_eq!(cost_whys(&mut i), ["received"]);
    }

    #[test]
    fn receiving_averages_over_every_location() {
        // 4 at €5 (2 here, 2 elsewhere) and 6 arriving at €10: (4·500 + 6·1000) / 10 = 800
        let mut i = costed(&[(1, 2), (2, 2)], Some("500"));
        i.receive(loc_id(1), qty(6), Some(eur("1000")), receipt())
            .unwrap();
        assert_eq!(i.unit_cost(), Some(eur("800")));
        assert_eq!((i.on_hand(loc_id(1)), i.on_hand(loc_id(2))), (8, 2));
    }

    #[test]
    fn the_average_rounds_half_away_from_zero_at_four_places() {
        // (1·0.0001 + 2·0) / 3 = 0.0000333… → 0
        let mut i = costed(&[(1, 1)], Some("0.0001"));
        i.receive(loc_id(1), qty(2), Some(eur("0")), receipt())
            .unwrap();
        assert_eq!(i.unit_cost(), Some(eur("0")));
        // (1·0.0001 + 1·0) / 2 = 0.00005 → 0.0001
        let mut i = costed(&[(1, 1)], Some("0.0001"));
        i.receive(loc_id(1), qty(1), Some(eur("0")), receipt())
            .unwrap();
        assert_eq!(i.unit_cost(), Some(eur("0.0001")));
        // (2·100 + 1·101) / 3 = 100.3333…
        let mut i = costed(&[(1, 2)], Some("100"));
        i.receive(loc_id(1), qty(1), Some(eur("101")), receipt())
            .unwrap();
        assert_eq!(i.unit_cost(), Some(eur("100.3333")));
    }

    #[test]
    fn stock_below_zero_doesnt_weigh_in_the_average() {
        let mut i = costed(&[(1, -3)], Some("500"));
        i.receive(loc_id(1), qty(5), Some(eur("700")), receipt())
            .unwrap();
        assert_eq!((i.on_hand(loc_id(1)), i.unit_cost()), (2, Some(eur("700"))));
        // Nothing on hand at all: the cost of what arrives.
        let mut i = costed(&[(1, 0)], Some("500"));
        i.receive(loc_id(1), qty(1), Some(eur("700")), receipt())
            .unwrap();
        assert_eq!(i.unit_cost(), Some(eur("700")));
    }

    #[test]
    fn receiving_at_the_same_cost_records_no_cost_change() {
        let mut i = costed(&[(1, 2)], Some("500"));
        i.receive(loc_id(1), qty(3), Some(eur("500")), receipt())
            .unwrap();
        assert_eq!(cost_whys(&mut i), Vec::<String>::new());
        assert_eq!(i.take_movements().len(), 1);
    }

    #[test]
    fn receiving_without_a_cost_keeps_the_average() {
        let mut i = costed(&[(1, 2)], Some("500"));
        i.receive(loc_id(1), qty(3), None, receipt()).unwrap();
        let row = i.take_movements().remove(0);
        assert_eq!(
            (i.unit_cost(), row.unit_cost, row.average_after),
            (Some(eur("500")), Some(eur("500")), Some(eur("500")))
        );
    }

    #[test]
    fn untracked_items_count_what_arrives() {
        let mut i = InventoryItem::new(
            InventoryItemId::from_uuid(Uuid::from_u128(1)),
            VariantId::from_uuid(Uuid::from_u128(2)),
            false,
            false,
        );
        i.receive(loc_id(1), qty(4), Some(eur("1")), receipt())
            .unwrap();
        assert_eq!(i.on_hand(loc_id(1)), 4);
    }

    #[test]
    fn a_cost_in_another_currency_is_refused_and_changes_nothing() {
        let mut i = costed(&[(1, 2)], Some("500"));
        let usd = UnitCost::parse("500", hypha_core::Currency::USD).unwrap();
        let before = i.clone();
        assert_eq!(
            i.receive(loc_id(1), qty(1), Some(usd), receipt()),
            Err(InventoryError::CostCurrency {
                expected: hypha_core::Currency::EUR,
                got: hypha_core::Currency::USD
            })
        );
        assert_eq!(i, before);
        assert_eq!(i.take_movements(), []);
    }

    #[test]
    fn receiving_past_the_largest_count_is_refused() {
        let mut i = costed(&[(1, MAX_STOCK)], None);
        assert_eq!(
            i.receive(loc_id(1), qty(1), None, receipt()),
            Err(InventoryError::StockOutOfRange(i128::from(MAX_STOCK) + 1))
        );
    }

    #[test]
    fn taking_a_receipt_back_takes_its_value_out_of_the_average() {
        // 4 at €5 and 6 at €10 averaged 8; taking the 6 back leaves (10·800 − 6·1000)/4 = 500
        let mut i = costed(&[(1, 4)], Some("500"));
        i.receive(loc_id(1), qty(6), Some(eur("1000")), receipt())
            .unwrap();
        i.take_movements();
        i.take_events();
        i.unreceive(loc_id(1), qty(6), Some(eur("1000")), receipt())
            .unwrap();
        assert_eq!((i.on_hand(loc_id(1)), i.unit_cost()), (4, Some(eur("500"))));
        let row = i.take_movements().remove(0);
        assert_eq!(
            (
                row.reason,
                row.delta,
                row.after,
                row.unit_cost,
                row.average_after
            ),
            (
                StockReason::Received,
                -6,
                Some(4),
                Some(eur("1000")),
                Some(eur("500"))
            )
        );
        assert_eq!(cost_whys(&mut i), ["receipt_cancelled"]);
    }

    #[test]
    fn taking_back_what_already_left_keeps_the_average() {
        // All 6 sold since: nothing left to take the value out of.
        let mut i = costed(&[(1, 6)], Some("800"));
        i.unreceive(loc_id(1), qty(6), Some(eur("1000")), receipt())
            .unwrap();
        assert_eq!((i.on_hand(loc_id(1)), i.unit_cost()), (0, Some(eur("800"))));
        // Fewer on hand than taken back: the count goes below zero.
        let mut i = costed(&[(1, 2)], Some("800"));
        i.unreceive(loc_id(1), qty(6), Some(eur("1000")), receipt())
            .unwrap();
        assert_eq!(
            (i.on_hand(loc_id(1)), i.unit_cost()),
            (-4, Some(eur("800")))
        );
        assert_eq!(cost_whys(&mut i), Vec::<String>::new());
    }

    #[test]
    fn taking_back_more_value_than_is_left_keeps_the_average() {
        // 3 at €1 on hand; taking back 2 that cost €10 would leave a negative value.
        let mut i = costed(&[(1, 3)], Some("100"));
        i.unreceive(loc_id(1), qty(2), Some(eur("1000")), receipt())
            .unwrap();
        assert_eq!((i.on_hand(loc_id(1)), i.unit_cost()), (1, Some(eur("100"))));
    }

    #[test]
    fn taking_back_rounds_half_away_from_zero() {
        // (3·100 − 1·99) / 2 = 100.5
        let mut i = costed(&[(1, 3)], Some("100"));
        i.unreceive(loc_id(1), qty(1), Some(eur("99")), receipt())
            .unwrap();
        assert_eq!(i.unit_cost(), Some(eur("100.5")));
        // (3·1 − 1·0.0001) / 2 = 1.49995 → 1.5 (in 1/10,000: 29999 / 2 = 14999.5 → 15000)
        let mut i = costed(&[(1, 3)], Some("1"));
        i.unreceive(loc_id(1), qty(1), Some(eur("0.0001")), receipt())
            .unwrap();
        assert_eq!(i.unit_cost(), Some(eur("1.5")));
    }

    // ---- transfers ----

    fn transfer_source() -> StockSource {
        StockSource::Transfer(hypha_core::TransferId::from_uuid(Uuid::from_u128(90)))
    }

    #[test]
    fn a_transfer_takes_units_off_the_origin_and_puts_them_at_the_destination() {
        let mut i = item(&[(1, 5)]);
        i.take_movements();
        i.transfer_out(loc_id(1), qty(3), transfer_source())
            .unwrap();
        i.transfer_in(loc_id(2), qty(2), transfer_source()).unwrap();
        assert_eq!((i.on_hand(loc_id(1)), i.on_hand(loc_id(2))), (2, 2));
        let rows = i.take_movements();
        assert_eq!(
            rows.iter()
                .map(|m| (m.location, m.delta, m.after, m.reason, m.source))
                .collect::<Vec<_>>(),
            [
                (
                    Some(loc_id(1)),
                    -3,
                    Some(2),
                    StockReason::TransferOut,
                    transfer_source()
                ),
                (
                    Some(loc_id(2)),
                    2,
                    Some(2),
                    StockReason::TransferIn,
                    transfer_source()
                ),
            ]
        );
    }

    #[test]
    fn shipping_more_than_is_there_goes_below_zero() {
        let mut i = item(&[(1, 1)]);
        i.transfer_out(loc_id(1), qty(3), transfer_source())
            .unwrap();
        assert_eq!(i.on_hand(loc_id(1)), -2);
    }

    #[test]
    fn a_transfer_leaves_the_cost_as_it_is() {
        let mut i = item(&[(1, 5)]);
        let cost = UnitCost::new(10_000, hypha_core::Currency::EUR).unwrap();
        i.set_unit_cost(Some(cost), None);
        i.take_movements();
        i.take_events();
        i.transfer_out(loc_id(1), qty(2), transfer_source())
            .unwrap();
        i.transfer_in(loc_id(2), qty(2), transfer_source()).unwrap();
        assert_eq!(i.unit_cost(), Some(cost));
        assert!(
            i.take_movements()
                .iter()
                .all(|m| m.unit_cost == Some(cost) && m.average_after == Some(cost))
        );
        assert!(
            i.take_events()
                .iter()
                .all(|e| e.event_type != "inventory_item.cost_changed"),
            "no cost changed"
        );
    }

    #[test]
    fn units_lost_in_transit_are_a_row_without_a_delta_at_the_destination() {
        let mut i = item(&[(2, 4)]);
        i.take_movements();
        i.lose_in_transit(loc_id(2), qty(3), transfer_source());
        assert_eq!(i.on_hand(loc_id(2)), 4);
        let rows = i.take_movements();
        assert_eq!(
            rows.iter()
                .map(|m| (
                    m.location,
                    m.delta,
                    m.after,
                    m.reason,
                    m.note.as_ref().map(MovementNote::as_str)
                ))
                .collect::<Vec<_>>(),
            [(
                Some(loc_id(2)),
                0,
                Some(4),
                StockReason::Shrinkage,
                Some("3 lost in transit")
            )]
        );
    }

    #[test]
    fn an_untracked_item_counts_transfers_too() {
        let mut i = item(&[(1, 5)]);
        i.set_tracked(false);
        i.transfer_out(loc_id(1), qty(2), transfer_source())
            .unwrap();
        assert_eq!(i.on_hand(loc_id(1)), 3);
    }

    // ---- late costs ----

    fn late() -> StockSource {
        StockSource::PurchaseOrder(hypha_core::PurchaseOrderId::from_uuid(Uuid::from_u128(90)))
    }

    #[test]
    fn a_late_cost_is_spread_over_every_unit_on_hand() {
        // €10.00 (10,000,000 scaled) over 6 + 4 units: €1.00 more each.
        let mut i = costed(&[(1, 6), (2, 4)], Some("500"));
        assert_eq!(
            i.revalue(10_000_000, hypha_core::Currency::EUR, late()),
            Ok(true)
        );
        assert_eq!(i.unit_cost(), Some(eur("600")));
        let row = i.take_movements().remove(0);
        assert_eq!(
            (
                row.reason,
                row.delta,
                row.location,
                row.source,
                row.unit_cost,
                row.average_after
            ),
            (
                StockReason::CostAdjustment,
                0,
                None,
                late(),
                Some(eur("500")),
                Some(eur("600"))
            )
        );
        assert_eq!(cost_whys(&mut i), ["late_cost"]);
    }

    #[test]
    fn a_late_cost_can_lower_the_cost_but_never_below_zero() {
        let mut i = costed(&[(1, 10)], Some("500"));
        assert_eq!(
            i.revalue(-20_000_000, hypha_core::Currency::EUR, late()),
            Ok(true)
        );
        assert_eq!(i.unit_cost(), Some(eur("300")));
        assert_eq!(
            i.revalue(-1_000_000_000, hypha_core::Currency::EUR, late()),
            Ok(true)
        );
        assert_eq!(i.unit_cost(), Some(eur("0")));
    }

    #[test]
    fn a_late_cost_rounds_half_away_from_zero() {
        // 1 scaled unit over 2 is 0.5 → 1; −1 over 2 → −1.
        let mut i = costed(&[(1, 2)], Some("0.001"));
        i.revalue(1, hypha_core::Currency::EUR, late()).unwrap();
        assert_eq!(i.unit_cost(), Some(eur("0.0011")));
        i.revalue(-1, hypha_core::Currency::EUR, late()).unwrap();
        assert_eq!(i.unit_cost(), Some(eur("0.001")));
    }

    #[test]
    fn a_late_cost_with_nothing_on_hand_changes_nothing() {
        let mut i = costed(&[(1, 0), (2, -3)], Some("500"));
        assert_eq!(
            i.revalue(10_000_000, hypha_core::Currency::EUR, late()),
            Ok(false)
        );
        assert_eq!(i.unit_cost(), Some(eur("500")));
        assert!(i.take_movements().is_empty());
        assert!(cost_whys(&mut i).is_empty());
    }

    #[test]
    fn a_late_cost_too_small_to_show_changes_nothing() {
        // 1 scaled unit over 3 is 0.33… → 0.
        let mut i = costed(&[(1, 3)], Some("500"));
        assert_eq!(i.revalue(1, hypha_core::Currency::EUR, late()), Ok(false));
        assert!(i.take_movements().is_empty());
    }

    #[test]
    fn a_late_cost_gives_an_item_without_one_a_cost() {
        let mut i = costed(&[(1, 4)], None);
        assert_eq!(
            i.revalue(4_000_000, hypha_core::Currency::EUR, late()),
            Ok(true)
        );
        assert_eq!(i.unit_cost(), Some(eur("100")));
        // Taking value off an item without a cost leaves it without one.
        let mut i = costed(&[(1, 4)], None);
        assert_eq!(
            i.revalue(-4_000_000, hypha_core::Currency::EUR, late()),
            Ok(false)
        );
        assert_eq!(i.unit_cost(), None);
    }

    #[test]
    fn a_late_cost_in_another_currency_is_refused() {
        let mut i = costed(&[(1, 4)], Some("500"));
        assert_eq!(
            i.revalue(1_000, hypha_core::Currency::USD, late()),
            Err(InventoryError::CostCurrency {
                expected: hypha_core::Currency::EUR,
                got: hypha_core::Currency::USD
            })
        );
        assert_eq!(i.unit_cost(), Some(eur("500")));
    }
}
