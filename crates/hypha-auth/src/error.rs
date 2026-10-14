use hypha_core::ErrorCode;
use thiserror::Error;

/// Broken rules of an `Identity`'s credentials.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum IdentityError {
    #[error("two-factor authentication is already on")]
    MfaAlreadyEnabled,
    #[error("two-factor authentication is off")]
    MfaNotEnabled,
    #[error("start setting up the authenticator first")]
    MfaSetupNotStarted,
    #[error("this identity has no password")]
    NoPassword,
}

impl ErrorCode for IdentityError {
    fn code(&self) -> &'static str {
        match self {
            IdentityError::MfaAlreadyEnabled => "mfa_enabled",
            IdentityError::MfaNotEnabled => "mfa_not_enabled",
            IdentityError::MfaSetupNotStarted => "mfa_setup_not_started",
            IdentityError::NoPassword => "no_password",
        }
    }
}

/// Why stored data can't be rebuilt into an aggregate of this crate. Only `restore`
/// returns it and adapters report it as a backend failure, so it has no API code.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RestoreError {
    #[error("unknown credential kind {0:?}")]
    UnknownCredentialKind(String),
    #[error("unreadable credential: {0}")]
    BadCredential(String),
    #[error("more than one {0} credential")]
    DuplicateCredential(&'static str),
    #[error("recovery codes without a confirmed authenticator")]
    RecoveryCodesWithoutMfa,
    #[error(transparent)]
    ApiKey(#[from] ApiKeyError),
    #[error("{0}")]
    Invalid(&'static str),
}

/// Broken rules of an API key.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ApiKeyError {
    #[error(transparent)]
    Text(#[from] hypha_core::TextError),
    #[error("a secret key needs at least one scope")]
    NoScopes,
    #[error("a publishable key always has the storefront scopes; don't send scopes")]
    PublishableScopesFixed,
}

impl ErrorCode for ApiKeyError {
    fn code(&self) -> &'static str {
        match self {
            ApiKeyError::Text(e) => e.code(),
            ApiKeyError::NoScopes => "api_key_scopes_required",
            ApiKeyError::PublishableScopesFixed => "api_key_scopes_fixed",
        }
    }
}
