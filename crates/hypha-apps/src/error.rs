use hypha_core::{AppKey, AppKeyError, CallbackUrlError, ErrorCode, Scope, TextError};

use crate::manifest::ManifestError;
use thiserror::Error;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum AppError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Url(#[from] CallbackUrlError),
    #[error(transparent)]
    Key(#[from] AppKeyError),
    #[error("a webhook's timeout is {min} to {max} ms")]
    InvalidTimeout { min: u32, max: u32 },
    #[error("{0:?} is not an event type, \"kind.*\" or \"*\"")]
    InvalidSubscription(String),
    #[error("an app subscribes to at most {max} event patterns")]
    TooManySubscriptions { max: usize },
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// A manifest stored without its URL or time (or the reverse): only `restore`
    /// returns it.
    #[error("a stored manifest is incomplete")]
    IncompleteManifest,
    #[error("the manifest is app {0}'s")]
    ManifestForOtherApp(AppKey),
    /// Uninstalling or narrowing an app that has no grant (made by hand, or uninstalled).
    #[error("the app is not installed through OAuth")]
    NotInstalled,
    /// Narrowing can only take scopes away: more needs an authorization staff approve.
    #[error("the grant doesn't have {0}; adding scopes takes a new authorization")]
    GrantNotNarrower(Scope),
    /// A point (or a version of it) this build doesn't know (design note 0029, §3).
    #[error("no extension point {point:?} version {version}")]
    UnknownExtensionPoint { point: String, version: u16 },
    #[error("extension point {0:?} is listed twice")]
    DuplicateExtensionPoint(String),
    /// An app installed from its manifest registers its points there (§4.1 step 5);
    /// staff set endpoints only for apps made by hand.
    #[error("app {0} registers its extension points in its manifest")]
    EndpointsFromManifest(AppKey),
}

impl ErrorCode for AppError {
    fn code(&self) -> &'static str {
        match self {
            AppError::Text(e) => e.code(),
            AppError::Url(e) => e.code(),
            AppError::Key(e) => e.code(),
            AppError::InvalidTimeout { .. } => "invalid_timeout",
            AppError::InvalidSubscription(_) => "invalid_subscription",
            AppError::TooManySubscriptions { .. } => "too_many_subscriptions",
            AppError::Manifest(e) => e.code(),
            AppError::IncompleteManifest => "internal_error",
            AppError::ManifestForOtherApp(_) => "manifest_invalid",
            AppError::NotInstalled => "app_not_installed",
            AppError::GrantNotNarrower(_) => "grant_not_narrower",
            AppError::UnknownExtensionPoint { .. } => "unknown_extension_point",
            AppError::DuplicateExtensionPoint(_) => "duplicate_extension_point",
            AppError::EndpointsFromManifest(_) => "endpoints_from_manifest",
        }
    }
}
