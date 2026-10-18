//! Checkout (design note 0007): turns a cart into an order.
//!
//! The top domain layer: it composes the cart (composing) with orders, inventory and
//! customers and payments (leaves), and writes everything a checkout changes in one
//! `HasBatch` commit: the order, its payment, the completed cart, the stock
//! reservations, a new customer for a guest, and the idempotency key.

pub mod commands;
mod payment;
mod start;
mod view;

pub use commands::{CHECKOUT, Checkout, CheckoutPorts, CommandError, Placed, checkout};
pub use payment::{cart_payment_methods, set_payment_method};
pub use start::{StartPayment, start_order_payment};
pub use view::view_cart;
