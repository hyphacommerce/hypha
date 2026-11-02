use std::fmt;

use thiserror::Error;

/// An app's key: `app_` (an external app) or `spore_` (a spore, built into the API), then
/// lowercase letters, digits and underscores, at most 64 characters. It names the app
/// everywhere: deliveries (`app:<key>`), shipping's rate settings, extension calls, its
/// OAuth client and `Actor::App`. In core because several leaf domains name apps.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AppKey(String);

/// Which kind of app a key names: the same identity, two runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppKind {
    /// An external service, called over HTTP (`app_<name>`).
    App,
    /// Rust compiled into the API, called in-process (`spore_<name>`).
    Spore,
}

impl AppKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AppKind::App => "app",
            AppKind::Spore => "spore",
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            AppKind::App => "app_",
            AppKind::Spore => "spore_",
        }
    }
}

impl fmt::Display for AppKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a string isn't an app key.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppKeyError {
    #[error(
        "{0:?} is not an app key: \"app_\" or \"spore_\" and lowercase letters, digits and underscores"
    )]
    Invalid(String),
    /// Not an external app's key, where only one belongs (creating an app, a manifest).
    #[error("{0:?} is not an app key: \"app_\" and lowercase letters, digits and underscores")]
    InvalidApp(String),
    /// A spore's key where only an external app's belongs (creating an app, a manifest):
    /// spores are built into the API, not registered.
    #[error("{0} is a spore's key: spores are built into the API, apps are \"app_<name>\"")]
    SporeKey(AppKey),
}

impl crate::ErrorCode for AppKeyError {
    fn code(&self) -> &'static str {
        match self {
            AppKeyError::Invalid(_) | AppKeyError::InvalidApp(_) => "invalid_app_key",
            AppKeyError::SporeKey(_) => "spore_key_reserved",
        }
    }
}

impl AppKey {
    pub const MAX_LEN: usize = 64;

    /// An app's or a spore's key.
    pub fn new(key: &str) -> Result<Self, AppKeyError> {
        let valid = key.len() <= Self::MAX_LEN
            && [AppKind::App, AppKind::Spore].into_iter().any(|kind| {
                key.strip_prefix(kind.prefix()).is_some_and(|name| {
                    let mut chars = name.chars();
                    chars.next().is_some_and(|c| c.is_ascii_lowercase())
                        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                })
            });
        if !valid {
            return Err(AppKeyError::Invalid(key.to_string()));
        }
        Ok(Self(key.to_string()))
    }

    /// An external app's key only: what staff create and manifests name.
    pub fn new_app(key: &str) -> Result<Self, AppKeyError> {
        let parsed = Self::new(key).map_err(|_| AppKeyError::InvalidApp(key.to_string()))?;
        match parsed.kind() {
            AppKind::App => Ok(parsed),
            AppKind::Spore => Err(AppKeyError::SporeKey(parsed)),
        }
    }

    /// The spore `name`'s key, `spore_<name>`.
    pub fn of_spore(name: &str) -> Result<Self, AppKeyError> {
        Self::new(&format!("{}{name}", AppKind::Spore.prefix()))
    }

    pub fn kind(&self) -> AppKind {
        if self.0.starts_with(AppKind::Spore.prefix()) {
            AppKind::Spore
        } else {
            AppKind::App
        }
    }

    pub fn is_spore(&self) -> bool {
        self.kind() == AppKind::Spore
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

/// Which extension points apps answer ("Registering"): what a domain asks before naming an
/// app for a flow (the store's search app), without depending on apps. The host answers
/// from the app's registrations. No `Send` bound, like every port.
#[allow(async_fn_in_trait)]
pub trait AppPoints {
    /// Whether `app` is installed, enabled and answers `point` (`search.query`).
    async fn answers(&self, app: &AppKey, point: &str) -> Result<bool, crate::RepoError>;
}

impl<T: AppPoints + ?Sized> AppPoints for &T {
    async fn answers(&self, app: &AppKey, point: &str) -> Result<bool, crate::RepoError> {
        (**self).answers(app, point).await
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
            "spores_erp",
            "app_e-rp",
            "spore_",
            "spore_1erp",
        ] {
            assert_eq!(AppKey::new(bad), Err(AppKeyError::Invalid(bad.to_string())));
        }
    }

    #[test]
    fn spore_keys_are_spore_and_a_lowercase_name() {
        let key = AppKey::new("spore_stripe").unwrap();
        assert_eq!(key.kind(), AppKind::Spore);
        assert_eq!(AppKey::of_spore("stripe"), Ok(key));
        assert_eq!(AppKey::new("app_stripe").unwrap().kind(), AppKind::App);
    }

    #[test]
    fn only_an_app_key_is_an_app_key_where_apps_are_registered() {
        assert!(AppKey::new_app("app_stripe").is_ok());
        // The message offers only what's accepted there.
        assert_eq!(
            AppKey::new_app("erp"),
            Err(AppKeyError::InvalidApp("erp".to_string()))
        );
        assert_eq!(
            AppKey::new_app("spore_stripe"),
            Err(AppKeyError::SporeKey(AppKey::of_spore("stripe").unwrap()))
        );
    }

    #[test]
    fn a_key_longer_than_64_characters_is_refused() {
        let long = format!("app_{}", "a".repeat(AppKey::MAX_LEN - 3));
        assert_eq!(AppKey::new(&long), Err(AppKeyError::Invalid(long.clone())));
        assert!(AppKey::new(&long[..AppKey::MAX_LEN]).is_ok());
    }

    #[test]
    fn only_spore_keys_are_spores() {
        assert!(AppKey::new("spore_stripe").unwrap().is_spore());
        assert!(!AppKey::new("app_stripe").unwrap().is_spore());
    }

    #[test]
    fn an_apps_deliveries_go_to_its_key() {
        assert_eq!(AppKey::new("app_erp").unwrap().destination(), "app:app_erp");
    }
}
