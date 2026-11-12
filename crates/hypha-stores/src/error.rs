use hypha_core::{AddressError, ErrorCode, Language, TextError};

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
    #[error("a setup step is 1 to 40 lowercase letters, digits, - or _, not {0:?}")]
    InvalidSetupStep(String),
    #[error("a store has at most {max} languages beside its default one")]
    TooManyLanguages { max: usize },
    #[error("language {0} is listed twice")]
    DuplicateLanguage(Language),
    #[error("{0} is the store's default language, so it isn't listed among the others")]
    LanguageIsDefault(Language),
    #[error(
        "{0} is one of the store's other languages: take it off that list to make it the default"
    )]
    DefaultLanguageListed(Language),
}

impl ErrorCode for StoreError {
    fn code(&self) -> &'static str {
        match self {
            StoreError::EmptyName => "store_name_empty",
            StoreError::NameTooLong { .. } => "store_name_too_long",
            StoreError::DescriptionTooLong { .. } => "store_description_too_long",
            StoreError::Text(e) => e.code(),
            StoreError::Address(e) => e.code(),
            StoreError::InvalidSetupStep(_) => "invalid_setup_step",
            StoreError::TooManyLanguages { .. } => "too_many_languages",
            StoreError::DuplicateLanguage(_) => "duplicate_language",
            StoreError::LanguageIsDefault(_) => "language_is_default",
            StoreError::DefaultLanguageListed(_) => "default_language_listed",
        }
    }
}
