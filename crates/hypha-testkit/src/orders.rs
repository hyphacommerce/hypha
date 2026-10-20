use std::sync::{
    Mutex,
    atomic::{AtomicI64, Ordering},
};

use hypha_core::{
    Keyset, OrderId, Page, PageRequest, Recorded, RepoError, Versioned, stamp_insert, stamp_update,
};
use hypha_orders::{
    Order, OrderFilter, OrderNumber, OrderRepo, OrderSort, OrderSummary, repo::keys,
};

use crate::{
    InMemoryOutbox,
    custom_fields::{SharedValues, fields_match, values_for},
    table::Table,
};

pub struct InMemoryOrderRepo {
    pub(crate) orders: Table<OrderId, Order>,
    /// The last number handed out; the first `next_number` is 1001, like Postgres.
    last_number: AtomicI64,
    pub(crate) outbox: InMemoryOutbox,
    /// What the next `get` returns instead of the stored order (`serve_next_get`).
    stale: Mutex<Option<Order>>,
    field_values: SharedValues,
}

impl Default for InMemoryOrderRepo {
    fn default() -> Self {
        Self {
            orders: Table::default(),
            last_number: AtomicI64::new(1000),
            outbox: InMemoryOutbox::default(),
            stale: Mutex::new(None),
            field_values: SharedValues::default(),
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

    /// The same repo, filtering its list by the custom field values `fields` holds
    /// (design note 0033, §7), as a database joins them.
    pub fn joining_fields(self, fields: &crate::InMemoryCustomFields) -> Self {
        Self {
            field_values: fields.shared_values(),
            ..self
        }
    }

    /// The events this repo wrote (into its outbox, shared or not).
    pub fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }

    /// The next `get` returns `order`, a copy loaded before another request committed:
    /// for testing a command that loses that race (its write is then stale).
    pub fn serve_next_get(&self, order: Order) {
        *self.stale.lock().unwrap() = Some(order);
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
        let stale = self.stale.lock().unwrap().take();
        if let Some(order) = stale.filter(|o| o.id() == id) {
            return Ok(Some(order));
        }
        Ok(self.orders.get(&id))
    }

    async fn get_by_number(&self, number: OrderNumber) -> Result<Option<Order>, RepoError> {
        Ok(self.orders.find(|o| o.number() == number))
    }

    async fn list(
        &self,
        filter: &OrderFilter,
        sort: OrderSort,
        page: PageRequest<Keyset<OrderId>>,
    ) -> Result<Page<OrderSummary>, RepoError> {
        if let Some(after) = &page.after
            && !sort.fits(&after.key)
        {
            return Err(RepoError::Backend(format!(
                "a {:?} position for a {sort:?} list",
                after.key
            )));
        }
        let values = values_for(&self.field_values, filter.fields());
        Ok(self.orders.page_sorted(
            |o| {
                filter.matches(o)
                    && fields_match(values.as_ref(), o.id().as_uuid(), filter.fields())
            },
            &page,
            sort.direction(),
            Order::summary,
            |s: &OrderSummary| sort.key(s),
        ))
    }

    async fn insert(&self, order: &mut Order) -> Result<(), RepoError> {
        stamp_insert(order, self.outbox.now());
        let events = order.take_events();
        self.orders
            .insert(order.id(), order, keys::ORDER_ID, &unique)?;
        self.outbox.write(events, Some(order.version()));
        Ok(())
    }

    async fn update(&self, order: &mut Order) -> Result<(), RepoError> {
        stamp_update(order, self.outbox.now());
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
