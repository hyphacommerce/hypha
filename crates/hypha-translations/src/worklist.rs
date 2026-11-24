//! What's left to translate: each resource's source text as digests, kept beside the
//! translations (`TranslationSourceRepo`), so a list of the resources missing or outdated
//! in a language is one read. The host keeps the digests in step with the resources; the
//! counting rule is here, so every store counts alike.

use std::{fmt, str::FromStr};

use hypha_core::Language;

use crate::{catalog::ResourceRef, source::Source, translation::Translation};

/// One field of a resource that has text to translate, as the worklist remembers it: its
/// key and `source_digest` of its text now.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceDigest {
    pub key: String,
    pub digest: String,
}

/// The fields of `source` that have text, as digests: a field the resource leaves empty
/// can't be translated (`source_empty`), so it counts for nothing.
pub fn source_digests(source: &Source) -> Vec<SourceDigest> {
    let mut digests: Vec<SourceDigest> = source
        .fields
        .iter()
        .filter(|f| !f.text.is_empty())
        .map(|f| SourceDigest {
            key: f.key.clone(),
            digest: f.digest.clone(),
        })
        .collect();
    digests.sort();
    digests.dedup_by(|a, b| a.key == b.key);
    digests
}

/// Where a resource stands in one language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranslationState {
    /// Some field with text has no translation in the language.
    Missing,
    /// Every field with text is translated, and some translation's source changed since.
    Outdated,
    /// Every field with text is translated from the text it has now.
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} isn't a translation state: missing, outdated or complete")]
pub struct UnknownState(pub String);

impl TranslationState {
    pub const ALL: [TranslationState; 3] = [
        TranslationState::Missing,
        TranslationState::Outdated,
        TranslationState::Complete,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TranslationState::Missing => "missing",
            TranslationState::Outdated => "outdated",
            TranslationState::Complete => "complete",
        }
    }
}

impl fmt::Display for TranslationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TranslationState {
    type Err = UnknownState;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        TranslationState::ALL
            .into_iter()
            .find(|state| state.as_str() == s)
            .ok_or_else(|| UnknownState(s.to_string()))
    }
}

/// Where a language's fields without a translation of their own are served from: the
/// language without its region (`de` for `de-AT`), when the store has it. What a reader
/// of the language sees, so counts tell an untranslated field shoppers still read in the
/// base language from one they read in the default language.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Inheritance {
    /// A language without a region, or one whose base isn't among the store's
    /// languages: an untranslated field shows the default language.
    #[default]
    Nothing,
    /// Its base is the store's default language (`en` for `en-GB`): the source text.
    Source,
    /// Its base is another of the store's languages, published or not: that
    /// language's translation, where it has one.
    Translation(Language),
}

impl Inheritance {
    /// What `language` inherits in a store whose content is in `default`, with `others`
    /// beside it.
    pub fn of(language: &Language, default: &Language, others: &[Language]) -> Self {
        match language.without_region() {
            Some(base) if base == *default => Inheritance::Source,
            Some(base) if others.contains(&base) => Inheritance::Translation(base),
            _ => Inheritance::Nothing,
        }
    }

    /// The language whose translations are read beside this one's: `Translation`'s.
    pub fn base(&self) -> Option<&Language> {
        match self {
            Inheritance::Translation(base) => Some(base),
            Inheritance::Nothing | Inheritance::Source => None,
        }
    }

    /// For one resource: what its untranslated fields fall back to, given its
    /// translation into `base()` (`None` without one).
    pub fn fallback<'a>(&self, base: Option<&'a Translation>) -> Fallback<'a> {
        match self {
            Inheritance::Nothing => Fallback::Nothing,
            Inheritance::Source => Fallback::Source,
            Inheritance::Translation(_) => Fallback::Translation(base),
        }
    }
}

/// What one resource's fields without a translation in a language fall back to:
/// `Inheritance` with the base language's translation read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback<'a> {
    Nothing,
    Source,
    /// The base language's translation of the resource; `None` when it has none.
    Translation(Option<&'a Translation>),
}

