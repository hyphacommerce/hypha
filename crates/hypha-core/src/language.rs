use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A BCP 47 language tag: `en`, `en-GB`, `sr-Latn`, `es-419`.
///
/// A strict subset of the grammar: a language (2 or 3 letters), then optionally a script
/// (4 letters), a region (2 letters or 3 digits) and variants (5 to 8 letters or digits,
/// or 4 starting with a digit). Extensions (`-u-…`) and private use (`-x-…`) aren't
/// accepted; loosening that later is easy, tightening it wouldn't be. Subtags are checked
/// by shape, not against the IANA registry. Stored in canonical case (`sr-Latn-RS`), so
/// two spellings of one tag compare equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Language(String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LanguageError {
    #[error("language tag must be at most {max} characters, got {0}", max = Language::MAX_LEN)]
    TooLong(usize),
    #[error(
        "invalid language tag {0:?}: expected a language, then optionally a script, region \
         and variants, such as \"en\", \"en-GB\" or \"sr-Latn\""
    )]
    Invalid(String),
}

impl crate::ErrorCode for LanguageError {
    fn code(&self) -> &'static str {
        match self {
            LanguageError::TooLong(_) | LanguageError::Invalid(_) => "invalid_language",
        }
    }
}

/// Where the parser is in the tag: each part may appear once, in this order.
#[derive(PartialEq, PartialOrd)]
enum Part {
    Language,
    Script,
    Region,
    Variant,
}

impl Language {
    /// RFC 5646's suggested buffer size for tags without extensions.
    pub const MAX_LEN: usize = 35;

    pub fn new(tag: &str) -> Result<Self, LanguageError> {
        if tag.len() > Self::MAX_LEN {
            return Err(LanguageError::TooLong(tag.len()));
        }
        let invalid = || LanguageError::Invalid(tag.to_string());

        let mut canonical = Vec::new();
        let mut variants: Vec<String> = Vec::new();
        let mut last = None;
        for subtag in tag.split('-') {
            let alpha = !subtag.is_empty() && subtag.bytes().all(|b| b.is_ascii_alphabetic());
            let digits = !subtag.is_empty() && subtag.bytes().all(|b| b.is_ascii_digit());
            let alnum = subtag.bytes().all(|b| b.is_ascii_alphanumeric());
            let (part, text) = match (subtag.len(), &last) {
                (2..=3, None) if alpha => (Part::Language, subtag.to_ascii_lowercase()),
                (4, Some(Part::Language)) if alpha => (Part::Script, title_case(subtag)),
                (2, Some(p)) if alpha && *p < Part::Region => {
                    (Part::Region, subtag.to_ascii_uppercase())
                }
                (3, Some(p)) if digits && *p < Part::Region => (Part::Region, subtag.to_string()),
                (5..=8, Some(_)) if alnum => (Part::Variant, subtag.to_ascii_lowercase()),
                (4, Some(_)) if alnum && subtag.as_bytes()[0].is_ascii_digit() => {
                    (Part::Variant, subtag.to_ascii_lowercase())
                }
                _ => return Err(invalid()),
            };
            if part == Part::Variant {
                // RFC 5646 §2.2.5: a variant appears at most once.
                if variants.contains(&text) {
                    return Err(invalid());
                }
                variants.push(text.clone());
            }
            canonical.push(text);
            last = Some(part);
        }
        Ok(Self(canonical.join("-")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `latn` → `Latn`. Only called on ASCII letters.
fn title_case(subtag: &str) -> String {
    let lower = subtag.to_ascii_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => lower,
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Language {
    type Error = LanguageError;

    fn try_from(tag: String) -> Result<Self, Self::Error> {
        Self::new(&tag)
    }
}

impl From<Language> for String {
    fn from(language: Language) -> String {
        language.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(tag: &str) -> String {
        Language::new(tag).unwrap().as_str().to_string()
    }

    fn invalid(tag: &str) -> Result<Language, LanguageError> {
        Err(LanguageError::Invalid(tag.to_string()))
    }

    #[test]
    fn accepts_a_bare_language() {
        assert_eq!(ok("en"), "en");
        assert_eq!(ok("fil"), "fil");
    }

    #[test]
    fn accepts_script_region_and_both() {
        assert_eq!(ok("en-GB"), "en-GB");
        assert_eq!(ok("sr-Latn"), "sr-Latn");
        assert_eq!(ok("sr-Latn-RS"), "sr-Latn-RS");
        assert_eq!(ok("es-419"), "es-419");
    }

    #[test]
    fn accepts_variants() {
        assert_eq!(ok("de-CH-1901"), "de-CH-1901");
        assert_eq!(ok("sl-rozaj-biske"), "sl-rozaj-biske");
    }

    #[test]
    fn canonicalizes_case() {
        assert_eq!(ok("EN-gb"), "en-GB");
        assert_eq!(ok("sr-latn-rs"), "sr-Latn-RS");
        assert_eq!(Language::new("EN"), Language::new("en"));
    }

    #[test]
    fn rejects_empty_and_empty_subtags() {
        for tag in ["", "-", "en-", "en--GB", "-en"] {
            assert_eq!(Language::new(tag), invalid(tag));
        }
    }

    #[test]
    fn rejects_underscores_and_spaces() {
        for tag in ["en_GB", "en GB", " en"] {
            assert_eq!(Language::new(tag), invalid(tag));
        }
    }

    #[test]
    fn rejects_wrong_language_lengths() {
        for tag in ["e", "engl", "e1"] {
            assert_eq!(Language::new(tag), invalid(tag));
        }
    }

    #[test]
    fn rejects_parts_out_of_order() {
        for tag in ["en-GB-Latn", "en-GB-US", "en-Latn-Cyrl", "1901-en"] {
            assert_eq!(Language::new(tag), invalid(tag));
        }
    }

    #[test]
    fn rejects_extensions_and_private_use() {
        for tag in ["en-u-ca-gregory", "en-x-private", "x-klingon"] {
            assert_eq!(Language::new(tag), invalid(tag));
        }
    }

    #[test]
    fn rejects_a_repeated_variant() {
        assert_eq!(Language::new("sl-rozaj-ROZAJ"), invalid("sl-rozaj-ROZAJ"));
    }

    #[test]
    fn length_boundary() {
        let longest = "en-variant1-variant2-variant3-varia".to_string();
        assert_eq!(longest.len(), Language::MAX_LEN);
        assert_eq!(ok(&longest), longest);
        let too_long = format!("{longest}1");
        assert_eq!(Language::new(&too_long), Err(LanguageError::TooLong(36)));
    }

    #[test]
    fn json_is_the_canonical_tag() {
        let tag: Language = serde_json::from_str("\"sr-latn\"").unwrap();
        assert_eq!(serde_json::to_string(&tag).unwrap(), "\"sr-Latn\"");
        assert!(serde_json::from_str::<Language>("\"en_GB\"").is_err());
    }
}
