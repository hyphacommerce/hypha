//! A resource's text in one of the store's other languages: the translated fields, each
//! remembering the source text it translated.

use std::collections::BTreeMap;

use hypha_core::{
    Description, DescriptionError, Language, PendingEvent, Recorded, Recorder, Stamped, Stamps,
    Subject, Version, Versioned,
};

use crate::{
    catalog::{FieldRule, ResourceRef, ValueFormat, ValueRule},
    error::{Problem, TranslationError},
    events::{TRANSLATION, TranslationDeleted, TranslationUpdated},
    source::Source,
};

/// One translated field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedField {
    /// The translation, by the rule of the field it replaces; never empty.
    pub value: String,
    /// The digest of the source text it translates. Another one than the source's now
    /// means the source changed since: the translation is outdated, and still served.
    pub source_digest: String,
}

/// What a write says about one field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInput {
    /// Blank (or HTML with nothing left once sanitised) removes the field.
    pub value: String,
    /// The digest of the source text the translator saw. Given, it must be the
    /// source's now, and the field is recorded as translating it. Left out, a new or
    /// changed value is recorded against the source as it is now, and an unchanged one
    /// keeps what it had (so saving a form again doesn't hide that a field is outdated).
    pub source_digest: Option<String>,
}

/// Identified by its resource and language; there's no ID of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    resource: ResourceRef,
    language: Language,
    version: Version,
    stamps: Option<Stamps>,
    fields: BTreeMap<String, TranslatedField>,
    events: Recorder,
}

impl Translation {
    /// A translation with no fields yet. It's stored once `replace` gave it some.
    pub fn new(resource: ResourceRef, language: Language) -> Self {
        Self {
            resource,
            language,
            version: Version::NEW,
            stamps: None,
            fields: BTreeMap::new(),
            events: Recorder::default(),
        }
    }

    /// Replaces the translated fields with `input`, checked against the resource as it
    /// is now (`source`), every problem reported at once. A field left out or sent
    /// blank is removed, so a key whose child is gone drops out on the next write.
    /// Returns whether anything changed; when it did, `translation.updated` names the
    /// fields.
    pub fn replace(
        &mut self,
        source: &Source,
        input: BTreeMap<String, FieldInput>,
    ) -> Result<bool, TranslationError> {
        let mut problems = Vec::new();
        let mut fields = BTreeMap::new();
        for (key, field) in input {
            match self.check(source, &key, field) {
                Ok(Some(field)) => {
                    fields.insert(key, field);
                }
                Ok(None) => {}
                Err(problem) => problems.push(problem),
            }
        }
        if !problems.is_empty() {
            return Err(TranslationError::Invalid(problems));
        }
        Ok(self.set_fields(fields))
    }

    /// Changes only the fields `input` names, by the same rules as `replace`: a field
    /// given a value is set (blank removes it), `None` removes it, and every field left
    /// out stays as it is. Returns whether anything changed; when it did,
    /// `translation.updated` names the fields.
    pub fn patch(
        &mut self,
        source: &Source,
        input: BTreeMap<String, Option<FieldInput>>,
    ) -> Result<bool, TranslationError> {
        let mut problems = Vec::new();
        let mut fields = self.fields.clone();
        for (key, field) in input {
            let checked = match field {
                Some(field) => self.check(source, &key, field),
                // Removing a field a gone child had is fine; one the kind never has isn't.
                None if self.resource.kind.field(&key).is_none() => Err(self.unknown_field(&key)),
                None => Ok(None),
            };
            match checked {
                Ok(Some(field)) => {
                    fields.insert(key, field);
                }
                Ok(None) => {
                    fields.remove(&key);
                }
                Err(problem) => problems.push(problem),
            }
        }
        if !problems.is_empty() {
            return Err(TranslationError::Invalid(problems));
        }
        Ok(self.set_fields(fields))
    }

    fn unknown_field(&self, key: &str) -> Problem {
        Problem {
            key: key.to_string(),
            code: "unknown_field",
            message: format!("a {} has no such field", self.resource.kind),
        }
    }

