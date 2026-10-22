//! Who owns a field: `custom` (the merchant's), `app_<name>` (only that app),
//! `spore_<name>` (only that spore). One spelling everywhere, and a field is addressed as
//! `namespace.key`, `custom.material`.

use std::{fmt, str::FromStr};

use hypha_core::{AppKey, AppKind};
use thiserror::Error;

/// A definition's namespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Namespace {
    /// The merchant's own fields, managed through the definitions API.
    Custom,
    /// An external app's (`app_<name>`), declared in its manifest.
    App(AppKey),
    /// A spore's (`spore_<name>`), registered in its code.
    Spore(AppKey),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NamespaceError {
    #[error("{0:?} is not a namespace: \"custom\", \"app_<name>\" or \"spore_<name>\"")]
    Invalid(String),
}

impl hypha_core::ErrorCode for NamespaceError {
    fn code(&self) -> &'static str {
        match self {
            NamespaceError::Invalid(_) => "invalid_namespace",
        }
    }
}

impl Namespace {
    pub const CUSTOM: &'static str = "custom";

    pub fn parse(text: &str) -> Result<Namespace, NamespaceError> {
        if text == Self::CUSTOM {
            return Ok(Namespace::Custom);
        }
        let key = AppKey::new(text).map_err(|_| NamespaceError::Invalid(text.to_string()))?;
        Ok(Namespace::of_app(key))
    }

    /// The namespace an app or a spore owns.
    pub fn of_app(key: AppKey) -> Namespace {
        match key.kind() {
            AppKind::App => Namespace::App(key),
            AppKind::Spore => Namespace::Spore(key),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Namespace::Custom => Self::CUSTOM,
            Namespace::App(key) | Namespace::Spore(key) => key.as_str(),
        }
    }

    /// The app or spore that manages this namespace's definitions; `None` for `custom`,
    /// whose definitions staff manage through the API.
    pub fn managed_by(&self) -> Option<&AppKey> {
        match self {
            Namespace::Custom => None,
            Namespace::App(key) | Namespace::Spore(key) => Some(key),
        }
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Namespace {
    type Err = NamespaceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Namespace::parse(s)
    }
}

/// A field's key within its namespace: a lowercase letter, then 1 to 63 lowercase
/// letters, digits or underscores (`material`, `erp_id`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyError {
    #[error(
        "{0:?} is not a field key: a lowercase letter, then 1 to 63 lowercase letters, digits or underscores"
    )]
    Invalid(String),
    #[error("{0:?} is not a field: \"namespace.key\", like \"custom.material\"")]
    InvalidFieldKey(String),
}

impl hypha_core::ErrorCode for KeyError {
    fn code(&self) -> &'static str {
        match self {
            KeyError::Invalid(_) | KeyError::InvalidFieldKey(_) => "invalid_custom_field_key",
        }
    }
}

impl Key {
    pub const MAX_LEN: usize = 64;

    pub fn new(text: &str) -> Result<Key, KeyError> {
        let mut chars = text.chars();
        let valid = (2..=Self::MAX_LEN).contains(&text.len())
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !valid {
            return Err(KeyError::Invalid(text.to_string()));
        }
        Ok(Key(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A field's full address, `namespace.key`: how values are written and read
/// (`{"custom.material": "wool"}`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldKey {
    pub namespace: Namespace,
    pub key: Key,
}

impl FieldKey {
    pub fn new(namespace: Namespace, key: Key) -> Self {
        Self { namespace, key }
    }

    /// `namespace.key`, split at the first dot.
    pub fn parse(text: &str) -> Result<FieldKey, KeyError> {
        let invalid = || KeyError::InvalidFieldKey(text.to_string());
        let (namespace, key) = text.split_once('.').ok_or_else(invalid)?;
        Ok(FieldKey {
            namespace: Namespace::parse(namespace).map_err(|_| invalid())?,
            key: Key::new(key).map_err(|_| invalid())?,
        })
    }
}

impl fmt::Display for FieldKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.namespace, self.key)
    }
}

impl FromStr for FieldKey {
    type Err = KeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        FieldKey::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_are_custom_or_an_app_or_spore_key() {
        assert_eq!(Namespace::parse("custom"), Ok(Namespace::Custom));
        let app = Namespace::parse("app_erp").unwrap();
        assert_eq!(app, Namespace::App(AppKey::new("app_erp").unwrap()));
        assert_eq!(app.managed_by().map(AppKey::as_str), Some("app_erp"));
        assert!(matches!(
            Namespace::parse("spore_stripe"),
            Ok(Namespace::Spore(_))
        ));
        for bad in ["Custom", "app_", "shopify", "app-erp", ""] {
            assert_eq!(
                Namespace::parse(bad),
                Err(NamespaceError::Invalid(bad.to_string()))
            );
        }
    }

    #[test]
    fn a_key_starts_with_a_letter() {
        assert!(Key::new("material").is_ok());
        assert_eq!(Key::new("1st"), Err(KeyError::Invalid("1st".to_string())));
        assert_eq!(Key::new("_a"), Err(KeyError::Invalid("_a".to_string())));
    }

    #[test]
    fn a_key_is_two_to_sixty_four_characters() {
        assert_eq!(Key::new("a"), Err(KeyError::Invalid("a".to_string())));
        assert!(Key::new("ab").is_ok());
        assert!(Key::new(&"a".repeat(64)).is_ok());
        let long = "a".repeat(65);
        assert_eq!(Key::new(&long), Err(KeyError::Invalid(long.clone())));
    }

    #[test]
    fn a_key_is_lowercase_letters_digits_and_underscores() {
        for bad in ["Material", "mat-erial", "mat.erial", "mät"] {
            assert_eq!(Key::new(bad), Err(KeyError::Invalid(bad.to_string())));
        }
        assert!(Key::new("erp_id_2").is_ok());
    }

    #[test]
    fn field_keys_split_at_the_first_dot() {
        let key = FieldKey::parse("app_erp.erp_id").unwrap();
        assert_eq!(key.to_string(), "app_erp.erp_id");
        for bad in ["material", "custom.", "custom.a.b", "other.material"] {
            assert_eq!(
                FieldKey::parse(bad),
                Err(KeyError::InvalidFieldKey(bad.to_string()))
            );
        }
    }
}
