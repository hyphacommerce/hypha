//! A merchant's edit of one template (design note 0028, §2, NT5): stored only once
//! edited, so a store that kept a default gets the build's improved defaults.

use hypha_core::{Language, Stamped, Stamps, Version, Versioned};

use crate::{TemplateError, TemplateKey, TemplatePart, TemplateProblem, render};

/// A template's Jinja sources: the subject line, the HTML body and, optionally, a plain
/// text body (derived from the HTML when absent). The layout has only its HTML.
///
/// Invariants, checked by `new`: within the size caps, a subject and HTML for a
/// message, nothing but HTML for the layout, no blank text (that's `None`), and every
/// source parses. `restore` paths check all but the parsing (see `Template`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSources {
    subject: String,
    html: String,
    text: Option<String>,
}

impl TemplateSources {
    /// Bytes: a subject line is short, and providers cap it near this anyway.
    pub const MAX_SUBJECT_BYTES: usize = 250;
    /// Bytes, for each body: a generous email; Gmail clips messages past ~100 KB.
    pub const MAX_BODY_BYTES: usize = 256 * 1024;

    /// Checked sources for `key`: the rules above, then minijinja's parser
    /// (`template_invalid` with the part and line).
    pub fn new(
        key: TemplateKey,
        subject: String,
        html: String,
        text: Option<String>,
    ) -> Result<Self, TemplateError> {
        let sources = Self::checked(key, subject, html, text)?;
        if let Some(problem) = sources.parse_problem() {
            return Err(TemplateError::Invalid(problem));
        }
        Ok(sources)
    }

    /// The build's own sources (`defaults`), which tests hold to these rules.
    pub(crate) fn trusted(subject: &str, html: &str, text: Option<&str>) -> Self {
        Self {
            subject: subject.to_string(),
            html: html.to_string(),
            text: text.map(str::to_string),
        }
    }

    /// Every rule but parsing. Sources are kept as written (no trimming): whitespace
    /// can matter to the output.
    fn checked(
        key: TemplateKey,
        subject: String,
        html: String,
        text: Option<String>,
    ) -> Result<Self, TemplateError> {
        let text = text.filter(|t| !t.trim().is_empty());
        if key.is_layout() {
            if !subject.trim().is_empty() {
                return Err(TemplateError::NotInLayout(TemplatePart::Subject));
            }
            if text.is_some() {
                return Err(TemplateError::NotInLayout(TemplatePart::Text));
            }
        } else if subject.trim().is_empty() {
            return Err(TemplateError::Missing(TemplatePart::Subject));
        }
        if html.trim().is_empty() {
            return Err(TemplateError::Missing(TemplatePart::Html));
        }
        let too_large = |part, source: &str, max| {
            if source.len() > max {
                Err(TemplateError::TooLarge { part, max })
            } else {
                Ok(())
            }
        };
        too_large(TemplatePart::Subject, &subject, Self::MAX_SUBJECT_BYTES)?;
        too_large(TemplatePart::Html, &html, Self::MAX_BODY_BYTES)?;
        if let Some(text) = &text {
            too_large(TemplatePart::Text, text, Self::MAX_BODY_BYTES)?;
        }
        Ok(Self {
            subject: if key.is_layout() {
                String::new()
            } else {
                subject
            },
            html,
            text,
        })
    }

    /// The first source that doesn't parse, in subject, HTML, text order.
    fn parse_problem(&self) -> Option<TemplateProblem> {
        let parts = [
            (TemplatePart::Subject, Some(self.subject.as_str())),
            (TemplatePart::Html, Some(self.html.as_str())),
            (TemplatePart::Text, self.text.as_deref()),
        ];
        parts.into_iter().find_map(|(part, source)| {
            source
                .filter(|s| !s.is_empty())
                .and_then(|source| render::parse(part, source).err())
        })
    }

    /// Empty for the layout.
    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn html(&self) -> &str {
        &self.html
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }
}

/// What a new stored template is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTemplate {
    pub key: TemplateKey,
    /// Reserved for translations (design note 0028, Q2): `None` is the default
    /// language, the only one used for now.
    pub locale: Option<Language>,
    /// `None`: the build's default sources (a template only turned off).
    pub sources: Option<TemplateSources>,
    pub enabled: bool,
}