    /// One field of a write, checked against the resource as it is now: what to store,
    /// `None` when it's blank (the field is removed), or why not.
    fn check(
        &self,
        source: &Source,
        key: &str,
        field: FieldInput,
    ) -> Result<Option<TranslatedField>, Problem> {
        let problem = |code: &'static str, message: String| Problem {
            key: key.to_string(),
            code,
            message,
        };
        let Some((spec, child)) = self.resource.kind.field(key) else {
            return Err(self.unknown_field(key));
        };
        let now = source.field(key);
        // A field whose rule is the resource's own has none to check by once it's
        // gone: it's unknown, whatever it holds (blank still removes it).
        if now.is_none()
            && matches!(spec.rule, FieldRule::Value(_))
            && !field.value.trim().is_empty()
        {
            return Err(problem(
                "unknown_child",
                "the resource has no such field now".to_string(),
            ));
        }
        let rule = now.map_or(spec.rule, |f| f.rule_or(spec.rule));
        let value = match rule {
            FieldRule::Value(rule) => {
                value_of(rule, &field.value).map_err(|(code, message)| problem(code, message))?
            }
            FieldRule::Text { max_chars } => {
                let value = field.value.trim();
                if value.chars().count() > max_chars {
                    return Err(problem(
                        "too_long",
                        format!("at most {max_chars} characters"),
                    ));
                }
                value.to_string()
            }
            FieldRule::Html { max_bytes } => match Description::new(&field.value) {
                Ok(html) => html.map(|h| h.as_html().to_string()).unwrap_or_default(),
                Err(DescriptionError::TooLong { .. }) => {
                    return Err(problem(
                        "too_long",
                        format!("at most {max_bytes} bytes of HTML"),
                    ));
                }
            },
        };
        if value.is_empty() {
            return Ok(None);
        }
        let Some(now) = now else {
            let code = match child {
                Some(_) => "unknown_child",
                None => "unknown_field",
            };
            return Err(problem(
                code,
                "the resource has no such field now".to_string(),
            ));
        };
        // A translation replaces text the resource has: without a source nothing
        // would ever show it.
        if now.text.is_empty() {
            return Err(problem(
                "source_empty",
                "the resource has no text in this field to translate".to_string(),
            ));
        }
        let source_digest = match (field.source_digest, self.fields.get(key)) {
            (Some(seen), _) if seen != now.digest => {
                return Err(problem(
                    "source_changed",
                    "the source text changed since it was read".to_string(),
                ));
            }
            (None, Some(stored)) if stored.value == value => stored.source_digest.clone(),
            _ => now.digest.clone(),
        };
        Ok(Some(TranslatedField {
            value,
            source_digest,
        }))
    }

    /// Makes `fields` the translation's, recording `translation.updated` naming the keys
    /// added, changed or removed. Returns whether any was.
    fn set_fields(&mut self, fields: BTreeMap<String, TranslatedField>) -> bool {
        let mut changed: Vec<String> = fields
            .iter()
            .filter(|(key, field)| self.fields.get(*key) != Some(field))
            .map(|(key, _)| key.clone())
            .chain(
                self.fields
                    .keys()
                    .filter(|key| !fields.contains_key(*key))
                    .cloned(),
            )
            .collect();
        if changed.is_empty() {
            return false;
        }
        changed.sort_unstable();
        self.fields = fields;
        self.record_updated(changed);
        true
    }

    /// Records `translation.updated` naming `changed`. One event per write: a second
    /// change before the write replaces the first's, naming both's fields.
    fn record_updated(&mut self, changed: Vec<String>) {
        let earlier = self.events.take();
        let mut named: Vec<String> = earlier
            .iter()
            .filter_map(|e| e.data["fields"].as_array())
            .flatten()
            .filter_map(|f| f.as_str().map(str::to_string))
            .chain(changed)
            .collect();
        named.sort_unstable();
        named.dedup();
        self.events.record(
            self.subject(),
            &TranslationUpdated {
                resource: self.resource.into(),
                language: self.language.clone(),
                fields: named,
            },
        );
    }

    /// Removes the fields `keys` name (what's gone from the resource: a custom field
    /// value removed), recording `translation.updated` with those it had. Returns whether
    /// any was there; a translation left empty is for the caller to delete.
    pub fn remove_fields(&mut self, keys: &[String]) -> bool {
        let removed: Vec<String> = keys
            .iter()
            .filter(|key| self.fields.remove(key.as_str()).is_some())
            .cloned()
            .collect();
        if removed.is_empty() {
            return false;
        }
        self.record_updated(removed);
        true
    }

    /// Records `translation.deleted` in place of anything pending, for the repo's delete.
    pub fn mark_deleted(&mut self) {
        self.events.take();
        self.events.record(
            self.subject(),
            &TranslationDeleted {
                resource: self.resource.into(),
                language: self.language.clone(),
            },
        );
    }

    fn subject(&self) -> Subject {
        Subject::new(TRANSLATION, self.resource.id)
    }

    pub fn resource(&self) -> ResourceRef {
        self.resource
    }

    pub fn language(&self) -> &Language {
        &self.language
    }

    /// The translated fields by key. A stored translation has at least one.
    pub fn fields(&self) -> &BTreeMap<String, TranslatedField> {
        &self.fields
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// The translation of `key`, if it has one.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(|f| f.value.as_str())
    }

    pub fn snapshot(&self) -> TranslationSnapshot {
        TranslationSnapshot {
            resource: self.resource,
            language: self.language.clone(),
            version: self.version,
            stamps: self.stamps,
            fields: self.fields.clone(),
        }
    }

    /// A stored translation. Its keys are read by the kind's catalog again; the values
    /// are trusted as `replace` stored them (HTML isn't sanitised twice), and keys of
    /// children that have since gone stay until the next write: readers ask only for
    /// the fields a resource has.
    pub fn restore(s: TranslationSnapshot) -> Result<Self, TranslationError> {
        let problems: Vec<Problem> = s
            .fields
            .iter()
            .filter_map(|(key, field)| {
                let (code, message) = if s.resource.kind.field(key).is_none() {
                    ("unknown_field", "no such field")
                } else if field.value.is_empty() {
                    ("empty", "an empty translation")
                } else {
                    return None;
                };
                Some(Problem {
                    key: key.clone(),
                    code,
                    message: message.to_string(),
                })
            })
            .collect();
        if !problems.is_empty() {
            return Err(TranslationError::Invalid(problems));
        }
        Ok(Self {
            resource: s.resource,
            language: s.language,
            version: s.version,
            stamps: s.stamps,
            fields: s.fields,
            events: Recorder::default(),
        })
    }
}