/// How one language's translation of a resource covers the fields it has text in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldCounts {
    /// The resource's fields with text.
    pub total: u32,
    /// Of those, the ones the language has no translation of: in exactly its tag.
    pub missing: u32,
    /// Of those translated, the ones whose source changed since (the stored
    /// `source_digest` isn't the digest now).
    pub outdated: u32,
    /// Of those missing, the ones served from the language's base (`Inheritance`): its
    /// translation, or the source when the base is the default language. 0 for a
    /// language without a region.
    pub inherited: u32,
}

impl FieldCounts {
    /// `translation`'s coverage of `sources` (a resource's `source_digests`), with what
    /// its untranslated fields fall back to: the rule every store's worklist follows. A
    /// translated field whose key isn't among them (its child is gone, or its source
    /// emptied) counts for nothing.
    pub fn of(
        sources: &[SourceDigest],
        translation: Option<&Translation>,
        fallback: Fallback<'_>,
    ) -> Self {
        let mut counts = Self::default();
        for source in sources {
            counts.total += 1;
            match translation.and_then(|t| t.fields().get(&source.key)) {
                None => {
                    counts.missing += 1;
                    let inherited = match fallback {
                        Fallback::Nothing => false,
                        Fallback::Source => true,
                        Fallback::Translation(base) => {
                            base.is_some_and(|b| b.fields().contains_key(&source.key))
                        }
                    };
                    if inherited {
                        counts.inherited += 1;
                    }
                }
                Some(field) if field.source_digest != source.digest => counts.outdated += 1,
                Some(_) => {}
            }
        }
        counts
    }

    /// The fields translated, outdated or not.
    pub fn translated(&self) -> u32 {
        self.total - self.missing
    }

    /// `missing` while any field is, else `outdated` while any is, else `complete`.
    pub fn state(&self) -> TranslationState {
        if self.missing > 0 {
            TranslationState::Missing
        } else if self.outdated > 0 {
            TranslationState::Outdated
        } else {
            TranslationState::Complete
        }
    }
}

/// Which resources a worklist shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorklistFilter {
    /// Of these kinds only.
    pub kinds: Vec<crate::catalog::ResourceKind>,
    /// Their state in this language.
    pub language: Language,
    /// What its untranslated fields are served from, for `FieldCounts::inherited`.
    pub inherits: Inheritance,
    /// In this state only; `None` is every state.
    pub state: Option<TranslationState>,
}

impl WorklistFilter {
    pub fn keeps(&self, resource: ResourceRef, counts: &FieldCounts) -> bool {
        counts.total > 0
            && self.kinds.contains(&resource.kind)
            && self.state.is_none_or(|state| counts.state() == state)
    }
}

/// A resource on a worklist: how its translation into the filter's language covers it,
/// and that translation when there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    pub resource: ResourceRef,
    pub counts: FieldCounts,
    pub translation: Option<Translation>,
}

/// How far a language's translations cover the store's text, in fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageProgress {
    pub language: Language,
    /// Fields with text, across every resource of the kinds counted.
    pub total: u64,
    /// Of those, the ones translated into the language, outdated or not.
    pub translated: u64,
    /// Of those translated, the ones whose source changed since.
    pub outdated: u64,
    /// Of those not translated into the language, the ones served from its base
    /// (`Inheritance`). 0 for a language without a region.
    pub inherited: u64,
}

/// A language whose progress is counted, and what its untranslated fields inherit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressLanguage {
    pub language: Language,
    pub inherits: Inheritance,
}

impl LanguageProgress {
    pub fn missing(&self) -> u64 {
        self.total.saturating_sub(self.translated)
    }
}

/// How far the store's languages cover its text, and how much of it is yet to be counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationProgress {
    /// Each language asked for, in the order given.
    pub languages: Vec<LanguageProgress>,
    /// Resources of the kinds counted still waiting for their digests to be read
    /// (`TranslationSourceRepo::pending`): while there are any, the totals are short of
    /// the store's text (after an upgrade, until the job has read every resource once).
    pub pending: u64,
}

/// A resource waiting for its digests to be read again (`TranslationSourceRepo::queue`),
/// and when it was queued: `unqueue` takes it off only if it wasn't queued again since.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueuedSource {
    pub resource: ResourceRef,
    pub mark: i64,
}

