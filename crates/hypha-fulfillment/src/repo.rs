use hypha_core::{FulfillmentId, OrderId, RepoError};

use crate::Fulfillment;

/// Uniqueness rules a `FulfillmentRepo` enforces, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const FULFILLMENT_ID: &str = "fulfillment_id";
}

#[allow(async_fn_in_trait)]
pub trait FulfillmentRepo {
    async fn get(&self, id: FulfillmentId) -> Result<Option<Fulfillment>, RepoError>;
    /// Every fulfilment of `order`, cancelled ones too, newest first (by ID). An order
    /// has a handful.
    async fn list_for_order(&self, order: OrderId) -> Result<Vec<Fulfillment>, RepoError>;
    /// Stores a new fulfilment (version 1). `hypha-order-actions` inserts through
    /// `StageFulfillments` instead, with the order and the stock.
    async fn insert(&self, fulfillment: &mut Fulfillment) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, fulfillment: &mut Fulfillment) -> Result<(), RepoError>;
}

/// Stages fulfilment writes in a `hypha_core::HasBatch` batch; the adapter's batch type
/// implements it.
pub trait StageFulfillments {
    /// Stages what `FulfillmentRepo::insert` would write; `fulfillment` is at version 1
    /// once the batch commits.
    fn insert_fulfillment(&mut self, fulfillment: &mut Fulfillment);
    /// Stages what `FulfillmentRepo::update` would write.
    fn update_fulfillment(&mut self, fulfillment: &mut Fulfillment);
}

/// Whatever holds the `FulfillmentRepo`: a host's per-request ports, a test world.
pub trait HasFulfillments {
    type Fulfillments: FulfillmentRepo;
    fn fulfillments(&self) -> &Self::Fulfillments;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasFulfillments + ?Sized> HasFulfillments for &T {
    type Fulfillments = T::Fulfillments;
    fn fulfillments(&self) -> &Self::Fulfillments {
        (**self).fulfillments()
    }
}
