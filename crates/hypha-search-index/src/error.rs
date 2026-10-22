use hypha_core::{AppKey, AppKeyError, ErrorCode, TextError};
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
    /// Not `vendor`, `product_type`, `tag`, `option.<name>` or
    /// `field.<namespace>.<key>`.
    #[error("unknown facet {0:?}")]
    UnknownFacet(String),
    /// A `field.<namespace>.<key>` facet whose field can't be one: no product or variant
    /// field has that key, or it isn't `filterable`, shown on storefronts, and of type
    /// text, choice, boolean, integer or a list of those.
    #[error(
        "custom field {0} can't be a search facet: it must be a filterable product or variant field shown on storefronts, of type text, choice, boolean or integer (or a list of them)"
    )]
    FacetFieldNotAllowed(String),
    /// A filter isn't `key:value`.
    #[error("filter {0:?} isn't key:value")]
    InvalidFilter(String),
    #[error("the price range's minimum is above its maximum")]
    InvalidPriceRange,
    #[error("a search pages at most {max} results deep")]
    TooDeep { max: u32 },
    #[error("a page has 1 to {max} results")]
    InvalidLimit { max: u32 },
    /// Not an app key.
    #[error(transparent)]
    SearchApp(AppKeyError),
    /// The app isn't installed, isn't enabled, or doesn't answer `search.query`.
    #[error("app {0} doesn't answer search.query")]
    SearchAppUnavailable(AppKey),
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
            SearchError::FacetFieldNotAllowed(_) => "facet_field_not_allowed",
            SearchError::InvalidFilter(_) => "invalid_filter",
            SearchError::InvalidPriceRange => "invalid_price_range",
            SearchError::TooDeep { .. } => "search_too_deep",
            SearchError::InvalidLimit { .. } => "invalid_limit",
            SearchError::SearchApp(e) => e.code(),
            SearchError::SearchAppUnavailable(_) => "search_app_unavailable",
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
