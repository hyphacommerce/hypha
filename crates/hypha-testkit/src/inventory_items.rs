use hypha_core::{InventoryItemId, RepoError, VariantId};
use hypha_inventory::{InventoryItem, InventoryItemRepo, repo::keys};

use crate::table::Table;

#[derive(Default)]
pub struct InMemoryInventoryItemRepo {
    items: Table<InventoryItemId, InventoryItem>,
}

/// One item per variant.
fn unique(new: &InventoryItem, other: &InventoryItem) -> Option<&'static str> {
    (new.variant() == other.variant()).then_some(keys::INVENTORY_ITEM_VARIANT)
}

impl InMemoryInventoryItemRepo {
    /// The next `n` lookups by a unique key (and `list_all`, where there is one) find
    /// nothing, as if another request inserted the row right after: for testing a
    /// command that loses the race between its pre-check and its write.
    pub fn hide_next_lookups(&self, n: usize) {
        self.items.hide_next_lookups(n);
    }
}

impl InventoryItemRepo for InMemoryInventoryItemRepo {
    async fn get(&self, id: InventoryItemId) -> Result<Option<InventoryItem>, RepoError> {
        Ok(self.items.get(&id))
    }

    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<InventoryItem>, RepoError> {
        Ok(self.items.find(|i| i.variant() == variant))
    }

    async fn for_variants(&self, variants: &[VariantId]) -> Result<Vec<InventoryItem>, RepoError> {
        Ok(self
            .items
            .all()
            .into_iter()
            .filter(|i| variants.contains(&i.variant()))
            .collect())
    }

    async fn insert(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        self.items
            .insert(item.id(), item, keys::INVENTORY_ITEM_ID, &unique)
    }

    async fn update(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        self.items.update(item.id(), item, &unique)
    }

    async fn update_many(&self, items: &mut [InventoryItem]) -> Result<(), RepoError> {
        self.items.update_many(items, InventoryItem::id, &unique)
    }
}
