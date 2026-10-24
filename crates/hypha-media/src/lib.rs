//! Media: assets (files and their metadata) and where files live.
//!
//! A leaf domain: it depends on core only. Products and other owners attach assets
//! through `hypha-merchandising`; this crate records who uses an asset (`usage.rs`)
//! without knowing what those owners are.

pub mod asset;
pub mod blob;
pub mod commands;
pub mod error;
pub mod events;
pub mod filter;
pub mod mime;
pub mod repo;
pub mod settings;
pub mod temporary;
pub mod usage;

pub use asset::{
    Asset, AssetSnapshot, AssetStatus, Checksum, FileFacts, HEAD_BYTES, NewUpload, Visibility,
};
pub use blob::{BlobHead, BlobKey, BlobStore, PresignedUpload, UploadTerms};
pub use commands::{
    CommandError, CreateTemporaryUpload, CreateUpload, IN_USE_LISTED, UpdateAsset, complete_upload,
    create_temporary_upload, create_upload, delete_asset, delete_temporary_upload, get_asset,
    list_asset_usages, list_assets, read_temporary_upload, sweep_abandoned_uploads,
    sweep_expired_uploads, update_asset,
};
pub use error::{BlobError, MediaError, UploadMismatch};
pub use events::AssetDeleted;
pub use filter::{AssetFilter, AssetSort};
pub use mime::{AssetKind, MimeType};
pub use repo::{
    AssetRepo, AssetUsageRepo, HasAssetUsages, HasAssets, HasTemporaryUploads, StageAssetUsages,
    TemporaryUploadRepo,
};
pub use settings::MediaSettings;
pub use temporary::{NewTemporaryUpload, TemporaryUpload, TemporaryUploadSnapshot, UploadPurpose};
pub use usage::{AssetUsage, UsageOwner, UsageRole};
