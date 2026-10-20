//! The event handlers the native host runs (design note 0019, section 5): the composition
//! root for core's in-process destinations. A domain crate with a handler adds it to the
//! tuple; a spore contributes its own (`Spore::Handlers`), joined to these by
//! `PgStorage` (design note 0030, §4). The dispatcher runs whatever it's given and knows
//! none of them.

use std::sync::Arc;

use hypha_custom_fields::CustomFieldCleanup;
use hypha_dispatch::VariantCleanup;
use hypha_http::mail::MailHost;
use hypha_mail::Mailer;
use hypha_merchandising::CategoryImageCleanup;
use hypha_search::SearchIndexer;

/// Core's handlers, in no particular order (names must be unique, spores' included; a
/// test and startup check).
pub type CoreHandlers = (
    VariantCleanup,
    SearchIndexer,
    CategoryImageCleanup,
    CustomFieldCleanup,
    Mailer<Arc<MailHost>>,
);

/// Core's handlers, built at startup: the `Mailer` holds the host's mail environment (the
/// sender, the store, the links), which exists only once the store is known.
pub fn core_handlers(mail: Arc<MailHost>) -> CoreHandlers {
    (
        VariantCleanup,
        SearchIndexer,
        CategoryImageCleanup,
        CustomFieldCleanup,
        Mailer::new(mail),
    )
}
