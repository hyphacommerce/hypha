//! Orders.
//!
//! A leaf domain: it depends on core only. An order is a snapshot of a priced cart,
//! so it knows nothing about carts, products or tax rates; `hypha-checkout` builds it.

pub mod address_check;
pub mod commands;
pub mod error;
pub mod events;
pub mod filter;
pub mod order;
pub mod repo;
pub mod sales;
pub mod stats;
pub mod summary;

pub use address_check::{MarkedValid, OrderAddressCheck};
pub use commands::{
    CommandError, ListOrders, change_order_shipping_address, customer_order_stats, get_order,
    get_placed_order, list_orders, mark_order_address_valid,
};
pub use error::OrderError;
pub use events::{
    OrderAddressMarkedValid, OrderCancelled, OrderFulfillmentStatusChanged, OrderPaid, OrderPlaced,
    OrderShippingAddressChanged, ShippingAddressPayload,
};
pub use filter::{OrderFilter, OrderFilterInput, OrderSort};
pub use order::{
    CancelReason, Cancellation, DiscountClass, DiscountShare, FinancialStatus, FulfillmentStatus,
    LocalizedDiscount, LocalizedLine, LocalizedShipping, NewOrder, Order, OrderDiscount, OrderLine,
    OrderNumber, OrderShippingLine, OrderSnapshot, OrderTax, OrderTaxSource,
};
pub use repo::{HasOrders, OrderRepo, StageOrders};
pub use sales::{
    BucketFigures, CurrencySales, CurrencyTop, ProductRevenue, ProductUnits, SalesFigures,
    SalesQuery,
};
pub use stats::OrderStats;
pub use summary::OrderSummary;
