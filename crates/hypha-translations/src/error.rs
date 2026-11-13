use hypha_core::ErrorCode;

/// One broken rule of a translation, and the field it's about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The field's key as sent.
    pub key: String,
    /// Stable snake_case: `unknown_field` (the kind has no such field), `unknown_child`
    /// (the option, value, choice or facet the key names isn't the resource's now),
    /// `too_long`, `source_changed` (the digest sent isn't the source's now).
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum TranslationError {
    /// Every rule the fields sent break, at once.
    #[error("the translation breaks {} rule(s): {}", .0.len(), .0.iter().map(|p| format!("{}: {}", p.key, p.message)).collect::<Vec<_>>().join("; "))]
    Invalid(Vec<Problem>),
}

impl ErrorCode for TranslationError {
    fn code(&self) -> &'static str {
        match self {
            TranslationError::Invalid(_) => "invalid_translation",
        }
    }
}
