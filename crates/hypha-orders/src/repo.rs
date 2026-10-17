use hypha_core::{Keyset, OrderId, Page, PageRequest, RepoError};

use crate::{Order, OrderFilter, OrderNumber, OrderSort, OrderSummary};

/// Uniqueness rules an `OrderRepo` enforces, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const ORDER_ID: &str = "order_id";
    pub const ORDER_NUMBER: &str = "order_number";
    /// One order per cart: checking a cart out twice can't make two orders.
    pub const ORDER_CART: &str = "order_cart";
    /// Order line IDs are unique across all orders (they reference stock reservations).
    pub const ORDER_LINE_ID: &str = "order_line_id";
}

#[allow(async_fn_in_trait)]
pub trait OrderRepo {
    async fn get(&self, id: OrderId) -> Result<Option<Order>, RepoError>;
    async fn get_by_number(&self, number: OrderNumber) -> Result<Option<Order>, RepoError>;
    /// A page of order summaries in `sort`'s order (design notes 0012 and 0024): the
    /// orders after `page.after` by `OrderSort::key` and then ID, only those `filter`
    /// matches (`OrderFilter::matches`). Each equals `Order::summary` of the stored order;
    /// an adapter may compute them without loading lines. The caller passes a sort the
    /// filter allows (`OrderSort::check`) and a position whose key fits it
    /// (`OrderSort::fits`); one that doesn't is a `Backend` error.
    async fn list(
        &self,
        filter: &OrderFilter,
        sort: OrderSort,
        page: PageRequest<Keyset<OrderId>>,
    ) -> Result<Page<OrderSummary>, RepoError>;
    /// Stores a new order (version 1). Enforces every key in `keys`. Checkout inserts
    /// through `StageOrders` instead, together with its other writes.
    async fn insert(&self, order: &mut Order) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, order: &mut Order) -> Result<(), RepoError>;
    /// The next order number: unique and increasing from 1001 on a new store, taken
    /// outside any batch, so a checkout that fails afterwards leaves a gap.
    async fn next_number(&self) -> Result<OrderNumber, RepoError>;
}

/// Stages order writes in a `hypha_core::HasBatch` batch; the adapter's batch type
/// implements it.
pub trait StageOrders {
    /// Stages what `OrderRepo::insert` would write; `order` is at version 1 once the
    /// batch commits.
    fn insert_order(&mut self, order: &mut Order);
    /// Stages what `OrderRepo::update` would write, and moves `order` to the version it
    /// has once the batch commits (stale then: `StaleVersion`, nothing written).
    fn update_order(&mut self, order: &mut Order);
}

/// Whatever holds the `OrderRepo`: a host's per-request ports, a test world.
pub trait HasOrders {
    type Orders: OrderRepo;
    fn orders(&self) -> &Self::Orders;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasOrders + ?Sized> HasOrders for &T {
    type Orders = T::Orders;
    fn orders(&self) -> &Self::Orders {
        (**self).orders()
    }
}