/// A custom field value's translation as its rule takes it: trimmed (sanitised, for
/// HTML), or why not. Blank is the empty text: the field is removed.
fn value_of(rule: ValueRule, raw: &str) -> Result<String, (&'static str, String)> {
    let too_many_bytes = |text: &str| rule.max_bytes.filter(|max| text.len() > *max);
    if let Some(max) = too_many_bytes(raw.trim()) {
        return Err(("too_long", format!("at most {max} bytes")));
    }
    let value = match rule.format {
        ValueFormat::Line | ValueFormat::Lines => raw.trim().to_string(),
        ValueFormat::Html => match Description::new(raw) {
            Ok(html) => html.map(|h| h.as_html().to_string()).unwrap_or_default(),
            Err(DescriptionError::TooLong { max }) => {
                return Err(("too_long", format!("at most {max} bytes of HTML")));
            }
        },
    };
    if value.is_empty() {
        return Ok(value);
    }
    if rule.format == ValueFormat::Line && value.contains(['\n', '\r']) {
        return Err((
            "not_single_line",
            "one line, without line breaks".to_string(),
        ));
    }
    if let Some(max) = too_many_bytes(&value) {
        return Err(("too_long", format!("at most {max} bytes")));
    }
    let chars = value.chars().count();
    if let Some(max) = rule.max_chars.filter(|max| chars > *max) {
        return Err(("too_long", format!("at most {max} characters")));
    }
    if let Some(min) = rule.min_chars.filter(|min| chars < *min) {
        return Err(("too_short", format!("at least {min} characters")));
    }
    Ok(value)
}

/// Plain-data form for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationSnapshot {
    pub resource: ResourceRef,
    pub language: Language,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub fields: BTreeMap<String, TranslatedField>,
}

impl Recorded for Translation {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for Translation {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Translation {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::{
        catalog::ResourceKind,
        source::{SourceField, source_digest},
    };

    fn product() -> ResourceRef {
        ResourceRef::new(ResourceKind::Product, Uuid::from_u128(1))
    }

    fn source() -> Source {
        Source::new(vec![
            SourceField::new("title", Some("Mug")),
            SourceField::new("description", Some("<p>A mug.</p>")),
            SourceField::new("options.opt_1.name", Some("Colour")),
        ])
    }

