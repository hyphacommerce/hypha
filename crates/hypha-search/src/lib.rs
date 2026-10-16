//! Search and the storefront's product listing (design note 0019): documents built
//! from products, prices and stock per market; a `SearchIndex` port engines implement;
//! a queue of what to rebuild, fed by the `SearchIndexer` event handler and worked by
//! the `sync` job; the merchant's settings; and the commands.
//!
//! A workflow crate: documents compose products, markets, pricing and inventory.

pub mod commands;
pub mod document;
pub mod error;
pub mod handler;
pub mod query;
pub mod reference;
pub mod repo;
pub mod settings;
pub mod sync;
pub mod terms;

pub use commands::{
    CommandError, DEFAULT_OPTION_FACETS, Found, SearchInput, SearchStatus, UpdateSearchSettings,
    get_search_settings, reindex, search, search_status, update_search_settings,
};
pub use document::{PriceRange, SearchDocument, WeightedTerms, build_documents};
pub use error::{IndexError, SearchError};
pub use handler::SearchIndexer;
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
pub use sync::{SyncError, SyncPorts, SyncSettings, queue_all, sync};
