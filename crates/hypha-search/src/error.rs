use hypha_core::{ErrorCode, TextError};
use thiserror::Error;

use crate::FacetKey;

/// A search or its settings broke a rule.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SearchError {
    #[error(transparent)]
    Text(TextError),
    #[error("option {0:?} splits listings twice")]
    DuplicateSplit(String),
    #[error("listings split by at most {max} options")]
    TooManySplits { max: usize },
    #[error("facet {0} is listed twice")]
    DuplicateFacet(FacetKey),
    #[error("at most {max} facets are shown")]
    TooManyFacets { max: usize },
    /// Not `vendor`, `product_type`, `tag` or `option.<name>`.
    #[error("unknown facet {0:?}")]
    UnknownFacet(String),
    /// A filter isn't `key:value`.
    #[error("filter {0:?} isn't key:value")]
    InvalidFilter(String),
    #[error("the price range's minimum is above its maximum")]
    InvalidPriceRange,
    #[error("a search pages at most {max} results deep")]
    TooDeep { max: u32 },
    #[error("a page has 1 to {max} results")]
    InvalidLimit { max: u32 },
}

impl ErrorCode for SearchError {
    fn code(&self) -> &'static str {
        match self {
            SearchError::Text(e) => e.code(),
            SearchError::DuplicateSplit(_) => "duplicate_split",
            SearchError::TooManySplits { .. } => "too_many_splits",
            SearchError::DuplicateFacet(_) => "duplicate_facet",
            SearchError::TooManyFacets { .. } => "too_many_facets",
            SearchError::UnknownFacet(_) => "unknown_facet",
            SearchError::InvalidFilter(_) => "invalid_filter",
            SearchError::InvalidPriceRange => "invalid_price_range",
            SearchError::TooDeep { .. } => "search_too_deep",
            SearchError::InvalidLimit { .. } => "invalid_limit",
        }
    }
}

/// The search engine failed (unreachable, refused, storage). The text is for logs;
/// clients see `search_unavailable`.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("search engine: {0}")]
pub struct IndexError(pub String);

impl ErrorCode for IndexError {
    fn code(&self) -> &'static str {
        "search_unavailable"
    }
}

impl From<hypha_core::RepoError> for IndexError {
    fn from(e: hypha_core::RepoError) -> Self {
        IndexError(e.to_string())
    }
}