    fn german() -> Translation {
        Translation::new(product(), Language::new("de").unwrap())
    }

    fn input(fields: &[(&str, &str)]) -> BTreeMap<String, FieldInput> {
        fields
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
            .collect()
    }

    fn problems(result: Result<bool, TranslationError>) -> Vec<(String, &'static str)> {
        match result {
            Err(TranslationError::Invalid(problems)) => {
                problems.into_iter().map(|p| (p.key, p.code)).collect()
            }
            Ok(_) => Vec::new(),
        }
    }

    #[test]
    fn a_field_is_trimmed_and_remembers_the_sources_digest() {
        let mut t = german();
        assert_eq!(
            t.replace(&source(), input(&[("title", "  Tasse ")])),
            Ok(true)
        );
        assert_eq!(
            t.fields().get("title"),
            Some(&TranslatedField {
                value: "Tasse".to_string(),
                source_digest: source_digest("Mug"),
            })
        );
    }

    #[test]
    fn html_is_sanitised_like_a_description() {
        let mut t = german();
        let html = "<p>Eine Tasse.</p><script>alert(1)</script>";
        assert_eq!(
            t.replace(&source(), input(&[("description", html)])),
            Ok(true)
        );
        assert_eq!(t.text("description"), Some("<p>Eine Tasse.</p>"));
    }

    #[test]
    fn text_is_capped_at_the_fields_limit_in_characters() {
        let mut t = german();
        let max = "ž".repeat(255);
        assert_eq!(t.replace(&source(), input(&[("title", &max)])), Ok(true));
        let over = format!("{max}ž");
        assert_eq!(
            problems(t.replace(&source(), input(&[("title", &over)]))),
            vec![("title".to_string(), "too_long")]
        );
    }

    #[test]
    fn html_over_the_descriptions_cap_is_too_long() {
        let mut t = german();
        let over = "a".repeat(Description::MAX_LEN + 1);
        assert_eq!(
            problems(t.replace(&source(), input(&[("description", &over)]))),
            vec![("description".to_string(), "too_long")]
        );
    }

    #[test]
    fn a_field_the_resource_has_no_text_in_is_refused() {
        let without_description = Source::new(vec![
            SourceField::new("title", Some("Mug")),
            SourceField::new("description", None),
        ]);
        let mut t = german();
        let result = t.replace(
            &without_description,
            input(&[("title", "Tasse"), ("description", "<p>Eine Tasse.</p>")]),
        );
        assert_eq!(
            problems(result),
            vec![("description".to_string(), "source_empty")]
        );
        assert!(t.is_empty());
    }

    #[test]
    fn a_blank_value_for_a_field_without_text_only_removes_it() {
        let without_description = Source::new(vec![
            SourceField::new("title", Some("Mug")),
            SourceField::new("description", None),
        ]);
        let mut t = german();
        assert_eq!(
            t.replace(
                &without_description,
                input(&[("title", "Tasse"), ("description", " ")])
            ),
            Ok(true)
        );
        assert_eq!(t.text("description"), None);
    }

    #[test]
    fn a_stored_translation_of_a_field_whose_text_went_still_loads() {
        let mut t = german();
        t.replace(&source(), input(&[("description", "<p>Eine Tasse.</p>")]))
            .unwrap();
        // The source's description is removed afterwards: nothing checks the row again.
        let restored = Translation::restore(t.snapshot()).unwrap();
        assert_eq!(restored.text("description"), Some("<p>Eine Tasse.</p>"));
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let mut t = german();
        let result = t.replace(
            &source(),
            input(&[("name", "Tasse"), ("options.opt_9.name", "Farbe")]),
        );
        assert_eq!(
            problems(result),
            vec![
                ("name".to_string(), "unknown_field"),
                ("options.opt_9.name".to_string(), "unknown_child"),
            ]
        );
        assert!(t.is_empty());
    }

    #[test]
    fn a_blank_value_removes_the_field_even_for_a_child_that_is_gone() {
        let mut t = german();
        t.replace(&source(), input(&[("title", "Tasse")])).unwrap();
        assert_eq!(
            t.replace(
                &source(),
                input(&[("title", " "), ("options.opt_9.name", "")])
            ),
            Ok(true)
        );
        assert!(t.is_empty());
    }

