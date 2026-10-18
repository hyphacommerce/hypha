use hypha_core::{AppKey, AuthError, ErrorCode, RepoError};
use thiserror::Error;

/// Why a call to an app's extension point gave no usable answer. Every kind is
/// `app_unavailable` to clients (502, `details: {app, point}`): which one it was is in
/// the call log (`ExtensionCall::outcome`), for the merchant, not the shopper.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExtensionError {
    /// No answer reached Hypha (refused, unreachable, broken off), or the app answered
    /// with a status other than 2xx.
    #[error("{app} is unavailable for {point}: {reason}")]
    Unavailable {
        app: AppKey,
        point: &'static str,
        reason: String,
    },
    #[error("{app} didn't answer {point} in time")]
    Timeout { app: AppKey, point: &'static str },
    /// A 2xx whose body isn't the point's response (or breaks its rules), or one over
    /// `MAX_ANSWER_BYTES`.
    #[error("{app} answered {point} with something that isn't its response: {reason}")]
    Invalid {
        app: AppKey,
        point: &'static str,
        reason: String,
    },
}

impl ExtensionError {
    /// The app that failed, for `details.app`.
    pub fn app(&self) -> &AppKey {
        match self {
            ExtensionError::Unavailable { app, .. }
            | ExtensionError::Timeout { app, .. }
            | ExtensionError::Invalid { app, .. } => app,
        }
    }

    /// The point it failed at, for `details.point`.
    pub fn point(&self) -> &'static str {
        match self {
            ExtensionError::Unavailable { point, .. }
            | ExtensionError::Timeout { point, .. }
            | ExtensionError::Invalid { point, .. } => point,
        }
    }
}

impl ErrorCode for ExtensionError {
    fn code(&self) -> &'static str {
        match self {
            ExtensionError::Unavailable { .. }
            | ExtensionError::Timeout { .. }
            | ExtensionError::Invalid { .. } => "app_unavailable",
        }
    }
}

/// Why reading the call log failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
        }
    }
}
