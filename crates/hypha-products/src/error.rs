use hypha_core::{OptionId, OptionValueId, VariantId};

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
    #[error("variant {0} already exists")]
    DuplicateVariantId(VariantId),
    #[error("expected {expected} option selections, got {got}")]
    SelectionCountMismatch { expected: usize, got: usize },
    #[error("option value {0} doesn't belong to the option in its position")]
    UnknownOptionValue(OptionValueId),
    #[error("a variant with this option combintion already exists")]
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
}
