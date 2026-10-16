use hypha_core::{
    AssetId, DescriptionError, ErrorCode, HandleError, MoneyError, OptionId, OptionValueId,
    TextError, VariantId,
};

use crate::ProductStatus;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
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
    #[error("a product has at most {max} variants")]
    TooManyVariants { max: usize },
    /// The variant named in the request's URL isn't this product's.
    #[error("variant {0} not found")]
    VariantNotFound(VariantId),
    /// A variant named in the request's body isn't this product's.
    #[error("variant {0} isn't one of this product's")]
    UnknownVariant(VariantId),
    #[error("option {0} isn't one of this product's")]
    UnknownOption(OptionId),
    #[error("option {0} is listed twice")]
    DuplicateOption(OptionId),
    #[error("option {option:?} has the value {label:?} twice")]
    DuplicateOptionLabel { option: String, label: String },
    /// Removing option values would orphan these variants; naming them for removal in
    /// the same request removes them too.
    #[error("variants use the option values being removed: {}", join(variants))]
    OptionValueInUse { variants: Vec<VariantId> },
    /// After removing an option, two variants would be the same combination.
    #[error("variants {first} and {second} would have the same options")]
    VariantsCollide { first: VariantId, second: VariantId },
    #[error("SKU {0:?} is used by another variant of this product")]
    DuplicateSku(String),
    #[error("a product has at most {max} media items")]
    TooManyMedia { max: usize },
    #[error("asset {0} is listed twice")]
    DuplicateMedia(AssetId),
    #[error("asset {0} isn't in the product's media")]
    MediaNotInProduct(AssetId),
    #[error("a description is at most {max} bytes")]
    DescriptionTooLong { max: usize },
    #[error("a product has at most {max} tags")]
    TooManyTags { max: usize },
}

fn join(ids: &[VariantId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
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
            ProductError::TooManyVariants { .. } => "too_many_variants",
            ProductError::VariantNotFound(_) => "variant_not_found",
            ProductError::UnknownVariant(_) => "unknown_variant",
            ProductError::UnknownOption(_) => "unknown_option",
            ProductError::DuplicateOption(_) => "duplicate_option",
            ProductError::DuplicateOptionLabel { .. } => "duplicate_option_value",
            ProductError::OptionValueInUse { .. } => "option_value_in_use",
            ProductError::VariantsCollide { .. } => "duplicate_option_combination",
            ProductError::DuplicateSku(_) => "sku_taken",
            ProductError::TooManyMedia { .. } => "too_many_media",
            ProductError::DuplicateMedia(_) => "duplicate_media",
            ProductError::MediaNotInProduct(_) => "media_not_in_product",
            ProductError::DescriptionTooLong { .. } => "description_too_long",
            ProductError::TooManyTags { .. } => "too_many_tags",
        }
    }
}

/// Handles are core's rule (`hypha_core::Handle`); a product reports a bad one as its own.
impl From<HandleError> for ProductError {
    fn from(e: HandleError) -> Self {
        ProductError::InvalidHandle(e)
    }
}

impl From<DescriptionError> for ProductError {
    fn from(e: DescriptionError) -> Self {
        match e {
            DescriptionError::TooLong { max } => ProductError::DescriptionTooLong { max },
        }
    }
}
