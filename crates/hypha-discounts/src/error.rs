use hypha_core::{Currency, ErrorCode, MarketId};

/// One broken rule of a discount, where it is (`kind.rewards[0].value.basis_points`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub path: String,
    /// Stable snake_case: `empty`, `too_many`, `duplicate`, `too_small`, `out_of_range`,
    /// `duplicate_class`, `allocation_not_allowed`, `value_not_allowed`,
    /// `threshold_mismatch`, `not_ascending`, `markets_differ`, `ends_not_after_start`,
    /// `invalid_text`.
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum DiscountError {
    /// Every rule the discount breaks, at once.
    #[error("the discount breaks {} rule(s): {}", .0.len(), .0.iter().map(|p| format!("{}: {}", p.path, p.message)).collect::<Vec<_>>().join("; "))]
    Invalid(Vec<Problem>),
    #[error("{path}: market {market} not found")]
    MarketNotFound { path: String, market: MarketId },
    #[error("{path}: market {market} sells in {expected}, not {found}")]
    CurrencyMismatch {
        path: String,
        market: MarketId,
        expected: Currency,
        found: Currency,
    },
    /// Automatic or by code is chosen once.
    #[error("a discount's method can't change")]
    MethodFixed,
}

impl ErrorCode for DiscountError {
    fn code(&self) -> &'static str {
        match self {
            DiscountError::Invalid(_) => "invalid_discount",
            DiscountError::MarketNotFound { .. } => "market_not_found",
            DiscountError::CurrencyMismatch { .. } => "discount_currency_mismatch",
            DiscountError::MethodFixed => "discount_method_fixed",
        }
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CodeError {
    #[error("a discount code is 1 to {max} characters", max = crate::code::MAX_CODE_LEN)]
    Length,
    /// Codes are typed by buyers: no spaces or control characters inside.
    #[error("a discount code has no spaces or control characters: {0:?}")]
    Character(String),
    #[error("a generated code is {min} to {max} characters long", min = crate::code::MIN_GENERATED_LEN, max = crate::code::MAX_GENERATED_LEN)]
    GeneratedLength,
    #[error("at most {max} codes at once", max = crate::code::MAX_GENERATED)]
    TooMany,
    #[error("a batch label is 1 to 255 characters")]
    Batch,
    #[error("a code's usage limit is at least 1")]
    UsageLimit,
}

impl ErrorCode for CodeError {
    fn code(&self) -> &'static str {
        match self {
            CodeError::Length => "invalid_discount_code",
            CodeError::Character(_) => "invalid_discount_code",
            CodeError::GeneratedLength => "invalid_code_length",
            CodeError::TooMany => "too_many_codes",
            CodeError::Batch => "invalid_code_batch",
            CodeError::UsageLimit => "invalid_code_usage_limit",
        }
    }
}
