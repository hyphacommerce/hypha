//! The one rule for free text (names, titles, address lines): trimmed, at most
//! `MAX_TEXT_LEN` characters, and an optional field left empty is `None`.

use thiserror::Error;

/// Longest free text, in characters (not bytes): fits every common column and form.
pub const MAX_TEXT_LEN: usize = 255;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TextError {
    #[error("{0} is required")]
    Missing(&'static str),
    #[error("{0} is longer than {MAX_TEXT_LEN} characters")]
    TooLong(&'static str),
}

impl crate::ErrorCode for TextError {
    fn code(&self) -> &'static str {
        match self {
            TextError::Missing(_) => "field_required",
            TextError::TooLong(_) => "field_too_long",
        }
    }
}

/// `value` trimmed; blank is `Missing(field)`.
pub fn required_text(field: &'static str, value: &str) -> Result<String, TextError> {
    optional_text(field, Some(value.to_string()))?.ok_or(TextError::Missing(field))
}

/// `value` trimmed; blank becomes `None`.
pub fn optional_text(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, TextError> {
    let Some(value) = value else { return Ok(None) };
    let value = value.trim();
    if value.chars().count() > MAX_TEXT_LEN {
        return Err(TextError::TooLong(field));
    }
    Ok((!value.is_empty()).then(|| value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_text_is_trimmed_and_not_blank() {
        assert_eq!(required_text("name", "  Zagreb "), Ok("Zagreb".to_string()));
        assert_eq!(
            required_text("name", " \t"),
            Err(TextError::Missing("name"))
        );
    }

    #[test]
    fn optional_text_turns_blank_into_none() {
        assert_eq!(optional_text("phone", Some("  ".to_string())), Ok(None));
        assert_eq!(optional_text("phone", None), Ok(None));
    }

    #[test]
    fn length_counts_characters_up_to_the_max() {
        let max = "ž".repeat(MAX_TEXT_LEN);
        assert_eq!(required_text("name", &max), Ok(max.clone()));
        assert_eq!(
            required_text("name", &format!("{max}ž")),
            Err(TextError::TooLong("name"))
        );
    }
}
