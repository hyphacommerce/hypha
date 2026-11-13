//! The build's templates: written in React Email in `packages/emails`, built to Jinja into
//! `defaults/` (committed; languages other than English in a folder each) and embedded here
//! through the generated index, `generated/defaults.rs`. A store uses them until it edits a
//! template, so improved defaults reach every store that kept them.

use hypha_core::Language;

use crate::{TemplateKey, TemplateSources, generated::defaults as built};

/// One key's default sources, as built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultTemplate {
    /// Empty for the layout.
    pub subject: &'static str,
    pub html: &'static str,
    /// `None` for the layout.
    pub text: Option<&'static str>,
}

impl DefaultTemplate {
    pub fn sources(&self) -> TemplateSources {
        TemplateSources::trusted(self.subject, self.html, self.text)
    }
}

/// The text the API writes into a context itself rather than a template: a cancellation's
/// reason, a payment recorded by hand. From `packages/emails`' `context` catalog, so a
/// language's defaults and these arrive together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextStrings {
    pub cancel_customer: &'static str,
    pub cancel_inventory: &'static str,
    pub cancel_fraud: &'static str,
    pub cancel_declined: &'static str,
    pub cancel_staff: &'static str,
    pub manual_payment: &'static str,
}

impl ContextStrings {
    /// `language`'s: its own, else its tag without the region, else English.
    pub fn of(language: &Language) -> &'static ContextStrings {
        let find = |tag: &str| {
            built::CONTEXT_STRINGS
                .iter()
                .find(|(t, _)| *t == tag)
                .map(|(_, strings)| strings)
        };
        find(language.as_str())
            .or_else(|| language.without_region().and_then(|l| find(l.as_str())))
            .unwrap_or_else(|| Self::english())
    }

    pub fn english() -> &'static ContextStrings {
        // The build writes English first.
        &built::CONTEXT_STRINGS[0].1
    }
}

/// The languages the build has defaults in, English first.
pub fn default_languages() -> impl Iterator<Item = &'static str> {
    built::LANGUAGES.iter().copied()
}

/// The English default of `key`: every key has one, and it's what a chain ends with.
pub fn default_template(key: TemplateKey) -> DefaultTemplate {
    built::english(key)
}

/// The build's default of `key` written in exactly `language` (`en` is English), if it has
/// one: the buyer's emails in each language the build has; the layout, staff and supplier
/// emails in English only.
pub fn default_template_in(key: TemplateKey, language: &Language) -> Option<DefaultTemplate> {
    match language.as_str() {
        "en" => Some(built::english(key)),
        other => built::translated(other, key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_keeps_the_rules_a_saved_template_must() {
        for key in TemplateKey::ALL {
            let d = default_template(key);
            let checked = TemplateSources::new(
                key,
                d.subject.to_string(),
                d.html.to_string(),
                d.text.map(str::to_string),
            );
            assert!(checked.is_ok(), "{key}: {checked:?}");
        }
    }

    #[test]
    fn every_translated_default_keeps_the_rules_and_extends_the_layout() {
        for tag in default_languages() {
            let language = Language::new(tag).unwrap();
            for key in TemplateKey::ALL {
                let Some(d) = default_template_in(key, &language) else {
                    continue;
                };
                let checked = TemplateSources::new(
                    key,
                    d.subject.to_string(),
                    d.html.to_string(),
                    d.text.map(str::to_string),
                );
                assert!(checked.is_ok(), "{tag} {key}: {checked:?}");
                if !key.is_layout() {
                    assert!(
                        d.html.starts_with(r#"{% extends "layout" %}"#),
                        "{tag} {key}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_buyer_email_has_a_serbian_default_and_staff_emails_dont() {
        let serbian = Language::new("sr-Latn").unwrap();
        for key in TemplateKey::ALL {
            let buyer = key.audience() == Some(crate::Audience::Buyer);
            assert_eq!(default_template_in(key, &serbian).is_some(), buyer, "{key}");
        }
        assert_ne!(
            default_template_in(TemplateKey::OrderConfirmation, &serbian),
            Some(default_template(TemplateKey::OrderConfirmation))
        );
    }

    #[test]
    fn context_strings_fall_back_from_the_region_to_english() {
        let of = |tag: &str| ContextStrings::of(&Language::new(tag).unwrap());
        assert_eq!(of("en").cancel_inventory, "Items unavailable");
        assert_eq!(of("sr-Latn-RS").cancel_inventory, "Artikli nisu dostupni");
        assert_eq!(of("ja"), ContextStrings::english());
    }

    #[test]
    fn every_message_extends_the_layout() {
        for key in TemplateKey::ALL.into_iter().filter(|k| !k.is_layout()) {
            assert!(
                default_template(key)
                    .html
                    .trim_start()
                    .starts_with(r#"{% extends "layout" %}"#),
                "{key}"
            );
        }
    }
}
