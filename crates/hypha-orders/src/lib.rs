//! Orders (design note 0007).
//!
//! A leaf domain: it depends on core only. An order is a snapshot of a priced cart,
//! so it knows nothing about carts, products or tax rates; `hypha-checkout` builds it.

pub mod commands;
pub mod error;
pub mod order;
pub mod repo;
pub mod summary;

pub use commands::{CommandError, get_order, list_orders, mark_order_paid};
pub use error::OrderError;
pub use order::{
    FinancialStatus, FulfillmentStatus, NewOrder, Order, OrderLine, OrderNumber, OrderSnapshot,
    OrderTax, OrderTaxSource,
};
pub use repo::{HasOrders, OrderRepo, StageOrders};
pub use summary::OrderSummary;
