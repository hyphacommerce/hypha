//! What staff and apps do to orders after checkout (design note 0018): fulfil them,
//! take a fulfilment back, cancel them. Later, refunds and order edits.
//!
//! A workflow: it composes orders, fulfilments and inventory, and writes each action's
//! aggregates in one batch, retried when another request committed first (like
//! `hypha-checkout`).

pub mod commands;
pub mod email;
pub mod view;

pub use commands::{
    CancelOrder, CommandError, FULFILL, Fulfil, FulfilLine, Fulfilled, OrderActionPorts,
    cancel_fulfillment, cancel_order, fulfill,
};
pub use email::BuyerEmail;
pub use view::{OrderView, Unfulfilled, order_view};
