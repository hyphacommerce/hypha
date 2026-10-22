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
pub mod usage;

pub use asset::{
    Asset, AssetSnapshot, AssetStatus, Checksum, FileFacts, HEAD_BYTES, NewUpload, Visibility,
};
pub use blob::{BlobHead, BlobKey, BlobStore, PresignedUpload, UploadTerms};
pub use commands::{
    CommandError, CreateUpload, IN_USE_LISTED, UpdateAsset, complete_upload, create_upload,
    delete_asset, get_asset, list_asset_usages, list_assets, sweep_abandoned_uploads, update_asset,
};
pub use error::{BlobError, MediaError, UploadMismatch};
pub use events::AssetDeleted;
pub use filter::{AssetFilter, AssetSort};
pub use mime::{AssetKind, MimeType};
pub use repo::{AssetRepo, AssetUsageRepo, HasAssetUsages, HasAssets, StageAssetUsages};
pub use settings::MediaSettings;
pub use usage::{AssetUsage, UsageOwner, UsageRole};