    #[test]
    fn a_field_left_out_is_removed() {
        let mut t = german();
        t.replace(
            &source(),
            input(&[("title", "Tasse"), ("options.opt_1.name", "Farbe")]),
        )
        .unwrap();
        t.replace(&source(), input(&[("title", "Tasse")])).unwrap();
        assert_eq!(t.fields().keys().collect::<Vec<_>>(), vec!["title"]);
    }

    #[test]
    fn an_unchanged_value_keeps_its_digest_when_the_source_changed() {
        let mut t = german();
        t.replace(&source(), input(&[("title", "Tasse")])).unwrap();
        let changed = Source::new(vec![SourceField::new("title", Some("Cup"))]);
        assert_eq!(t.replace(&changed, input(&[("title", "Tasse")])), Ok(false));
        assert_eq!(t.fields()["title"].source_digest, source_digest("Mug"));
    }

    #[test]
    fn a_changed_value_translates_the_source_as_it_is_now() {
        let mut t = german();
        t.replace(&source(), input(&[("title", "Tasse")])).unwrap();
        let changed = Source::new(vec![SourceField::new("title", Some("Cup"))]);
        assert_eq!(t.replace(&changed, input(&[("title", "Becher")])), Ok(true));
        assert_eq!(t.fields()["title"].source_digest, source_digest("Cup"));
    }

    #[test]
    fn the_digest_sent_confirms_an_unchanged_value() {
        let mut t = german();
        t.replace(&source(), input(&[("title", "Tasse")])).unwrap();
        let changed = Source::new(vec![SourceField::new("title", Some("Cup"))]);
        let mut confirmed = input(&[("title", "Tasse")]);
        confirmed.get_mut("title").unwrap().source_digest = Some(source_digest("Cup"));
        assert_eq!(t.replace(&changed, confirmed), Ok(true));
        assert_eq!(t.fields()["title"].source_digest, source_digest("Cup"));
    }

    #[test]
    fn a_digest_that_isnt_the_sources_now_is_refused() {
        let mut t = german();
        let mut stale = input(&[("title", "Tasse")]);
        stale.get_mut("title").unwrap().source_digest = Some(source_digest("Jug"));
        assert_eq!(
            problems(t.replace(&source(), stale)),
            vec![("title".to_string(), "source_changed")]
        );
    }

    #[test]
    fn a_write_records_one_event_naming_what_changed() {
        let mut t = german();
        t.replace(
            &source(),
            input(&[("title", "Tasse"), ("options.opt_1.name", "Farbe")]),
        )
        .unwrap();
        t.take_events();
        t.replace(
            &source(),
            input(&[("title", "Becher"), ("description", "<p>Eine Tasse.</p>")]),
        )
        .unwrap();
        let events = t.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "translation.updated");
        assert_eq!(events[0].subject, Subject::new("translation", product().id));
        assert_eq!(
            events[0].data,
            serde_json::json!({
                "resource": {"kind": "product", "id": product().id_string()},
                "language": "de",
                "fields": ["description", "options.opt_1.name", "title"],
            })
        );
    }

    #[test]
    fn writing_the_same_fields_records_nothing() {
        let mut t = german();
        t.replace(&source(), input(&[("title", "Tasse")])).unwrap();
        t.take_events();
        assert_eq!(
            t.replace(&source(), input(&[("title", "Tasse")])),
            Ok(false)
        );
        assert!(t.take_events().is_empty());
    }

    #[test]
    fn deleting_records_translation_deleted_alone() {
        let mut t = german();
        t.replace(&source(), input(&[("title", "Tasse")])).unwrap();
        t.mark_deleted();
        let events = t.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "translation.deleted");
        assert_eq!(
            events[0].data,
            serde_json::json!({
                "resource": {"kind": "product", "id": product().id_string()},
                "language": "de",
            })
        );
    }

