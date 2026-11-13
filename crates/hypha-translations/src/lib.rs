//! Translations: the merchant's content in the store's other languages, kept beside
//! the content rather than in it. One `Translation` per resource and language holds the
//! translated fields, each with a digest of the source text it translated. A leaf crate
//! on core: hosts hand it the resource's fields as plain values.

pub mod catalog;
pub mod commands;
pub mod error;
pub mod events;
pub mod handler;
pub mod overlay;
pub mod repo;
pub mod source;
pub mod translation;

pub use catalog::{
    FieldRule, FieldSpec, MAX_KEY_LEN, ResourceKind, ResourceRef, UnknownKind, child_key,
};
pub use commands::{
    CommandError, StoreLanguages, delete_translation, get_translations, list_translations,
    put_translation, require_read, require_write,
};
pub use error::{Problem, TranslationError};
pub use events::{TRANSLATION, TranslatedResource, TranslationDeleted, TranslationUpdated};
pub use handler::TranslationCleanup;
pub use overlay::Overlay;
pub use repo::{
    HasTranslations, InvalidTranslationKey, TranslationFilter, TranslationKey, TranslationRepo,
    keys,
};
pub use source::{Source, SourceField, source_digest};
pub use translation::{FieldInput, TranslatedField, Translation, TranslationSnapshot};
