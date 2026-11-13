use std::{cmp::Ordering, fmt, str::FromStr};

use hypha_core::{Language, Page, PageRequest, RepoError};
use uuid::Uuid;

use crate::{
    catalog::{ResourceKind, ResourceRef},
    translation::Translation,
    worklist::{LanguageProgress, QueuedSource, SourceDigest, WorkItem, WorklistFilter},
};

/// Uniqueness rules the translations repo enforces, reported as
/// `RepoError::Duplicate(key)`.
pub mod keys {
    /// A resource has one translation per language.
    pub const TRANSLATION: &str = "translation";
}

/// What identifies a translation, and where a list of them continues: they're listed by
/// kind (its name), then resource ID, then language tag, each compared as bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TranslationKey {
    pub resource: ResourceRef,
    pub language: Language,
}

impl TranslationKey {
    pub fn of(translation: &Translation) -> Self {
        Self {
            resource: translation.resource(),
            language: translation.language().clone(),
        }
    }
}

impl Ord for TranslationKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let parts = |k: &'_ Self| (k.resource.kind.as_str(), k.resource.id);
        parts(self)
            .cmp(&parts(other))
            .then_with(|| self.language.as_str().cmp(other.language.as_str()))
    }
}

impl PartialOrd for TranslationKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `product:<uuid>:de`, for cursors.
impl fmt::Display for TranslationKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.resource.kind,
            self.resource.id.simple(),
            self.language
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a translation's key")]
pub struct InvalidTranslationKey;

impl FromStr for TranslationKey {
    type Err = InvalidTranslationKey;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(3, ':');
        let mut part = || parts.next().ok_or(InvalidTranslationKey);
        let kind: ResourceKind = part()?.parse().map_err(|_| InvalidTranslationKey)?;
        let id = Uuid::parse_str(part()?).map_err(|_| InvalidTranslationKey)?;
        let language = Language::new(part()?).map_err(|_| InvalidTranslationKey)?;
        Ok(Self {
            resource: ResourceRef::new(kind, id),
            language,
        })
    }
}

/// Which translations a list shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TranslationFilter {
    /// Of these kinds only; `None` is every kind, an empty list none.
    pub kinds: Option<Vec<ResourceKind>>,
    /// Into this language only.
    pub language: Option<Language>,
}

impl TranslationFilter {
    pub fn matches(&self, translation: &Translation) -> bool {
        self.kinds
            .as_ref()
            .is_none_or(|kinds| kinds.contains(&translation.resource().kind))
            && self
                .language
                .as_ref()
                .is_none_or(|language| language == translation.language())
    }
}

/// The store's translations, one per resource and language.
#[allow(async_fn_in_trait)]
pub trait TranslationRepo {
    async fn get(
        &self,
        resource: ResourceRef,
        language: &Language,
    ) -> Result<Option<Translation>, RepoError>;
    /// Every translation of one resource, by language tag.
    async fn for_resource(&self, resource: ResourceRef) -> Result<Vec<Translation>, RepoError>;
    /// The translations of any of `resources` into any of `languages`, in one read: what
    /// a page of resources is overlaid with. In `TranslationKey` order.
    async fn load(
        &self,
        resources: &[ResourceRef],
        languages: &[Language],
    ) -> Result<Vec<Translation>, RepoError>;
    /// A page of the translations `filter` keeps, in `TranslationKey` order.
    async fn list(
        &self,
        filter: &TranslationFilter,
        page: PageRequest<TranslationKey>,
    ) -> Result<Page<Translation>, RepoError>;
    /// Whether the store has a translation into any of `languages`.
    async fn any_in(&self, languages: &[Language]) -> Result<bool, RepoError>;
    /// Stores a new translation (version 1) with its events. Enforces `TRANSLATION`.
    async fn insert(&self, translation: &mut Translation) -> Result<(), RepoError>;
    /// Stores its changes with their events when still at the version loaded; otherwise
    /// `StaleVersion`.
    async fn update(&self, translation: &mut Translation) -> Result<(), RepoError>;
    /// Removes it, writing its events, when still at the version loaded; otherwise
    /// `StaleVersion`.
    async fn delete(&self, translation: &mut Translation) -> Result<(), RepoError>;
    /// Removes every translation of `resources`, in every language. Records no events
    /// and checks no versions: for resources that are gone. Removing none is fine.
    async fn delete_resources(&self, resources: &[ResourceRef]) -> Result<(), RepoError>;
    /// Removes every translation into a language not among `languages`. Records no
    /// events and checks no versions: for languages taken off the store's list.
    async fn retain_languages(&self, languages: &[Language]) -> Result<(), RepoError>;
}