/// Where a worklist continues: after this resource. `product:<uuid>`, for cursors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkPosition(pub ResourceRef);

impl fmt::Display for WorkPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.0.kind, self.0.id.simple())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a worklist's position")]
pub struct InvalidWorkPosition;

impl FromStr for WorkPosition {
    type Err = InvalidWorkPosition;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, id) = s.split_once(':').ok_or(InvalidWorkPosition)?;
        let kind: crate::catalog::ResourceKind = kind.parse().map_err(|_| InvalidWorkPosition)?;
        let id = uuid::Uuid::parse_str(id).map_err(|_| InvalidWorkPosition)?;
        Ok(Self(ResourceRef::new(kind, id)))
    }
}

/// `[jobs.translation_sources]` in `hypha.config.toml`, defaults here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSyncSettings {
    /// Queued resources read again per call.
    pub batch: u32,
}

impl Default for SourceSyncSettings {
    fn default() -> Self {
        Self { batch: 100 }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        catalog::ResourceKind, source::SourceField, source_digest, translation::FieldInput,
    };

    fn source() -> Source {
        Source::new(vec![
            SourceField::new("title", Some("Mug")),
            SourceField::new("description", None),
            SourceField::new("options.opt_1.name", Some("Colour")),
        ])
    }

    fn translated(fields: &[(&str, &str)]) -> Translation {
        let resource = ResourceRef::new(ResourceKind::Product, uuid::Uuid::from_u128(1));
        let mut t = Translation::new(resource, Language::new("de").unwrap());
        let input: BTreeMap<String, FieldInput> = fields
            .iter()
            .map(|(key, value)| {
                (
                    key.to_string(),
                    FieldInput {
                        value: value.to_string(),
                        source_digest: None,
                    },
                )
            })
            .collect();
        t.replace(&source(), input).unwrap();
        t
    }

    #[test]
    fn an_empty_field_has_nothing_to_translate() {
        let digests = source_digests(&source());
        let keys: Vec<&str> = digests.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(keys, ["options.opt_1.name", "title"]);
        assert_eq!(digests[1].digest, source_digest("Mug"));
    }

    #[test]
    fn untranslated_fields_are_missing() {
        let sources = source_digests(&source());
        let none = FieldCounts::of(&sources, None, Fallback::Nothing);
        assert_eq!(
            none,
            FieldCounts {
                total: 2,
                missing: 2,
                outdated: 0,
                inherited: 0,
            }
        );
        assert_eq!(none.state(), TranslationState::Missing);
        let part = FieldCounts::of(
            &sources,
            Some(&translated(&[("title", "Tasse")])),
            Fallback::Nothing,
        );
        assert_eq!((part.missing, part.translated()), (1, 1));
        assert_eq!(part.state(), TranslationState::Missing);
    }

    #[test]
    fn a_translation_of_text_that_changed_is_outdated() {
        let t = translated(&[("title", "Tasse"), ("options.opt_1.name", "Farbe")]);
        let sources = source_digests(&source());
        assert_eq!(
            FieldCounts::of(&sources, Some(&t), Fallback::Nothing).state(),
            TranslationState::Complete
        );
        let changed = source_digests(&Source::new(vec![
            SourceField::new("title", Some("Cup")),
            SourceField::new("options.opt_1.name", Some("Colour")),
        ]));
        let counts = FieldCounts::of(&changed, Some(&t), Fallback::Nothing);
        assert_eq!((counts.missing, counts.outdated), (0, 1));
        assert_eq!(counts.state(), TranslationState::Outdated);
    }

    #[test]
    fn missing_outranks_outdated() {
        let t = translated(&[("title", "Tasse")]);
        let changed = source_digests(&Source::new(vec![
            SourceField::new("title", Some("Cup")),
            SourceField::new("options.opt_1.name", Some("Colour")),
        ]));
        let counts = FieldCounts::of(&changed, Some(&t), Fallback::Nothing);
        assert_eq!((counts.missing, counts.outdated), (1, 1));
        assert_eq!(counts.state(), TranslationState::Missing);
    }

