use std::fmt;

use thiserror::Error;

/// An app's key: `app_` and lowercase letters, digits and underscores, at most 64
/// characters. It names the app everywhere: deliveries (`app:<key>`), shipping's rate
/// settings, extension calls (design note 0029), its OAuth client and `Actor::App`. In
/// core because several leaf domains name apps.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AppKey(String);

/// Why a string isn't an app key.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppKeyError {
    #[error("{0:?} is not an app key: \"app_\" and lowercase letters, digits and underscores")]
    Invalid(String),
}

impl crate::ErrorCode for AppKeyError {
    fn code(&self) -> &'static str {
        match self {
            AppKeyError::Invalid(_) => "invalid_app_key",
        }
    }
}

impl AppKey {
    pub const MAX_LEN: usize = 64;

    pub fn new(key: &str) -> Result<Self, AppKeyError> {
        let valid = key.len() <= Self::MAX_LEN
            && key.strip_prefix("app_").is_some_and(|name| {
                let mut chars = name.chars();
                chars.next().is_some_and(|c| c.is_ascii_lowercase())
                    && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            });
        if !valid {
            return Err(AppKeyError::Invalid(key.to_string()));
        }
        Ok(Self(key.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Its webhook deliveries' destination in the outbox: `app:<key>`.
    pub fn destination(&self) -> String {
        format!("app:{}", self.0)
    }
}

impl fmt::Display for AppKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_app_and_a_lowercase_name() {
        assert!(AppKey::new("app_erp_2").is_ok());
        for bad in [
            "erp",
            "app_",
            "app_Erp",
            "app_1erp",
            "spore_erp",
            "app_e-rp",
        ] {
            assert_eq!(AppKey::new(bad), Err(AppKeyError::Invalid(bad.to_string())));
        }
    }

    #[test]
    fn a_key_longer_than_64_characters_is_refused() {
        let long = format!("app_{}", "a".repeat(AppKey::MAX_LEN - 3));
        assert_eq!(AppKey::new(&long), Err(AppKeyError::Invalid(long.clone())));
        assert!(AppKey::new(&long[..AppKey::MAX_LEN]).is_ok());
    }
}
