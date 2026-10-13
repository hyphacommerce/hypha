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
    /// stock for all its lines at once (design note 0004, I7). Each item at most once.
    async fn update_many(&self, items: &mut [InventoryItem]) -> Result<(), RepoError>;
}
