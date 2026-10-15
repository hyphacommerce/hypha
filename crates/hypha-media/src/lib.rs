//! Media: assets (files and their metadata) and where files live (design note 0013).
//!
//! A leaf domain: it depends on core only. Products and other owners attach assets
//! through `hypha-merchandising`; this crate never knows who uses an asset.

pub mod asset;
pub mod blob;
pub mod commands;
pub mod error;
pub mod mime;
pub mod repo;

pub use asset::{
    Asset, AssetSnapshot, AssetStatus, Checksum, FileFacts, HEAD_BYTES, NewUpload, Visibility,
};
pub use blob::{BlobHead, BlobKey, BlobStore, PresignedUpload, UploadTerms};
pub use commands::{
    ABANDONED_AFTER_SECS, CommandError, CreateUpload, DOWNLOAD_EXPIRES_IN_SECS,
    UPLOAD_EXPIRES_IN_SECS, UpdateAsset, complete_upload, create_upload, delete_asset, get_asset,
    list_assets, sweep_abandoned_uploads, update_asset,
};
pub use error::{BlobError, MediaError, UploadMismatch};
pub use mime::{AssetKind, MimeType};
pub use repo::{AssetRepo, HasAssets};
