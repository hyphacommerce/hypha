use hypha_core::{ErrorCode, MoneyError, OptionId, OptionValueId, TextError, VariantId};

use crate::ProductStatus;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HandleError {
    #[error("handle is empty")]
    Empty,
    #[error("invalid character {0:?}")]
    InvalidChar(char),
    #[error("handle can't start or end with '-'")]
    EdgeDash,
    #[error("handle can't contain '--'")]
    DoubleDash,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProductError {
    #[error("invalid handle: {0}")]
    InvalidHandle(HandleError),
    #[error(transparent)]
    Text(#[from] TextError),
    #[error("variant {0} already exists")]
    DuplicateVariantId(VariantId),
    #[error("expected {expected} option selections, got {got}")]
    SelectionCountMismatch { expected: usize, got: usize },
    #[error("option value {0} doesn't belong to the option in its position")]
    UnknownOptionValue(OptionValueId),
    #[error("a variant with this option combination already exists")]
    DuplicateCombination,
    #[error("a configurable product needs at least one option")]
    NoOptions,
    #[error("option {0} has no values")]
    EmptyOption(OptionId),
    #[error("option name {0:?} is used twice")]
    DuplicateOptionName(String),
    #[error("option value {0} is used twice")]
    DuplicateOptionValue(OptionValueId),
    #[error("invalid status transition from {from} to {to}")]
    InvalidTransition {
        from: ProductStatus,
        to: ProductStatus,
    },
    #[error("a product needs at least one variant")]
    NoVariants,
    #[error("invalid price: {0}")]
    InvalidPrice(MoneyError),
}

impl ErrorCode for HandleError {
    fn code(&self) -> &'static str {
        match self {
            HandleError::Empty
            | HandleError::InvalidChar(_)
            | HandleError::EdgeDash
            | HandleError::DoubleDash => "invalid_handle",
        }
    }
}

impl ErrorCode for ProductError {
    fn code(&self) -> &'static str {
        match self {
            ProductError::InvalidHandle(e) => e.code(),
            ProductError::Text(e) => e.code(),
            ProductError::DuplicateVariantId(_) => "duplicate_variant",
            ProductError::SelectionCountMismatch { .. } => "selection_count_mismatch",
            ProductError::UnknownOptionValue(_) => "unknown_option_value",
            ProductError::DuplicateCombination => "duplicate_option_combination",
            ProductError::NoOptions => "no_options",
            ProductError::EmptyOption(_) => "empty_option",
            ProductError::DuplicateOptionName(_) => "duplicate_option_name",
            ProductError::DuplicateOptionValue(_) => "duplicate_option_value",
            ProductError::InvalidTransition { .. } => "invalid_status_transition",
            ProductError::NoVariants => "no_variants",
            ProductError::InvalidPrice(e) => e.code(),
        }
    }
}
