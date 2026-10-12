use hypha_core::ErrorCode;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("store name can't be empty")]
    EmptyName,
    #[error("store name can be at most {max} characters")]
    NameTooLong { max: usize },
}

impl ErrorCode for StoreError {
    fn code(&self) -> &'static str {
        match self {
            StoreError::EmptyName => "store_name_empty",
            StoreError::NameTooLong { .. } => "store_name_too_long",
        }
    }
}
