use std::sync::atomic::{AtomicI64, Ordering};

use hypha_core::{OrderId, Page, PageRequest, Recorded, RepoError, Versioned};
use hypha_orders::{Order, OrderNumber, OrderRepo, OrderSummary, repo::keys};

use crate::{InMemoryOutbox, table::Table};

pub struct InMemoryOrderRepo {
    pub(crate) orders: Table<OrderId, Order>,
    /// The last number handed out; the first `next_number` is 1001, like Postgres.
    last_number: AtomicI64,
    pub(crate) outbox: InMemoryOutbox,
}

impl Default for InMemoryOrderRepo {
    fn default() -> Self {
        Self {
            orders: Table::default(),
            last_number: AtomicI64::new(1000),
            outbox: InMemoryOutbox::default(),
        }
    }
}

impl InMemoryOrderRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The events this repo wrote (into its outbox, shared or not).
    pub fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }
}

/// One order per number and per cart; line IDs unique across orders.
pub(crate) fn unique(new: &Order, other: &Order) -> Option<&'static str> {
    if new.number() == other.number() {
        return Some(keys::ORDER_NUMBER);
    }
    if new.cart() == other.cart() {
        return Some(keys::ORDER_CART);
    }
    let shares_line = new
        .lines()
        .iter()
        .any(|l| other.lines().iter().any(|o| o.id == l.id));
    shares_line.then_some(keys::ORDER_LINE_ID)
}

impl OrderRepo for InMemoryOrderRepo {
    async fn get(&self, id: OrderId) -> Result<Option<Order>, RepoError> {
        Ok(self.orders.get(&id))
    }

    async fn get_by_number(&self, number: OrderNumber) -> Result<Option<Order>, RepoError> {
        Ok(self.orders.find(|o| o.number() == number))
    }

    async fn list(&self, page: PageRequest<OrderId>) -> Result<Page<OrderSummary>, RepoError> {
        Ok(self.orders.page(|_| true, page, Order::summary))
    }

    async fn insert(&self, order: &mut Order) -> Result<(), RepoError> {
        let events = order.take_events();
        self.orders
            .insert(order.id(), order, keys::ORDER_ID, &unique)?;
        self.outbox.write(events, Some(order.version()));
        Ok(())
    }

    async fn update(&self, order: &mut Order) -> Result<(), RepoError> {
        let events = order.take_events();
        self.orders.update(order.id(), order, &unique)?;
        self.outbox.write(events, Some(order.version()));
        Ok(())
    }

    async fn next_number(&self) -> Result<OrderNumber, RepoError> {
        let n = self.last_number.fetch_add(1, Ordering::SeqCst) + 1;
        OrderNumber::new(n).map_err(|e| RepoError::Backend(e.to_string()))
    }
}
