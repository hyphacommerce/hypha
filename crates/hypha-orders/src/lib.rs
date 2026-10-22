//! Orders.
//!
//! A leaf domain: it depends on core only. An order is a snapshot of a priced cart,
//! so it knows nothing about carts, products or tax rates; `hypha-checkout` builds it.

pub mod commands;
pub mod error;
pub mod events;
pub mod filter;
pub mod order;
pub mod repo;
pub mod stats;
pub mod summary;

pub use commands::{
    CommandError, ListOrders, customer_order_stats, get_order, get_placed_order, list_orders,
};
pub use error::OrderError;
pub use events::{OrderCancelled, OrderFulfillmentStatusChanged, OrderPaid, OrderPlaced};
pub use filter::{OrderFilter, OrderFilterInput, OrderSort};
pub use order::{
    CancelReason, Cancellation, FinancialStatus, FulfillmentStatus, NewOrder, Order, OrderLine,
    OrderNumber, OrderShippingLine, OrderSnapshot, OrderTax, OrderTaxSource,
};
pub use repo::{HasOrders, OrderRepo, StageOrders};
pub use stats::OrderStats;
pub use summary::OrderSummary;