/// The worklist's side of the store: each resource's source text as digests, a read model
/// kept beside the translations (the host reads the resource and writes them), and the
/// resources waiting to be read again. Its reads join the translations, so the same store
/// keeps both.
#[allow(async_fn_in_trait)]
pub trait TranslationSourceRepo {
    /// Sets `resource`'s digests to `digests` (`source_digests`: one per key), replacing
    /// what it had; none removes them, as for a resource that's gone. Records no events.
    async fn replace_sources(
        &self,
        resource: ResourceRef,
        digests: &[SourceDigest],
    ) -> Result<(), RepoError>;
    /// The digests of `resources`, by resource then key.
    async fn sources(
        &self,
        resources: &[ResourceRef],
    ) -> Result<Vec<(ResourceRef, SourceDigest)>, RepoError>;
    /// A page of the resources with digests that `filter` keeps (`WorklistFilter::keeps`,
    /// counted by `FieldCounts::of` against the translation into its language), in
    /// `ResourceRef` order: the catalog's order of kinds, then the ID's bytes. One read.
    async fn worklist(
        &self,
        filter: &WorklistFilter,
        page: PageRequest<ResourceRef>,
    ) -> Result<Page<WorkItem>, RepoError>;
    /// For each of `languages`, in the order given, how its translations cover the digests
    /// of `kinds`' resources, in fields. One read.
    async fn progress(
        &self,
        kinds: &[ResourceKind],
        languages: &[Language],
    ) -> Result<Vec<LanguageProgress>, RepoError>;
    /// Queues `resources` to have their digests read again; one queued already moves to
    /// the back of the queue (a later `mark`).
    async fn queue(&self, resources: &[ResourceRef]) -> Result<(), RepoError>;
    /// Queues every resource of `kind` that has digests: what a change reaching all of
    /// them (a custom field definition's access) does.
    async fn queue_kind(&self, kind: ResourceKind) -> Result<(), RepoError>;
    /// Up to `limit` queued resources, longest waiting first.
    async fn queued(&self, limit: u32) -> Result<Vec<QueuedSource>, RepoError>;
    /// Takes `done` off the queue, each only if it wasn't queued again since it was read
    /// (its `mark` is the same).
    async fn unqueue(&self, done: &[QueuedSource]) -> Result<(), RepoError>;
    /// Gives those of `resources` that are queued a later mark, leaving the rest
    /// unqueued: a writer that read one of them since the queue's reader did tells it its
    /// read may be older than this one's, so it reads again instead of unqueueing.
    async fn touch_queued(&self, resources: &[ResourceRef]) -> Result<(), RepoError>;
}

pub trait HasTranslations {
    type Translations: TranslationRepo + TranslationSourceRepo;
    fn translations(&self) -> &Self::Translations;
}

impl<T: HasTranslations + ?Sized> HasTranslations for &T {
    type Translations = T::Translations;
    fn translations(&self) -> &Self::Translations {
        (**self).translations()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(kind: ResourceKind, id: u128, language: &str) -> TranslationKey {
        TranslationKey {
            resource: ResourceRef::new(kind, Uuid::from_u128(id)),
            language: Language::new(language).unwrap(),
        }
    }

    #[test]
    fn a_key_reads_back_from_its_text() {
        let k = key(ResourceKind::CustomFieldDefinition, 7, "sr-Latn-RS");
        assert_eq!(k.to_string().parse(), Ok(k));
        assert_eq!(
            "product:nope:de".parse::<TranslationKey>(),
            Err(InvalidTranslationKey)
        );
        assert_eq!(
            "product".parse::<TranslationKey>(),
            Err(InvalidTranslationKey)
        );
    }

    #[test]
    fn keys_order_by_kind_name_then_id_then_language() {
        let mut keys = vec![
            key(ResourceKind::Product, 1, "de"),
            key(ResourceKind::Category, 2, "de"),
            key(ResourceKind::Category, 1, "sr-Latn"),
            key(ResourceKind::Category, 1, "de"),
            key(ResourceKind::Asset, 9, "de"),
        ];
        keys.sort();
        assert_eq!(
            keys,
            vec![
                key(ResourceKind::Asset, 9, "de"),
                key(ResourceKind::Category, 1, "de"),
                key(ResourceKind::Category, 1, "sr-Latn"),
                key(ResourceKind::Category, 2, "de"),
                key(ResourceKind::Product, 1, "de"),
            ]
        );
    }
}
