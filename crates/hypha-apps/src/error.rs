use hypha_core::{AppKeyError, CallbackUrlError, ErrorCode, TextError};
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
    /// A point (or a version of it) this build doesn't know (design note 0029, §3).
    #[error("no extension point {point:?} version {version}")]
    UnknownExtensionPoint { point: String, version: u16 },
    #[error("extension point {0:?} is listed twice")]
    DuplicateExtensionPoint(String),
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
            AppError::UnknownExtensionPoint { .. } => "unknown_extension_point",
            AppError::DuplicateExtensionPoint(_) => "duplicate_extension_point",
        }
    }
}
