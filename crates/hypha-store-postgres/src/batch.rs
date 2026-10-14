use hypha_cart::{Cart, CartSnapshot, StageCarts};
use hypha_core::{HasBatch, IdempotencyRecord, RepoError, StageIdempotency, Version, Versioned};
use hypha_customers::{Customer, CustomerSnapshot, StageCustomers};
use hypha_inventory::{InventoryItem, InventoryItemSnapshot, StageInventoryItems};
use hypha_orders::{Order, OrderSnapshot, StageOrders};

use crate::{
    carts::{insert_cart, update_cart},
    customers::insert_customer,
    error::repo_error,
    idempotency::insert_records,
    inventory::update_items,
    orders::insert_order,
    store::{PgConnection, PgStore},
};

/// Writes staged for `PgStore::commit`: snapshots taken when each aggregate was staged,
/// carrying the version it was loaded at.
#[derive(Default)]
pub struct PgBatch {
    new_orders: Vec<OrderSnapshot>,
    new_carts: Vec<CartSnapshot>,
    new_customers: Vec<CustomerSnapshot>,
    carts: Vec<CartSnapshot>,
    inventory_items: Vec<InventoryItemSnapshot>,
    idempotency: Vec<IdempotencyRecord>,
}

impl PgBatch {
    fn is_empty(&self) -> bool {
        self.new_orders.is_empty()
            && self.new_carts.is_empty()
            && self.new_customers.is_empty()
            && self.carts.is_empty()
            && self.inventory_items.is_empty()
            && self.idempotency.is_empty()
    }
}

impl StageCarts for PgBatch {
    fn insert_cart(&mut self, cart: &mut Cart) {
        self.new_carts.push(cart.snapshot());
        cart.set_version(Version::NEW.next());
    }

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

impl StageOrders for PgBatch {
    fn insert_order(&mut self, order: &mut Order) {
        self.new_orders.push(order.snapshot());
        order.set_version(Version::NEW.next());
    }
}

impl StageCustomers for PgBatch {
    fn insert_customer(&mut self, customer: &mut Customer) {
        self.new_customers.push(customer.snapshot());
        customer.set_version(Version::NEW.next());
    }
}

impl StageIdempotency for PgBatch {
    fn record_idempotency(&mut self, record: IdempotencyRecord) {
        self.idempotency.push(record);
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
        if batch.is_empty() {
            return Ok(());
        }
        batch.carts.sort_by_key(|s| s.id.as_uuid());

        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        for order in &batch.new_orders {
            insert_order(&tx, order).await?;
        }
        for cart in &batch.new_carts {
            insert_cart(&tx, cart).await?;
        }
        for cart in &batch.carts {
            update_cart(&tx, cart).await?;
        }
        update_items(&tx, batch.inventory_items).await?;
        for customer in &batch.new_customers {
            insert_customer(&tx, customer).await?;
        }
        insert_records(&tx, &batch.idempotency).await?;
        tx.commit().await.map_err(repo_error)
    }
}
