use hypha_core::{ErrorCode, TextError};
use thiserror::Error;

use crate::{AssetKind, AssetStatus, MimeType};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum MediaError {
    #[error(transparent)]
    Text(#[from] TextError),
    /// SVG lands here too until it can be sanitised (design note 0013, M12).
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
    /// A usage's owner kind, owner ID or role that breaks its rule (`usage.rs`).
    #[error("invalid usage {0}")]
    InvalidUsage(String),
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
            MediaError::InvalidUsage(_) => "invalid_usage",
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
