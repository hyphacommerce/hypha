use hypha_core::{AddressError, ErrorCode, TextError};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("store name can't be empty")]
    EmptyName,
    #[error("store name can be at most {max} characters")]
    NameTooLong { max: usize },
    #[error("store description can be at most {max} characters")]
    DescriptionTooLong { max: usize },
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Address(#[from] AddressError),
}

impl ErrorCode for StoreError {
    fn code(&self) -> &'static str {
        match self {
            StoreError::EmptyName => "store_name_empty",
            StoreError::NameTooLong { .. } => "store_name_too_long",
            StoreError::DescriptionTooLong { .. } => "store_description_too_long",
            StoreError::Text(e) => e.code(),
            StoreError::Address(e) => e.code(),
        }
    }
}