/// One stored template (design note 0028, §2): the merchant's sources for a key, or
/// none (the default's) when they only turned it off, and whether it's on.
///
/// Identified by (key, locale); there's no ID. Invariants: `enabled` is `true` for the
/// keys that can't be disabled, and the sources keep their rules. A source that stops
/// parsing after an upgrade of minijinja doesn't fail `restore` (that would make the
/// template unreadable, a 500): it's kept, with the parser's `problem`, so the editor
/// can show the merchant where, and sends fall back to the default meanwhile (Q5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    key: TemplateKey,
    locale: Option<Language>,
    version: Version,
    stamps: Option<Stamps>,
    sources: Option<TemplateSources>,
    enabled: bool,
    /// Why the stored sources don't parse, found when loaded.
    problem: Option<TemplateProblem>,
}

impl Versioned for Template {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Template {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Template {
    pub fn new(new: NewTemplate) -> Result<Self, TemplateError> {
        check_enabled(new.key, new.enabled)?;
        Ok(Self {
            key: new.key,
            locale: new.locale,
            version: Version::NEW,
            stamps: None,
            sources: new.sources,
            enabled: new.enabled,
            problem: None,
        })
    }

    /// Replaces the sources (`None`: back to the default's), which `TemplateSources::new`
    /// checked for this key.
    pub fn set_sources(&mut self, sources: Option<TemplateSources>) {
        self.sources = sources;
        self.problem = None;
    }

    /// Refused (`template_required`) for the keys that can't be disabled.
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), TemplateError> {
        check_enabled(self.key, enabled)?;
        self.enabled = enabled;
        Ok(())
    }

    /// Nothing worth storing: the default's sources, enabled. Commands delete such a
    /// template rather than keep it.
    pub fn is_default(&self) -> bool {
        self.sources.is_none() && self.enabled
    }

    pub fn key(&self) -> TemplateKey {
        self.key
    }

    pub fn locale(&self) -> Option<&Language> {
        self.locale.as_ref()
    }

    /// The merchant's sources; `None` uses the default's.
    pub fn sources(&self) -> Option<&TemplateSources> {
        self.sources.as_ref()
    }

    /// Whether the merchant edited the sources (not only turned it off).
    pub fn customized(&self) -> bool {
        self.sources.is_some()
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Why the stored sources no longer parse; `None` when they do.
    pub fn problem(&self) -> Option<&TemplateProblem> {
        self.problem.as_ref()
    }
}

fn check_enabled(key: TemplateKey, enabled: bool) -> Result<(), TemplateError> {
    if !enabled && !key.can_disable() {
        return Err(TemplateError::Required(key));
    }
    Ok(())
}

/// Plain-data form of a `Template` for storage adapters (design note 0001, 3.4): the
/// sources flattened into columns, `subject` and `html` both set or both `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSnapshot {
    pub key: TemplateKey,
    pub locale: Option<Language>,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub subject: Option<String>,
    pub html: Option<String>,
    pub text: Option<String>,
    pub enabled: bool,
}

impl Template {
    pub fn snapshot(&self) -> TemplateSnapshot {
        let sources = self.sources.as_ref();
        TemplateSnapshot {
            key: self.key,
            locale: self.locale.clone(),
            version: self.version,
            stamps: self.stamps,
            subject: sources.map(|s| s.subject.clone()),
            html: sources.map(|s| s.html.clone()),
            text: sources.and_then(|s| s.text.clone()),
            enabled: self.enabled,
        }
    }

