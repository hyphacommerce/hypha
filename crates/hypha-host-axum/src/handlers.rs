//! The event handlers the native host runs (design note 0019, section 5): the composition
//! root for in-process destinations. A crate or spore with a handler adds it to the
//! tuple; the dispatcher runs whatever is here and knows none of them.

use std::sync::Arc;

use hypha_dispatch::VariantCleanup;
use hypha_http::mail::MailHost;
use hypha_mail::Mailer;
use hypha_merchandising::CategoryImageCleanup;
use hypha_search::SearchIndexer;

/// Every handler, in no particular order (names must be unique; a test checks).
pub type Handlers = (
    VariantCleanup,
    SearchIndexer,
    CategoryImageCleanup,
    Mailer<Arc<MailHost>>,
);

/// The handlers, built at startup: the `Mailer` holds the host's mail environment (the
/// sender, the store, the links), which exists only once the store is known.
pub fn handlers(mail: Arc<MailHost>) -> Handlers {
    (
        VariantCleanup,
        SearchIndexer,
        CategoryImageCleanup,
        Mailer::new(mail),
    )
}
