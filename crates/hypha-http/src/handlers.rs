//! The event handlers every host runs: the composition root for core's in-process
//! destinations, shared by the native host and the Worker so both deliver events to the
//! same places. A domain crate with a handler adds it to the tuple; a spore contributes its
//! own (`Spore::Handlers`), joined to these by the host's storage. The dispatcher runs
//! whatever it's given and knows none of them.

use std::sync::Arc;

use hypha_custom_fields::CustomFieldCleanup;
use hypha_dispatch::VariantCleanup;
use hypha_mail::Mailer;
use hypha_merchandising::CategoryImageCleanup;
use hypha_search::SearchIndexer;
use hypha_translations::TranslationCleanup;

use crate::{
    mail::MailHost,
    translations::{LanguageCleanup, TranslationSourceSync, ValueTranslationCleanup},
};

/// Core's handlers, in no particular order (names must be unique, spores' included; a
/// test and startup check). A tuple, not a list of trait objects, so `Send` stays provable.
pub type CoreHandlers = (
    VariantCleanup,
    SearchIndexer,
    CategoryImageCleanup,
    CustomFieldCleanup,
    TranslationCleanup,
    LanguageCleanup,
    ValueTranslationCleanup,
    TranslationSourceSync,
    Mailer<Arc<MailHost>>,
);

/// Core's handlers, built once the store is known: the `Mailer` holds the host's mail
/// environment (the sender, the store, the links), the worklist's sync the store's ID.
pub fn core_handlers(mail: Arc<MailHost>) -> CoreHandlers {
    let store = hypha_mail::MailEnv::store(&*mail);
    (
        VariantCleanup,
        SearchIndexer,
        CategoryImageCleanup,
        CustomFieldCleanup,
        TranslationCleanup,
        LanguageCleanup,
        ValueTranslationCleanup,
        TranslationSourceSync::new(store),
        Mailer::new(mail),
    )
}