    #[test]
    fn a_stored_translation_keeps_a_gone_childs_field_but_not_an_unknown_one() {
        let field = TranslatedField {
            value: "Farbe".to_string(),
            source_digest: source_digest("Colour"),
        };
        let snapshot = |key: &str| TranslationSnapshot {
            resource: product(),
            language: Language::new("de").unwrap(),
            version: Version::NEW.next(),
            stamps: None,
            fields: BTreeMap::from([(key.to_string(), field.clone())]),
        };
        assert!(Translation::restore(snapshot("options.opt_9.name")).is_ok());
        assert_eq!(
            Translation::restore(snapshot("name")).map(|_| ()),
            Err(TranslationError::Invalid(vec![Problem {
                key: "name".to_string(),
                code: "unknown_field",
                message: "no such field".to_string(),
            }]))
        );
    }

    const VALUE: &str = "custom_fields.cfdef_1.value";

    fn with_value(rule: ValueRule, text: &str) -> Source {
        Source::new(vec![
            SourceField::new("title", Some("Mug")),
            SourceField::new(VALUE, Some(text)).with_rule(FieldRule::Value(rule)),
        ])
    }

    fn rule(format: ValueFormat) -> ValueRule {
        ValueRule {
            format,
            min_chars: Some(2),
            max_chars: Some(5),
            max_bytes: Some(8),
        }
    }

    #[test]
    fn a_value_takes_its_sources_rule_over_the_catalogs() {
        let source = with_value(rule(ValueFormat::Line), "Wool");
        let mut t = german();
        assert_eq!(t.replace(&source, input(&[(VALUE, " Wolle ")])), Ok(true));
        assert_eq!(t.text(VALUE), Some("Wolle"));
        assert_eq!(
            problems(t.replace(&source, input(&[(VALUE, "Wollen")]))),
            vec![(VALUE.to_string(), "too_long")]
        );
        assert_eq!(
            problems(t.replace(&source, input(&[(VALUE, "W")]))),
            vec![(VALUE.to_string(), "too_short")]
        );
    }

    #[test]
    fn a_one_line_value_has_no_line_breaks_and_a_multiline_one_may() {
        let mut t = german();
        let line = with_value(rule(ValueFormat::Line), "Wool");
        assert_eq!(
            problems(t.replace(&line, input(&[(VALUE, "a\nb")]))),
            vec![(VALUE.to_string(), "not_single_line")]
        );
        let lines = with_value(rule(ValueFormat::Lines), "Wool");
        assert_eq!(t.replace(&lines, input(&[(VALUE, "a\nb")])), Ok(true));
    }

    #[test]
    fn a_value_is_capped_in_bytes_as_well_as_characters() {
        let mut t = german();
        let lines = with_value(rule(ValueFormat::Lines), "Wool");
        // Five characters, ten bytes.
        assert_eq!(
            problems(t.replace(&lines, input(&[(VALUE, "žžžžž")]))),
            vec![(VALUE.to_string(), "too_long")]
        );
    }

    #[test]
    fn a_rich_text_value_is_sanitised_and_counted_as_stored() {
        let mut t = german();
        let html = with_value(
            ValueRule {
                format: ValueFormat::Html,
                min_chars: None,
                max_chars: None,
                max_bytes: Some(64),
            },
            "<p>Wool</p>",
        );
        assert_eq!(
            t.replace(&html, input(&[(VALUE, "<p>Wolle</p><script>x</script>")])),
            Ok(true)
        );
        assert_eq!(t.text(VALUE), Some("<p>Wolle</p>"));
        // Nothing left once sanitised removes the field.
        assert_eq!(
            t.replace(&html, input(&[(VALUE, "<script>x</script>")])),
            Ok(true)
        );
        assert_eq!(t.text(VALUE), None);
    }

    /// A value whose definition stops qualifying (deleted, made private, no longer
    /// shown to storefronts) leaves the source: its stored translation stays, and the
    /// next write of the language drops it, or refuses to take it again.
    #[test]
    fn a_value_gone_from_the_source_drops_out_on_the_next_write() {
        let mut t = german();
        let source = with_value(ValueRule::LINE, "Wool");
        t.replace(&source, input(&[("title", "Tasse"), (VALUE, "Wolle")]))
            .unwrap();
        let without = Source::new(vec![SourceField::new("title", Some("Mug"))]);
        assert_eq!(
            problems(t.replace(&without, input(&[("title", "Tasse"), (VALUE, "Wolle")]))),
            vec![(VALUE.to_string(), "unknown_child")]
        );
        // Whatever it holds: a gone multiline value isn't checked as one line.
        assert_eq!(
            problems(t.replace(&without, input(&[(VALUE, "Zeile 1\nZeile 2")]))),
            vec![(VALUE.to_string(), "unknown_child")]
        );
        assert_eq!(t.text(VALUE), Some("Wolle"));
        assert_eq!(t.replace(&without, input(&[("title", "Tasse")])), Ok(true));
        assert_eq!(t.text(VALUE), None);
    }

