use hypha_core::{ErrorCode, TextError};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ChannelError {
    #[error(transparent)]
    Text(#[from] TextError),
}

impl ErrorCode for ChannelError {
    fn code(&self) -> &'static str {
        match self {
            ChannelError::Text(e) => e.code(),
        }
    }
}
