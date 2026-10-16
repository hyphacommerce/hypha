//! Fulfilments (design note 0018): what left a location for an order, with its
//! tracking.
//!
//! A leaf domain: it depends on core only. A fulfilment names its order, its lines and
//! its location by ID and knows nothing about what was ordered or what's in stock; those
//! rules span aggregates and live in `hypha-order-actions`, which writes a fulfilment,
//! its order and the inventory items in one batch.

pub mod commands;
pub mod error;
pub mod events;
pub mod fulfillment;
pub mod repo;

pub use commands::{
    CommandError, TrackingInput, get_fulfillment, list_order_fulfillments, update_tracking,
};
pub use error::FulfillmentError;
pub use events::{FulfillmentCancelled, FulfillmentCreated, FulfillmentUpdated};
pub use fulfillment::{
    Fulfillment, FulfillmentLine, FulfillmentSnapshot, FulfillmentState, NewFulfillment, Tracking,
};
pub use repo::{FulfillmentRepo, HasFulfillments, StageFulfillments, keys};
