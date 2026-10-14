use hypha_cart::{Cart, StageCarts, repo::keys as cart_keys};
use hypha_core::{HasBatch, IdempotencyRecord, RepoError, StageIdempotency, Version, Versioned};
use hypha_inventory::{InventoryItem, StageInventoryItems};

use crate::{
    InMemoryPorts,
    idempotency::insert_record,
    inventory_items,
    table::{insert_row, no_unique_keys, update_row},
};

/// Writes staged for `InMemoryPorts::commit`: copies of the aggregates as they were
/// staged, still carrying the version they were loaded at.
#[derive(Default)]
pub struct InMemoryBatch {
    new_carts: Vec<Cart>,
    carts: Vec<Cart>,
    inventory_items: Vec<InventoryItem>,
    idempotency: Vec<IdempotencyRecord>,
}

impl StageCarts for InMemoryBatch {
    fn insert_cart(&mut self, cart: &mut Cart) {
        self.new_carts.push(cart.clone());
        cart.set_version(Version::NEW.next());
    }

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

impl StageIdempotency for InMemoryBatch {
    fn record_idempotency(&mut self, record: IdempotencyRecord) {
        self.idempotency.push(record);
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
        let mut records = self.idempotency.lock();
        let before = (carts.clone(), items.clone(), records.clone());

        let written = (|| {
            for mut cart in batch.new_carts {
                insert_row(
                    &mut carts,
                    cart.id(),
                    &mut cart,
                    cart_keys::CART_ID,
                    &no_unique_keys,
                )?;
            }
            for mut cart in batch.carts {
                update_row(&mut carts, cart.id(), &mut cart, &no_unique_keys)?;
            }
            for mut item in batch.inventory_items {
                update_row(&mut items, item.id(), &mut item, &inventory_items::unique)?;
            }
            for record in batch.idempotency {
                insert_record(&mut records, record)?;
            }
            Ok(())
        })();
        if written.is_err() {
            (*carts, *items, *records) = before;
        }
        written
    }
}
