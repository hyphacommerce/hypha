//! Where files live (design note 0013, section 8): the `BlobStore` port. The S3 adapter
//! (`hypha-store-s3`) serves R2, S3 and MinIO; the testkit has one in memory.
//!
//! Files never pass through Hypha: a client uploads straight to storage with a presigned
//! request (to an `uploads/…` key), and Hypha signs it, checks what was stored, and
//! copies it inside storage to the key it is served from.

use std::fmt;

use hypha_core::Timestamp;

use crate::{BlobError, Checksum, MimeType};

/// Where a file is stored: `assets/…` for public files, `private/…` for the rest.
/// Built by `Asset`, so it is always made of `[a-z0-9._/-]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlobKey(String);

impl BlobKey {
    /// For adapters reading keys back from storage; `Asset` builds new ones.
    pub fn restore(key: String) -> Self {
        Self(key)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BlobKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a presigned upload allows: exactly this type, size and (when given) checksum,
/// so storage refuses anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadTerms {
    pub content_type: MimeType,
    pub content_length: u64,
    pub checksum: Option<Checksum>,
    /// Stored with the object and sent back on every download.
    pub cache_control: &'static str,
    /// How long the upload URL works, in seconds.
    pub expires_in_secs: u32,
}

/// A request the client sends itself: `method` to `url` with exactly `headers`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresignedUpload {
    pub url: String,
    pub method: &'static str,
    pub headers: Vec<(String, String)>,
    pub expires_at: Timestamp,
}

/// What storage knows about a stored file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobHead {
    pub size: u64,
    /// As uploaded (`UploadTerms::content_type`), kept through `copy`.
    pub content_type: Option<String>,
    /// As uploaded (`UploadTerms::cache_control`), kept through `copy`.
    pub cache_control: Option<String>,
}

/// Every call may fail with `BlobError::Backend`. `async fn` without a `Send` bound, like
/// the repositories, so a Workers adapter can implement it.
#[allow(async_fn_in_trait)]
pub trait BlobStore {
    /// A request that uploads one file to `key` under `terms`, valid from `now`.
    async fn presign_upload(
        &self,
        key: &BlobKey,
        terms: &UploadTerms,
        now: Timestamp,
    ) -> Result<PresignedUpload, BlobError>;
    /// The stored file's size, or `None` when nothing is stored at `key`.
    async fn head(&self, key: &BlobKey) -> Result<Option<BlobHead>, BlobError>;
    /// Up to `len` bytes from `start`; fewer when the file is shorter.
    async fn read_range(&self, key: &BlobKey, start: u64, len: u64) -> Result<Vec<u8>, BlobError>;
    /// Copies the file at `from` to `to` inside storage, with its content type and cache
    /// control (no bytes pass through Hypha). Nothing at `from` is `Backend`.
    async fn copy(&self, from: &BlobKey, to: &BlobKey) -> Result<(), BlobError>;
    /// Removes the file; nothing stored at `key` is fine.
    async fn delete(&self, key: &BlobKey) -> Result<(), BlobError>;
    /// A URL that downloads the file until `expires_in_secs` after `now`: for private
    /// files, which have no public URL.
    async fn presign_download(
        &self,
        key: &BlobKey,
        now: Timestamp,
        expires_in_secs: u32,
    ) -> Result<String, BlobError>;
}
