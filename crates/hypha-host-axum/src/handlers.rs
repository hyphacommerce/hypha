//! The event handlers the native host runs (design note 0019, section 5): the composition
//! root for in-process destinations. A crate or spore with a handler adds it to the
//! tuple; the dispatcher runs whatever is here and knows none of them.

use hypha_dispatch::VariantCleanup;
use hypha_merchandising::CategoryImageCleanup;
use hypha_search::SearchIndexer;

/// Every handler, in no particular order (names must be unique; a test checks).
pub type Handlers = (VariantCleanup, SearchIndexer, CategoryImageCleanup);

pub const HANDLERS: Handlers = (VariantCleanup, SearchIndexer, CategoryImageCleanup);
