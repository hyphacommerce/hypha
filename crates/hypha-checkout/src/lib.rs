//! Checkout (design note 0007): turns a cart into an order.
//!
//! The top domain layer: it composes the cart (composing) with orders, inventory and
//! customers (leaves), and writes everything a checkout changes in one `HasBatch`
//! commit: the order, the completed cart, the stock reservations, a new customer for a
//! guest, and the idempotency key.

pub mod commands;
mod view;

pub use commands::{CHECKOUT, Checkout, CheckoutPorts, CommandError, checkout};
pub use view::view_cart;
