use hypha_core::{AddressError, Currency, EmailError, ErrorCode, MoneyError, TextError, VariantId};
use thiserror::Error;

/// How much of a purchase order line is still to arrive: what an over-received refusal
/// reports for each line it was refused on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outstanding {
    pub variant: VariantId,
    pub outstanding: u32,
}

/// A supplier, catalogue row or purchase order that breaks a rule.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum PurchasingError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Email(#[from] EmailError),
    #[error(transparent)]
    Address(#[from] AddressError),
    #[error("a supplier has at most {max} contacts")]
    TooManyContacts { max: usize },
    #[error("at most one contact is the primary one")]
    SeveralPrimaryContacts,
    #[error("net payment terms are 1 to {max} days, not {days}")]
    InvalidNetDays { days: u32, max: u32 },
    #[error("a lead time is at most {max} days, not {days}")]
    LeadTimeTooLong { days: u32, max: u32 },
    #[error("notes are at most {max} characters")]
    NotesTooLong { max: usize },
    #[error("a minimum order quantity is at least 1")]
    InvalidMinOrderQuantity,
    /// A catalogue cost is in the supplier's currency.
    #[error("the cost must be in {expected}, the supplier's currency, not {got}")]
    CostCurrencyMismatch { expected: Currency, got: Currency },
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error("purchase order numbers start at 1, got {0}")]
    InvalidNumber(i64),
    #[error("a purchase order has at most {max} lines")]
    TooManyLines { max: usize },
    #[error("variant {0} is on the purchase order twice: one line per variant")]
    DuplicateLine(VariantId),
    #[error("a line's quantity is at least 1")]
    InvalidLineQuantity(VariantId),
    /// A line's unit cost is in the order's currency.
    #[error(
        "the unit cost of variant {variant} must be in {expected}, the order's currency, not {got}"
    )]
    LineCurrencyMismatch {
        variant: VariantId,
        expected: Currency,
        got: Currency,
    },
    #[error("a purchase order has at most {max} extra costs")]
    TooManyCosts { max: usize },
    #[error("an extra cost or the tax is never negative")]
    NegativeCost,
    /// An extra cost is in the order's currency or the store's (duties paid at home).
    #[error("an extra cost is in {order} or {store}, not {got}")]
    ExtraCostCurrency {
        order: Currency,
        store: Currency,
        got: Currency,
    },
    #[error("the tax must be in {expected}, the order's currency, not {got}")]
    TaxCurrencyMismatch { expected: Currency, got: Currency },
    #[error("the purchase order is {status}, not a draft")]
    NotDraft { status: &'static str },
    #[error("the purchase order is {status}, not ordered")]
    NotOrdered { status: &'static str },
    #[error("the purchase order is {status}: it can't change any more")]
    NotOpen { status: &'static str },
    /// Cancelling would lose what arrived: close it instead.
    #[error("goods were received on this purchase order: close it instead of cancelling")]
    AlreadyReceived,
    #[error("a purchase order needs a destination before it's ordered")]
    DestinationRequired,
    #[error("a purchase order needs at least one line before it's ordered")]
    NoLines,
    /// Costs are kept in the store's base currency, so it must be chosen first.
    #[error("choose the store's base currency before ordering")]
    StoreCurrencyRequired,
    #[error("an order in {currency} needs an exchange rate to {store}, the store's currency")]
    ExchangeRateRequired { currency: Currency, store: Currency },
    #[error("the currency of an ordered purchase order doesn't change")]
    CurrencyFixed,
    /// A line goods were received or rejected on stays on the order.
    #[error("variant {0} was received on this order: its line stays")]
    LineReceived(VariantId),
    #[error(
        "variant {variant}'s quantity can't go below the {minimum} already received or rejected"
    )]
    BelowReceived { variant: VariantId, minimum: u32 },
    #[error("variant {0} isn't on the purchase order")]
    UnknownLine(VariantId),
    #[error("a receipt line receives or rejects at least one unit")]
    EmptyReceiptLine(VariantId),
    #[error("more than is outstanding would be received")]
    OverReceived(Vec<Outstanding>),
    /// Taking back more than a line counts: the receipt doesn't match the order.
    #[error("variant {0}'s counts are below what would be taken back")]
    TakeBackMismatch(VariantId),
}

impl ErrorCode for PurchasingError {
    fn code(&self) -> &'static str {
        match self {
            PurchasingError::Text(e) => e.code(),
            PurchasingError::Email(e) => e.code(),
            PurchasingError::Address(e) => e.code(),
            PurchasingError::TooManyContacts { .. } => "too_many_contacts",
            PurchasingError::SeveralPrimaryContacts => "several_primary_contacts",
            PurchasingError::InvalidNetDays { .. } => "invalid_payment_terms",
            PurchasingError::LeadTimeTooLong { .. } => "invalid_lead_time",
            PurchasingError::NotesTooLong { .. } => "notes_too_long",
            PurchasingError::InvalidMinOrderQuantity => "invalid_min_order_quantity",
            PurchasingError::CostCurrencyMismatch { .. } => "cost_currency_mismatch",
            PurchasingError::Money(e) => e.code(),
            PurchasingError::InvalidNumber(_) => "invalid_purchase_order_number",
            PurchasingError::TooManyLines { .. } => "too_many_lines",
            PurchasingError::DuplicateLine(_) => "duplicate_line",
            PurchasingError::InvalidLineQuantity(_) => "invalid_quantity",
            PurchasingError::LineCurrencyMismatch { .. }
            | PurchasingError::ExtraCostCurrency { .. }
            | PurchasingError::TaxCurrencyMismatch { .. } => "cost_currency_mismatch",
            PurchasingError::TooManyCosts { .. } => "too_many_costs",
            PurchasingError::NegativeCost => "negative_amount",
            PurchasingError::NotDraft { .. } => "purchase_order_not_draft",
            PurchasingError::NotOrdered { .. } => "purchase_order_not_ordered",
            PurchasingError::NotOpen { .. } => "purchase_order_not_open",
            PurchasingError::AlreadyReceived => "purchase_order_received",
            PurchasingError::DestinationRequired => "destination_required",
            PurchasingError::NoLines => "purchase_order_empty",
            PurchasingError::StoreCurrencyRequired => "store_currency_required",
            PurchasingError::ExchangeRateRequired { .. } => "exchange_rate_required",
            PurchasingError::CurrencyFixed => "purchase_order_currency_fixed",
            PurchasingError::LineReceived(_) => "purchase_order_line_received",
            PurchasingError::BelowReceived { .. } => "below_received",
            PurchasingError::UnknownLine(_) => "unknown_purchase_order_line",
            PurchasingError::EmptyReceiptLine(_) => "empty_receipt_line",
            PurchasingError::OverReceived(_) => "over_received",
            PurchasingError::TakeBackMismatch(_) => "receipt_mismatch",
        }
    }
}
