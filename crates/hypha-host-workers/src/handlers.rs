//! The event handlers the Worker runs (design note 0019, section 5): the composition
//! root for core's in-process destinations. A spore contributes its own
//! (`Spore::Handlers`), joined to these by `WorkersStorage` (design note 0030, §4); the
//! dispatcher runs whatever it's given and knows none of them.

use std::sync::Arc;

use hypha_custom_fields::CustomFieldCleanup;
use hypha_dispatch::VariantCleanup;
use hypha_http::mail::MailHost;
use hypha_mail::Mailer;
use hypha_merchandising::CategoryImageCleanup;
use hypha_search::SearchIndexer;

/// Core's handlers, in no particular order: the same as `hypha-host-axum`'s (whose test
/// checks the names are unique), so both hosts deliver events to the same destinations.
/// Keep the two in step.
pub type CoreHandlers = (
    VariantCleanup,
    SearchIndexer,
    CategoryImageCleanup,
    CustomFieldCleanup,
    Mailer<Arc<MailHost>>,
);

/// Core's handlers, built when the isolate sets up: the `Mailer` holds the host's mail
/// environment, which exists only once the store is known.
pub fn core_handlers(mail: Arc<MailHost>) -> CoreHandlers {
    (
        VariantCleanup,
        SearchIndexer,
        CategoryImageCleanup,
        CustomFieldCleanup,
        Mailer::new(mail),
    )
}
