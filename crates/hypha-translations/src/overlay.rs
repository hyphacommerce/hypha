//! Translations as a response reads them: the text of a field in the language asked
//! for, falling back to the language without its region.

use std::collections::HashMap;

use hypha_core::{Language, RepoError};

use crate::{
    catalog::ResourceRef,
    repo::{HasTranslations, TranslationRepo},
    translation::Translation,
};

/// The translations of some resources into one language. A field without one answers
/// `None`: the caller shows the source.
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    /// Per resource, its translations from the exact language to the fallback.
    translations: HashMap<ResourceRef, Vec<Translation>>,
}

impl Overlay {
    /// An overlay that translates nothing: for the store's default language.
    pub fn none() -> Self {
        Self::default()
    }

    /// Reads the translations of `resources` into `language` and into the language it
    /// falls back to (`de` for `de-AT`), in one read, each only if among `servable`:
    /// the languages this reader may be shown. A storefront passes the store's
    /// published languages, so an unpublished one is never served, not even as a
    /// fallback; a preview for staff passes every language on the store's list.
    pub async fn load<S: HasTranslations>(
        ports: &S,
        language: &Language,
        servable: &[Language],
        resources: &[ResourceRef],
    ) -> Result<Self, RepoError> {
        let chain: Vec<Language> = std::iter::once(language.clone())
            .chain(language.without_region())
            .filter(|l| servable.contains(l))
            .collect();
        if resources.is_empty() || chain.is_empty() {
            return Ok(Self::none());
        }
        let loaded = ports.translations().load(resources, &chain).await?;
        let mut translations: HashMap<ResourceRef, Vec<Translation>> = HashMap::new();
        for translation in loaded {
            translations
                .entry(translation.resource())
                .or_default()
                .push(translation);
        }
        let rank = |t: &Translation| chain.iter().position(|l| l == t.language());
        for of_resource in translations.values_mut() {
            of_resource.sort_by_key(rank);
        }
        Ok(Self { translations })
    }

    /// The translation of `resource`'s field `key`: the exact language's, else the
    /// language's without its region, else `None`. Ask only for fields the resource
    /// has: a translation may still hold the field of a child that's gone.
    pub fn text(&self, resource: ResourceRef, key: &str) -> Option<&str> {
        self.translations
            .get(&resource)?
            .iter()
            .find_map(|t| t.text(key))
    }
}
