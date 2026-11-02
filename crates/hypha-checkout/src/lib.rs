//! Checkout: turns a cart into an order, once an online payment's money is committed
//! (`start_cart_payment`, then `checkout`).
//!
//! The top domain layer: it composes the cart (composing) with orders, inventory and
//! customers and payments (leaves), and writes everything a checkout changes in one
//! `HasBatch` commit: the order, its payment, the completed cart, the stock
//! reservations, a new customer for a guest, and the idempotency key.

mod cart_payment;
pub mod commands;
mod messages;
mod payment;
mod start;
mod validation;
mod view;

pub use cart_payment::{CartPaymentStarted, StartCartPayment, start_cart_payment};
pub use commands::{
    CHECKOUT, Checkout, CheckoutPorts, CommandError, Placed, checkout, complete_paid_checkout,
};
pub use messages::{CartChecks, Checks, cart_messages};
pub use payment::{cart_payment_methods, set_payment_method};
pub use validation::{CheckoutValidators, NoValidators, Validator, validate_cart};
pub use view::view_cart;
