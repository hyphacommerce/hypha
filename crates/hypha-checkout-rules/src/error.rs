use hypha_core::ErrorCode;

/// One broken rule of a checkout rule, where it is (`conditions[0].pattern`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub path: String,
    /// Stable snake_case: `empty`, `too_many`, `too_long`, `too_small`, `duplicate`,
    /// `out_of_range`, `invalid_text`, `invalid_pattern`, `invalid_path`,
    /// `path_not_allowed`, `invalid_code`, `lengths_differ`, `not_ascending`.
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CheckoutRuleError {
    /// Every rule the checkout rule breaks, at once.
    #[error("the checkout rule breaks {} rule(s): {}", .0.len(), .0.iter().map(|p| format!("{}: {}", p.path, p.message)).collect::<Vec<_>>().join("; "))]
    Invalid(Vec<Problem>),
}

impl ErrorCode for CheckoutRuleError {
    fn code(&self) -> &'static str {
        match self {
            CheckoutRuleError::Invalid(_) => "invalid_checkout_rule",
        }
    }
}
