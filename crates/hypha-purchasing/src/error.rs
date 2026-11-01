use hypha_core::{AddressError, Currency, EmailError, ErrorCode, TextError};
use thiserror::Error;

/// A supplier or catalogue row that breaks a rule.
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
        }
    }
}
