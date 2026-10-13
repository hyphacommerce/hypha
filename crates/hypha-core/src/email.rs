use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// An email address, trimmed and lowercased (design note 0005, C7).
///
/// Lowercasing makes the comparison case-insensitive, so `Ana@Example.com` and
/// `ana@example.com` are one customer. Validation is deliberately shallow: whether an
/// address really receives mail can only be proven by sending to it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Email(String);

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum EmailError {
    #[error("{0:?} is not a valid email address")]
    Invalid(String),
}

impl crate::ErrorCode for EmailError {
    fn code(&self) -> &'static str {
        match self {
            EmailError::Invalid(_) => "invalid_email",
        }
    }
}

impl Email {
    /// The longest address SMTP allows.
    pub const MAX_LEN: usize = 254;

    pub fn new(s: &str) -> Result<Self, EmailError> {
        let email = s.trim().to_lowercase();
        let invalid = || EmailError::Invalid(s.to_string());
        if email.chars().count() > Self::MAX_LEN
            || email.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(invalid());
        }
        let (local, domain) = email.split_once('@').ok_or_else(invalid)?;
        let domain_ok = domain.contains('.')
            && !domain.contains('@')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && !domain.contains("..");
        if local.is_empty() || !domain_ok {
            return Err(invalid());
        }
        Ok(Self(email))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Email {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Email {
    type Error = EmailError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(&s)
    }
}

impl From<Email> for String {
    fn from(email: Email) -> String {
        email.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(s: &str) -> Result<Email, EmailError> {
        Err(EmailError::Invalid(s.to_string()))
    }

    #[test]
    fn trims_and_lowercases() {
        assert_eq!(
            Email::new("  Ana.Horvat@Example.HR ").map(|e| e.to_string()),
            Ok("ana.horvat@example.hr".to_string())
        );
    }

    #[test]
    fn same_address_in_another_case_is_equal() {
        assert_eq!(Email::new("ANA@x.hr"), Email::new("ana@X.hr"));
    }

    #[test]
    fn needs_one_at_with_text_before_it() {
        assert_eq!(Email::new("ana.example.hr"), invalid("ana.example.hr"));
        assert_eq!(Email::new("@example.hr"), invalid("@example.hr"));
        assert_eq!(Email::new("a@b@example.hr"), invalid("a@b@example.hr"));
    }

    #[test]
    fn needs_a_dotted_domain() {
        for s in [
            "ana@localhost",
            "ana@.hr",
            "ana@example.",
            "ana@ex..hr",
            "ana@",
        ] {
            assert_eq!(Email::new(s), invalid(s), "{s}");
        }
    }

    #[test]
    fn rejects_inner_whitespace() {
        assert_eq!(Email::new("ana h@x.hr"), invalid("ana h@x.hr"));
    }

    #[test]
    fn accepts_max_length_and_rejects_one_more() {
        let domain = "@example.hr";
        let local = "a".repeat(Email::MAX_LEN - domain.len());
        assert!(Email::new(&format!("{local}{domain}")).is_ok());
        let long = format!("a{local}{domain}");
        assert_eq!(Email::new(&long), invalid(&long));
    }

    #[test]
    fn deserializes_through_validation() {
        assert_eq!(
            serde_json::from_str::<Email>("\"Ana@X.hr\"").ok(),
            Email::new("ana@x.hr").ok()
        );
        assert!(serde_json::from_str::<Email>("\"nope\"").is_err());
    }
}
