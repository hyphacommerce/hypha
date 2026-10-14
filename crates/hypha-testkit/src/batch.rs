use hypha_cart::{Cart, StageCarts};
use hypha_core::{HasBatch, RepoError, Versioned};
use hypha_inventory::{InventoryItem, StageInventoryItems};

use crate::{
    InMemoryPorts, inventory_items,
    table::{no_unique_keys, update_row},
};

/// Writes staged for `InMemoryPorts::commit`: copies of the aggregates as they were
/// staged, still carrying the version they were loaded at.
#[derive(Default)]
pub struct InMemoryBatch {
    carts: Vec<Cart>,
    inventory_items: Vec<InventoryItem>,
}

impl StageCarts for InMemoryBatch {
    fn update_cart(&mut self, cart: &mut Cart) {
        self.carts.push(cart.clone());
        cart.set_version(cart.version().next());
    }
}

impl StageInventoryItems for InMemoryBatch {
    fn update_inventory_item(&mut self, item: &mut InventoryItem) {
        self.inventory_items.push(item.clone());
        item.set_version(item.version().next());
    }
}

impl HasBatch for InMemoryPorts {
    type Batch = InMemoryBatch;

    fn batch(&self) -> InMemoryBatch {
        InMemoryBatch::default()
    }

    /// Locks every table a batch can write, always in this order, so two commits can't
    /// deadlock and a single write (which locks one table) can't slip in between. The
    /// writes then run with the usual version and key checks; if one fails, the tables
    /// are put back as they were. Readers wait for the locks, so they never see half a
    /// batch. Copying the tables for the rollback is fine at test sizes.
    async fn commit(&self, batch: InMemoryBatch) -> Result<(), RepoError> {
        let mut carts = self.carts.carts.lock();
        let mut items = self.inventory_items.items.lock();
        let before = (carts.clone(), items.clone());

        let written = (|| {
            for mut cart in batch.carts {
                update_row(&mut carts, cart.id(), &mut cart, &no_unique_keys)?;
            }
            for mut item in batch.inventory_items {
                update_row(&mut items, item.id(), &mut item, &inventory_items::unique)?;
            }
            Ok(())
        })();
        if written.is_err() {
            (*carts, *items) = before;
        }
        written
    }
}