    #[test]
    fn a_translated_field_the_resource_no_longer_has_counts_for_nothing() {
        let t = translated(&[("title", "Tasse"), ("options.opt_1.name", "Farbe")]);
        let fewer = source_digests(&Source::new(vec![SourceField::new("title", Some("Mug"))]));
        assert_eq!(
            FieldCounts::of(&fewer, Some(&t), Fallback::Nothing),
            FieldCounts {
                total: 1,
                missing: 0,
                outdated: 0,
                inherited: 0,
            }
        );
    }

    #[test]
    fn a_regional_language_inherits_from_its_base_when_the_store_has_it() {
        let lang = |tag: &str| Language::new(tag).unwrap();
        let others = [lang("de"), lang("de-AT"), lang("en-GB"), lang("fr-CA")];
        let of = |tag: &str| Inheritance::of(&lang(tag), &lang("en"), &others);
        assert_eq!(of("de-AT"), Inheritance::Translation(lang("de")));
        assert_eq!(of("en-GB"), Inheritance::Source);
        // `fr` isn't the store's: a Canadian reader falls back to the default.
        assert_eq!(of("fr-CA"), Inheritance::Nothing);
        assert_eq!(of("de"), Inheritance::Nothing);
    }

    #[test]
    fn a_missing_field_is_inherited_where_the_base_translates_it() {
        let sources = source_digests(&source());
        let de = translated(&[("title", "Tasse")]);
        let base = Inheritance::Translation(Language::new("de").unwrap());
        let counts = FieldCounts::of(&sources, None, base.fallback(Some(&de)));
        assert_eq!((counts.missing, counts.inherited), (2, 1));
        // An exact translation isn't inherited, nor anything without a base translation.
        let at = translated(&[("title", "Häferl")]);
        let counts = FieldCounts::of(&sources, Some(&at), base.fallback(Some(&de)));
        assert_eq!((counts.missing, counts.inherited), (1, 0));
        let counts = FieldCounts::of(&sources, None, base.fallback(None));
        assert_eq!((counts.missing, counts.inherited), (2, 0));
        // State is by the exact tag still.
        assert_eq!(counts.state(), TranslationState::Missing);
    }

    #[test]
    fn a_language_based_on_the_default_inherits_every_missing_field() {
        let sources = source_digests(&source());
        let t = translated(&[("title", "Mug")]);
        let counts = FieldCounts::of(&sources, Some(&t), Inheritance::Source.fallback(None));
        assert_eq!((counts.missing, counts.inherited), (1, 1));
    }

    #[test]
    fn a_position_reads_back_from_its_text() {
        let at = WorkPosition(ResourceRef::new(
            ResourceKind::SearchFacet,
            uuid::Uuid::from_u128(7),
        ));
        assert_eq!(at.to_string().parse(), Ok(at));
        assert_eq!("product".parse::<WorkPosition>(), Err(InvalidWorkPosition));
        assert_eq!(
            "product:x".parse::<WorkPosition>(),
            Err(InvalidWorkPosition)
        );
    }

    #[test]
    fn states_read_back_from_their_names() {
        for state in TranslationState::ALL {
            assert_eq!(state.as_str().parse(), Ok(state));
        }
        assert_eq!(
            "done".parse::<TranslationState>(),
            Err(UnknownState("done".to_string()))
        );
    }

    #[test]
    fn a_worklist_keeps_resources_with_text_in_its_state() {
        let resource = ResourceRef::new(ResourceKind::Product, uuid::Uuid::from_u128(1));
        let filter = WorklistFilter {
            kinds: vec![ResourceKind::Product],
            language: Language::new("de").unwrap(),
            inherits: Inheritance::Nothing,
            state: Some(TranslationState::Missing),
        };
        let missing = FieldCounts {
            total: 1,
            missing: 1,
            ..FieldCounts::default()
        };
        assert!(filter.keeps(resource, &missing));
        assert!(!filter.keeps(resource, &FieldCounts::default()));
        let category = ResourceRef::new(ResourceKind::Category, resource.id);
        assert!(!filter.keeps(category, &missing));
        let complete = FieldCounts {
            total: 1,
            ..FieldCounts::default()
        };
        assert!(!filter.keeps(resource, &complete));
    }
}
