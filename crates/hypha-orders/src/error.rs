use hypha_core::{Currency, ErrorCode, MoneyError, OrderLineId, TextError};
use thiserror::Error;

/// Why an order can't be built. Checkout builds orders from a priced cart, so these
/// mean a bug or bad stored data rather than bad input, but they are typed like any
/// other rule.
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
        }
    }
}
