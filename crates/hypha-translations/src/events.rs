//! What happens to translations, as events. Contract types: a reshaped field is a new
//! `VERSION`, never an edit.

use hypha_core::{Event, Language};
use serde::Serialize;

use crate::catalog::ResourceRef;

/// The subject kind of a translation's events; the subject's ID is the resource's.
pub const TRANSLATION: &str = "translation";

/// The resource a translation is of: its kind (`product`) and its ID as the API shows
/// it (`prod_…`).
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TranslatedResource {
    pub kind: &'static str,
    pub id: String,
}

impl From<ResourceRef> for TranslatedResource {
    fn from(resource: ResourceRef) -> Self {
        Self {
            kind: resource.kind.as_str(),
            id: resource.id_string(),
        }
    }
}

/// `translation.updated`: a resource's translation into `language` was written.
/// `fields` are the keys whose translation was added, changed or removed, sorted. The
/// text isn't in it: read the translation.
#[derive(Debug, Serialize)]
pub struct TranslationUpdated {
    pub resource: TranslatedResource,
    pub language: Language,
    pub fields: Vec<String>,
}

impl Event for TranslationUpdated {
    const TYPE: &'static str = "translation.updated";
    const VERSION: u16 = 1;
}

/// `translation.deleted`: a resource has no translation into `language` any more. Not
/// recorded for translations that go with their resource or with a language taken off
/// the store's list: the resource's `*.deleted` and `store.updated` say those.
#[derive(Debug, Serialize)]
pub struct TranslationDeleted {
    pub resource: TranslatedResource,
    pub language: Language,
}

impl Event for TranslationDeleted {
    const TYPE: &'static str = "translation.deleted";
    const VERSION: u16 = 1;
}
