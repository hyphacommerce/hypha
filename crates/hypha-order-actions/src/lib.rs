//! What staff and apps do to orders after checkout (design note 0018): fulfil them,
//! take a fulfilment back, cancel them, record money received (design note 0027) and
//! what a payment's provider reports (design note 0029, §3.1).
//! Later, refunds and order edits.
//!
//! A workflow: it composes orders, fulfilments and inventory, and writes each action's
//! aggregates in one batch, retried when another request committed first (like
//! `hypha-checkout`).

pub mod commands;
pub mod payment_events;
pub mod view;

pub use commands::{
    CancelOrder, CommandError, FULFILL, Fulfil, FulfilLine, Fulfilled, MarkPaid, OrderActionPorts,
    OrderPayments, cancel_fulfillment, cancel_order, fulfill, mark_paid,
};
pub use payment_events::{PAYMENT_EVENT, PaymentEvent, PaymentEventKind, record_payment_event};
pub use view::{OrderView, Unfulfilled, order_view, view_of};
