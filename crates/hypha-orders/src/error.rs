use hypha_core::{Currency, ErrorCode, MoneyError, OrderId, OrderLineId, TextError};
use thiserror::Error;

/// Why an order can't be built or changed. Checkout builds orders from a priced cart, so
/// the building ones mean a bug or bad stored data rather than bad input, but they are
/// typed like any other rule. The changes (fulfilling, cancelling, paying) refuse what
/// the order's state doesn't allow.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum OrderError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error("an order needs at least one line")]
    NoLines,
    #[error("order line {0} appears more than once")]
    DuplicateLine(OrderLineId),
    #[error("the order is in {expected}, but an amount is in {found}")]
    CurrencyMismatch { expected: Currency, found: Currency },
    #[error("line {0}'s total is not its unit price times its quantity")]
    LineTotalMismatch(OrderLineId),
    #[error("the order's totals don't add up from its lines")]
    TotalsMismatch,
    #[error("order numbers start at 1, got {0}")]
    InvalidNumber(i64),
    #[error("an order with shipping lines needs a shipping address")]
    ShippingWithoutAddress,
    #[error("a shipping line names order line {0}, which the order doesn't have")]
    UnknownShippedLine(OrderLineId),
    #[error("line {0} is fulfilled past its quantity")]
    FulfilledPastQuantity(OrderLineId),
    #[error("the order's fulfillment status isn't its lines'")]
    FulfillmentStatusMismatch,
    #[error("order {0} is cancelled")]
    Cancelled(OrderId),
    #[error("the order has no line {0}")]
    UnknownLine(OrderLineId),
    #[error("line {line} has {unfulfilled} left to fulfil, not {requested}")]
    OverFulfilled {
        line: OrderLineId,
        requested: u32,
        unfulfilled: u32,
    },
    #[error("line {line} has {fulfilled} fulfilled, not {requested}")]
    NotFulfilled {
        line: OrderLineId,
        requested: u32,
        fulfilled: u32,
    },
    /// Cancelling an order with standing fulfilments.
    #[error("order {0} has fulfilments; cancel them first")]
    HasFulfillments(OrderId),
    /// A list's text filter that breaks the text rule: the caller's input, unlike `Text`,
    /// which only bad stored data or a bug breaks.
    #[error("filter: {0}")]
    Filter(TextError),
    /// Sorting a list by total without the `currency` filter.
    #[error("sorting by total needs one currency: filter by currency")]
    SortNeedsCurrency,
}

impl ErrorCode for OrderError {
    fn code(&self) -> &'static str {
        match self {
            OrderError::Text(e) => e.code(),
            OrderError::Money(e) => e.code(),
            OrderError::NoLines => "order_has_no_lines",
            OrderError::DuplicateLine(_) => "duplicate_order_line",
            OrderError::CurrencyMismatch { .. } => "currency_mismatch",
            OrderError::LineTotalMismatch(_) => "line_total_mismatch",
            OrderError::TotalsMismatch => "totals_mismatch",
            OrderError::InvalidNumber(_) => "invalid_order_number",
            OrderError::ShippingWithoutAddress => "shipping_without_address",
            OrderError::UnknownShippedLine(_) => "unknown_shipped_line",
            OrderError::FulfilledPastQuantity(_) => "fulfilled_past_quantity",
            OrderError::FulfillmentStatusMismatch => "fulfillment_status_mismatch",
            OrderError::Cancelled(_) => "order_cancelled",
            OrderError::UnknownLine(_) => "unknown_order_line",
            OrderError::OverFulfilled { .. } => "over_fulfilled",
            OrderError::NotFulfilled { .. } => "line_not_fulfilled",
            OrderError::HasFulfillments(_) => "order_has_fulfillments",
            OrderError::Filter(e) => e.code(),
            OrderError::SortNeedsCurrency => "sort_needs_currency",
        }
    }
}
