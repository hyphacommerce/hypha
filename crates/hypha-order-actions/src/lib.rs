//! What staff and apps do to orders after checkout (design note 0018): fulfil them,
//! take a fulfilment back, cancel them, record money received (design note 0027).
//! Later, refunds and order edits.
//!
//! A workflow: it composes orders, fulfilments and inventory, and writes each action's
//! aggregates in one batch, retried when another request committed first (like
//! `hypha-checkout`).

pub mod commands;
pub mod view;

pub use commands::{
    CancelOrder, CommandError, FULFILL, Fulfil, FulfilLine, Fulfilled, MarkPaid, OrderActionPorts,
    OrderPayments, cancel_fulfillment, cancel_order, fulfill, mark_paid,
};
pub use view::{OrderView, Unfulfilled, order_view, view_of};
