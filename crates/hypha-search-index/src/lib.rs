//! The search index (design note 0019): what search documents and queries are, what a
//! search means (`reference::search`), the ports engines and storage implement (the
//! `SearchIndex` engine, the `SearchQueue` of what to rebuild, the settings), and the
//! merchant's settings.
//!
//! A leaf crate on core, so storage adapters and engines depend on it without depending
//! on the domains documents are built from. Building and syncing documents, and
//! searching a market, are the `hypha-search` workflow's.

pub mod commands;
pub mod document;
pub mod error;
pub mod query;
pub mod reference;
pub mod repo;
pub mod settings;
pub mod terms;

pub use commands::{
    CommandError, SEARCH_POINT, SearchStatus, UpdateSearchSettings, get_search_settings, queue_all,
    reindex, search_status, update_search_settings,
};
pub use document::{PriceRange, SearchDocument, WeightedTerms};
pub use error::{IndexError, SearchError};
pub use query::{
    FacetCounts, FacetKey, FacetValue, Filter, Group, Hit, MAX_DEPTH, MAX_FACET_VALUES, MAX_LIMIT,
    PriceBounds, SearchQuery, SearchResults, Sort,
};
pub use repo::{
    HasSearchQueue, HasSearchSettings, Mark, SearchIndex, SearchQueue, SearchSettingsRepo, Target,
    keys,
};
pub use settings::{
    FacetSetting, SEARCH_SETTINGS, SearchSettings, SearchSettingsChanged, SearchSettingsSnapshot,
};
