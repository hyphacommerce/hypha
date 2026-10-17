use hypha_core::{CategoryId, DescriptionError, ErrorCode, HandleError, TextError};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CategoryError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Handle(#[from] HandleError),
    #[error(transparent)]
    Description(#[from] DescriptionError),
    /// A category can't be its own parent, nor sit under one of its descendants.
    #[error("category {category} can't move under {parent}: that would make a cycle")]
    Cycle {
        category: CategoryId,
        parent: CategoryId,
    },
    #[error("categories nest at most {max} levels deep")]
    TooDeep { max: u8 },
}

impl ErrorCode for CategoryError {
    fn code(&self) -> &'static str {
        match self {
            CategoryError::Text(e) => e.code(),
            CategoryError::Handle(e) => e.code(),
            CategoryError::Description(e) => e.code(),
            CategoryError::Cycle { .. } => "category_cycle",
            CategoryError::TooDeep { .. } => "category_too_deep",
        }
    }
}
