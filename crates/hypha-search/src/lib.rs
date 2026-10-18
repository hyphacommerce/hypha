//! Search and the storefront's product listing (design note 0019), the workflow:
//! documents built from products, prices and stock per market (`build_documents`), the
//! `SearchIndexer` event handler that queues what to rebuild, the `sync` job that
//! rebuilds it, and searching the market a shopper's country belongs to (`search`).
//!
//! The model and the ports (documents, queries, the engine, the queue, the settings)
//! are `hypha-search-index`'s, a leaf crate, so storage adapters don't depend on the
//! domains documents compose.

pub mod build;
pub mod commands;
pub mod handler;
pub mod sync;

pub use build::build_documents;
pub use commands::{CommandError, DEFAULT_OPTION_FACETS, Found, SearchInput, search};
pub use handler::SearchIndexer;
pub use sync::{SyncError, SyncPorts, SyncSettings, Synced, documents_for, sync};
