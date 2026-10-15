use hypha_core::{CallbackUrlError, ErrorCode, TextError};
use thiserror::Error;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum AppError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Url(#[from] CallbackUrlError),
    #[error("{0:?} is not an app key: \"app_\" and lowercase letters, digits and underscores")]
    InvalidKey(String),
    #[error("a webhook's timeout is {min} to {max} ms")]
    InvalidTimeout { min: u32, max: u32 },
    #[error("{0:?} is not an event type, \"kind.*\" or \"*\"")]
    InvalidSubscription(String),
    #[error("an app subscribes to at most {max} event patterns")]
    TooManySubscriptions { max: usize },
}

impl ErrorCode for AppError {
    fn code(&self) -> &'static str {
        match self {
            AppError::Text(e) => e.code(),
            AppError::Url(e) => e.code(),
            AppError::InvalidKey(_) => "invalid_app_key",
            AppError::InvalidTimeout { .. } => "invalid_timeout",
            AppError::InvalidSubscription(_) => "invalid_subscription",
            AppError::TooManySubscriptions { .. } => "too_many_subscriptions",
        }
    }
}