    fn patch(fields: &[(&str, Option<&str>)]) -> BTreeMap<String, Option<FieldInput>> {
        fields
            .iter()
            .map(|(key, value)| {
                (
                    key.to_string(),
                    value.map(|value| FieldInput {
                        value: value.to_string(),
                        source_digest: None,
                    }),
                )
            })
            .collect()
    }

    #[test]
    fn a_patch_keeps_the_fields_it_leaves_out() {
        let mut t = german();
        t.replace(
            &source(),
            input(&[("title", "Tasse"), ("options.opt_1.name", "Farbe")]),
        )
        .unwrap();
        t.take_events();
        assert_eq!(
            t.patch(&source(), patch(&[("title", Some("Becher"))])),
            Ok(true)
        );
        assert_eq!(t.text("title"), Some("Becher"));
        assert_eq!(t.text("options.opt_1.name"), Some("Farbe"));
        let events = t.take_events();
        assert_eq!(events[0].data["fields"], serde_json::json!(["title"]));
    }

    #[test]
    fn a_patch_removes_a_field_set_to_none_or_blank() {
        let mut t = german();
        t.replace(
            &source(),
            input(&[
                ("title", "Tasse"),
                ("description", "<p>Eine Tasse.</p>"),
                ("options.opt_1.name", "Farbe"),
            ]),
        )
        .unwrap();
        assert_eq!(
            t.patch(
                &source(),
                patch(&[("title", None), ("description", Some(" "))])
            ),
            Ok(true)
        );
        assert_eq!(
            t.fields().keys().collect::<Vec<_>>(),
            vec!["options.opt_1.name"]
        );
    }

    #[test]
    fn a_patch_checks_what_it_sets_like_a_replace() {
        let mut t = german();
        t.replace(&source(), input(&[("title", "Tasse")])).unwrap();
        let mut stale = patch(&[("options.opt_1.name", Some("Farbe"))]);
        stale
            .get_mut("options.opt_1.name")
            .unwrap()
            .as_mut()
            .unwrap()
            .source_digest = Some(source_digest("Color"));
        stale.insert("name".to_string(), None);
        assert_eq!(
            problems(t.patch(&source(), stale)),
            vec![
                ("name".to_string(), "unknown_field"),
                ("options.opt_1.name".to_string(), "source_changed"),
            ]
        );
        assert_eq!(t.fields().keys().collect::<Vec<_>>(), vec!["title"]);
    }

    #[test]
    fn a_patch_may_remove_a_gone_childs_field() {
        let mut t = german();
        t.replace(&source(), input(&[("options.opt_1.name", "Farbe")]))
            .unwrap();
        let without = Source::new(vec![SourceField::new("title", Some("Mug"))]);
        assert_eq!(
            t.patch(&without, patch(&[("options.opt_1.name", None)])),
            Ok(true)
        );
        assert!(t.is_empty());
    }

    #[test]
    fn a_patch_that_changes_nothing_records_nothing() {
        let mut t = german();
        t.replace(&source(), input(&[("title", "Tasse")])).unwrap();
        t.take_events();
        assert_eq!(
            t.patch(
                &source(),
                patch(&[("title", Some("Tasse")), ("description", None)])
            ),
            Ok(false)
        );
        assert!(t.take_events().is_empty());
    }

    #[test]
    fn removing_fields_names_only_those_it_had() {
        let mut t = german();
        let source = with_value(ValueRule::LINE, "Wool");
        t.replace(&source, input(&[("title", "Tasse"), (VALUE, "Wolle")]))
            .unwrap();
        t.take_events();
        let gone = ["custom_fields.cfdef_9.value".to_string(), VALUE.to_string()];
        assert!(t.remove_fields(&gone));
        assert_eq!(t.text(VALUE), None);
        let events = t.take_events();
        assert_eq!(events[0].data["fields"], serde_json::json!([VALUE]));
        assert!(!t.remove_fields(&gone));
        assert!(t.take_events().is_empty());
    }
}
