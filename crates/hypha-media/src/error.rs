use hypha_core::{ErrorCode, TextError};
use thiserror::Error;

use crate::{AssetKind, AssetStatus, MimeType};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum MediaError {
    #[error(transparent)]
    Text(#[from] TextError),
    /// SVG lands here too until it can be sanitised.
    #[error("files of type {0:?} aren't accepted")]
    UnsupportedMediaType(String),
    #[error("an empty file can't be uploaded")]
    EmptyFile,
    #[error("{kind} files are at most {max} bytes")]
    FileTooLarge { kind: AssetKind, max: u64 },
    #[error("a SHA-256 checksum is 64 hex characters, not {0:?}")]
    InvalidChecksum(String),
    #[error("the asset is {status}, so its upload can't be completed")]
    NotPending { status: AssetStatus },
    /// An owner kind that isn't a lowercase name (`usage.rs`).
    #[error("invalid usage owner kind {0:?}")]
    InvalidOwnerKind(String),
    /// An owner ID that is empty, too long, or has whitespace or `:`.
    #[error("invalid usage owner ID {0:?}")]
    InvalidOwnerId(String),
    /// A usage role that isn't a lowercase name.
    #[error("invalid usage role {0:?}")]
    InvalidRole(String),
    /// A usage owner's text form (a page cursor) without the `kind:` part.
    #[error("invalid usage owner {0:?}: expected `kind:id`")]
    InvalidOwner(String),
}

impl ErrorCode for MediaError {
    fn code(&self) -> &'static str {
        match self {
            MediaError::Text(e) => e.code(),
            MediaError::UnsupportedMediaType(_) => "unsupported_media_type",
            MediaError::EmptyFile => "empty_file",
            MediaError::FileTooLarge { .. } => "file_too_large",
            MediaError::InvalidChecksum(_) => "invalid_checksum",
            MediaError::NotPending { .. } => "asset_not_pending",
            MediaError::InvalidOwnerKind(_)
            | MediaError::InvalidOwnerId(_)
            | MediaError::InvalidRole(_)
            | MediaError::InvalidOwner(_) => "invalid_usage",
        }
    }
}

/// The stored file isn't what the upload declared. The asset becomes `failed`.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum UploadMismatch {
    #[error("{declared} bytes were declared but {stored} were stored")]
    Size { declared: u64, stored: u64 },
    #[error("the file was declared as {declared} but its contents aren't")]
    Type { declared: MimeType },
}

impl ErrorCode for UploadMismatch {
    fn code(&self) -> &'static str {
        match self {
            UploadMismatch::Size { .. } | UploadMismatch::Type { .. } => "upload_mismatch",
        }
    }
}

/// File storage failed (`BlobStore`). Like `RepoError::Backend`, the message is for
/// logs, not clients.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum BlobError {
    #[error("file storage failed: {0}")]
    Backend(String),
}

impl ErrorCode for BlobError {
    fn code(&self) -> &'static str {
        match self {
            BlobError::Backend(_) => "storage_error",
        }
    }
}
