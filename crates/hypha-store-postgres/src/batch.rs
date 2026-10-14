use hypha_cart::{Cart, CartSnapshot, StageCarts};
use hypha_core::{HasBatch, RepoError, Versioned};
use hypha_inventory::{InventoryItem, InventoryItemSnapshot, StageInventoryItems};

use crate::{
    carts::update_cart,
    error::repo_error,
    inventory::update_items,
    store::{PgConnection, PgStore},
};

/// Writes staged for `PgStore::commit`: snapshots taken when each aggregate was staged,
/// carrying the version it was loaded at.
#[derive(Default)]
pub struct PgBatch {
    carts: Vec<CartSnapshot>,
    inventory_items: Vec<InventoryItemSnapshot>,
}

impl StageCarts for PgBatch {
    fn update_cart(&mut self, cart: &mut Cart) {
        self.carts.push(cart.snapshot());
        cart.set_version(cart.version().next());
    }
}

impl StageInventoryItems for PgBatch {
    fn update_inventory_item(&mut self, item: &mut InventoryItem) {
        self.inventory_items.push(item.snapshot());
        item.set_version(item.version().next());
    }
}

impl<C: PgConnection> HasBatch for PgStore<C> {
    type Batch = PgBatch;

    fn batch(&self) -> PgBatch {
        PgBatch::default()
    }

    /// One transaction. Tables are written in a fixed order, and rows within a table by
    /// ID, so two commits lock rows in the same order and queue instead of deadlocking.
    /// Any failure returns early and drops the transaction, which rolls it back.
    async fn commit(&self, mut batch: PgBatch) -> Result<(), RepoError> {
        if batch.carts.is_empty() && batch.inventory_items.is_empty() {
            return Ok(());
        }
        batch.carts.sort_by_key(|s| s.id.as_uuid());

        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        for cart in &batch.carts {
            update_cart(&tx, cart).await?;
        }
        update_items(&tx, batch.inventory_items).await?;
        tx.commit().await.map_err(repo_error)
    }
}