    /// Rebuilds a template from storage, checking every rule and parsing the sources;
    /// a parse failure is kept as `problem` rather than refused (see `Template`).
    pub fn restore(s: TemplateSnapshot) -> Result<Self, TemplateError> {
        check_enabled(s.key, s.enabled)?;
        let sources = match (s.subject, s.html) {
            (Some(subject), Some(html)) => {
                Some(TemplateSources::checked(s.key, subject, html, s.text)?)
            }
            (None, None) if s.text.is_none() => None,
            (_, None) | (None, _) => {
                return Err(TemplateError::Missing(TemplatePart::Html));
            }
        };
        let problem = sources.as_ref().and_then(TemplateSources::parse_problem);
        Ok(Self {
            key: s.key,
            locale: s.locale,
            version: s.version,
            stamps: s.stamps,
            sources,
            enabled: s.enabled,
            problem,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(
        subject: &str,
        html: &str,
        text: Option<&str>,
    ) -> Result<TemplateSources, TemplateError> {
        TemplateSources::new(
            TemplateKey::OrderShipped,
            subject.to_string(),
            html.to_string(),
            text.map(str::to_string),
        )
    }

    fn template(enabled: bool) -> Template {
        Template::new(NewTemplate {
            key: TemplateKey::OrderShipped,
            locale: None,
            sources: Some(sources("Shipped {{ order.number }}", "<p>Hi</p>", None).unwrap()),
            enabled,
        })
        .unwrap()
    }

    #[test]
    fn a_syntax_error_names_its_part_and_line() {
        let result = sources("Ok", "<p>\n{% if x %}\n</p>", None);
        let Err(TemplateError::Invalid(problem)) = result else {
            panic!("expected template_invalid, got {result:?}");
        };
        assert_eq!(problem.part, TemplatePart::Html);
        assert!(problem.line.is_some(), "{problem}");
    }

    #[test]
    fn a_text_body_is_parsed_too() {
        let result = sources("Ok", "<p>Hi</p>", Some("{{ unclosed"));
        assert!(
            matches!(&result, Err(TemplateError::Invalid(p)) if p.part == TemplatePart::Text),
            "{result:?}"
        );
    }

    #[test]
    fn a_message_needs_a_subject_and_html() {
        assert_eq!(
            sources(" ", "<p>Hi</p>", None),
            Err(TemplateError::Missing(TemplatePart::Subject))
        );
        assert_eq!(
            sources("Ok", "\n", None),
            Err(TemplateError::Missing(TemplatePart::Html))
        );
    }

    #[test]
    fn blank_text_is_none() {
        assert_eq!(
            sources("Ok", "<p>Hi</p>", Some("  ")).map(|s| s.text().map(str::to_string)),
            Ok(None)
        );
    }

    #[test]
    fn sizes_are_capped_in_bytes() {
        let subject = "é".repeat(TemplateSources::MAX_SUBJECT_BYTES / 2);
        assert!(sources(&subject, "<p>Hi</p>", None).is_ok());
        assert_eq!(
            sources(&format!("{subject}x"), "<p>Hi</p>", None),
            Err(TemplateError::TooLarge {
                part: TemplatePart::Subject,
                max: TemplateSources::MAX_SUBJECT_BYTES
            })
        );
        let body = "x".repeat(TemplateSources::MAX_BODY_BYTES + 1);
        assert_eq!(
            sources("Ok", "<p>Hi</p>", Some(&body)),
            Err(TemplateError::TooLarge {
                part: TemplatePart::Text,
                max: TemplateSources::MAX_BODY_BYTES
            })
        );
    }

    #[test]
    fn the_layout_has_only_html() {
        let layout = |subject: &str, text: Option<&str>| {
            TemplateSources::new(
                TemplateKey::Layout,
                subject.to_string(),
                "<html>{% block content %}{% endblock %}</html>".to_string(),
                text.map(str::to_string),
            )
        };
        assert!(layout("", None).is_ok());
        assert_eq!(
            layout("Hi", None),
            Err(TemplateError::NotInLayout(TemplatePart::Subject))
        );
        assert_eq!(
            layout("", Some("text")),
            Err(TemplateError::NotInLayout(TemplatePart::Text))
        );
    }

    #[test]
    fn required_templates_stay_enabled() {
        let new = |key, enabled| {
            Template::new(NewTemplate {
                key,
                locale: None,
                sources: None,
                enabled,
            })
        };
        assert_eq!(
            new(TemplateKey::StaffInvite, false),
            Err(TemplateError::Required(TemplateKey::StaffInvite))
        );
        assert_eq!(
            new(TemplateKey::Layout, false),
            Err(TemplateError::Required(TemplateKey::Layout))
        );
        let mut shipped = new(TemplateKey::OrderShipped, false).unwrap();
        assert!(!shipped.enabled());
        assert_eq!(shipped.set_enabled(true), Ok(()));
    }

    #[test]
    fn only_turned_off_or_edited_is_worth_storing() {
        let mut t = template(true);
        assert!(!t.is_default());
        t.set_sources(None);
        assert!(t.is_default());
        t.set_enabled(false).unwrap();
        assert!(!t.is_default());
    }

    #[test]
    fn restore_round_trips() {
        let t = template(false);
        assert_eq!(Template::restore(t.snapshot()), Ok(t));
        let mut bare = template(false);
        bare.set_sources(None);
        assert_eq!(Template::restore(bare.snapshot()), Ok(bare));
    }

    #[test]
    fn restore_keeps_sources_that_stopped_parsing_with_their_problem() {
        let mut s = template(true).snapshot();
        s.html = Some("<p>{% for %}</p>".to_string());
        let restored = Template::restore(s).unwrap();
        assert_eq!(restored.problem().map(|p| p.part), Some(TemplatePart::Html));
        assert_eq!(
            restored.sources().map(TemplateSources::html),
            Some("<p>{% for %}</p>")
        );
    }

    #[test]
    fn restore_refuses_half_a_template() {
        let mut s = template(true).snapshot();
        s.subject = None;
        assert_eq!(
            Template::restore(s),
            Err(TemplateError::Missing(TemplatePart::Html))
        );
    }
}
