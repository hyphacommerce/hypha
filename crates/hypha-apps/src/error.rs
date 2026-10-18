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
        }
    }
}
