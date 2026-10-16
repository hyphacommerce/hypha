use hypha_core::{InventoryItemId, LocationId, MarketId, RepoError, VariantId};

use crate::{InventoryItem, Location};

pub mod keys {
    pub const LOCATION_ID: &str = "location_id";
    pub const INVENTORY_ITEM_ID: &str = "inventory_item_id";
    /// One inventory item per variant.
    pub const INVENTORY_ITEM_VARIANT: &str = "inventory_item_variant";
}

#[allow(async_fn_in_trait)]
pub trait LocationRepo {
    async fn get(&self, id: LocationId) -> Result<Option<Location>, RepoError>;
    /// The locations whose stock counts for `market`, by priority (then ID).
    async fn serving(&self, market: MarketId) -> Result<Vec<Location>, RepoError>;
    async fn insert(&self, location: &mut Location) -> Result<(), RepoError>;
    async fn update(&self, location: &mut Location) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait InventoryItemRepo {
    async fn get(&self, id: InventoryItemId) -> Result<Option<InventoryItem>, RepoError>;
    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<InventoryItem>, RepoError>;
    /// The items of `variants` that have one, in any order.
    async fn for_variants(&self, variants: &[VariantId]) -> Result<Vec<InventoryItem>, RepoError>;
    async fn insert(&self, item: &mut InventoryItem) -> Result<(), RepoError>;
    async fn update(&self, item: &mut InventoryItem) -> Result<(), RepoError>;
    /// Updates several items **all or nothing**: if any of them is stale (or gone),
    /// nothing is written and the result is `StaleVersion`. How one checkout reserves
    /// stock for all its lines at once (design note 0004, I7).
    ///
    /// Precondition: each item at most once. Two copies of one item fail (`Backend` or
    /// `StaleVersion`, depending on the adapter) and write nothing.
    async fn update_many(&self, items: &mut [InventoryItem]) -> Result<(), RepoError>;
    /// Removes the item with its levels and reservations, writing the events it
    /// recorded (`InventoryItem::delete`); one that doesn't exist any more is fine.
    async fn delete(&self, item: &mut InventoryItem) -> Result<(), RepoError>;
}

/// Stages inventory item writes in a `hypha_core::HasBatch` batch; the adapter's batch
/// type implements it.
pub trait StageInventoryItems {
    /// Stages what `InventoryItemRepo::update` would write, and moves `item` to the
    /// version it has once the batch commits. If the commit fails, `item` is stale: load
    /// it again.
    fn update_inventory_item(&mut self, item: &mut InventoryItem);
    /// Stages removing the item (after `InventoryItem::delete`) with its levels and
    /// reservations. Unlike `InventoryItemRepo::delete`, it checks the version: a change
    /// since the item was loaded (a new reservation) fails the batch with
    /// `StaleVersion`, and so does an item already gone.
    fn delete_inventory_item(&mut self, item: &mut InventoryItem);
}

/// Whatever holds the `LocationRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use (review plan D1); `hypha_cart::CartPorts` is
/// the union of all of them.
pub trait HasLocations {
    type Locations: LocationRepo;
    fn locations(&self) -> &Self::Locations;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasLocations + ?Sized> HasLocations for &T {
    type Locations = T::Locations;
    fn locations(&self) -> &Self::Locations {
        (**self).locations()
    }
}

/// Whatever holds the `InventoryItemRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use (review plan D1); `hypha_cart::CartPorts` is
/// the union of all of them.
pub trait HasInventoryItems {
    type InventoryItems: InventoryItemRepo;
    fn inventory_items(&self) -> &Self::InventoryItems;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasInventoryItems + ?Sized> HasInventoryItems for &T {
    type InventoryItems = T::InventoryItems;
    fn inventory_items(&self) -> &Self::InventoryItems {
        (**self).inventory_items()
    }
}
