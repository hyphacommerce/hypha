use hypha_core::{AssetId, Keyset, Page, PageRequest, RepoError, Timestamp, UploadId};

use crate::{Asset, AssetFilter, AssetSort, AssetUsage, TemporaryUpload, UsageOwner, UsageRole};

/// Rules the media repositories enforce, reported as `RepoError::Duplicate(key)` or
/// `RepoError::Reference(key)`.
pub mod keys {
    pub const ASSET_ID: &str = "asset_id";
    /// Every usage's asset exists: staging usages of a missing asset, or deleting an asset
    /// that has usages, fails with `Reference(USAGE_ASSET)`.
    pub const USAGE_ASSET: &str = "asset_usage_asset";
    pub const TEMPORARY_UPLOAD_ID: &str = "temporary_upload_id";
}

#[allow(async_fn_in_trait)]
pub trait AssetRepo {
    async fn get(&self, id: AssetId) -> Result<Option<Asset>, RepoError>;
    /// The assets among `ids` that exist, in no particular order, in one query.
    async fn get_many(&self, ids: &[AssetId]) -> Result<Vec<Asset>, RepoError>;
    /// A page of `ready` assets in `sort`'s order: those after `page.after` by
    /// `AssetSort::key` and then ID, only those `filter` matches (`AssetFilter::matches`).
    /// Pending and failed assets aren't in the library. A position whose key doesn't fit
    /// the sort (`AssetSort::fits`) is a `Backend` error.
    async fn list(
        &self,
        filter: &AssetFilter,
        sort: AssetSort,
        page: PageRequest<Keyset<AssetId>>,
    ) -> Result<Page<Asset>, RepoError>;
    /// Up to `limit` assets whose upload was abandoned: `pending` ones created before
    /// `before`, and `failed` ones last changed before it. Oldest first (by ID).
    async fn abandoned(&self, before: Timestamp, limit: u32) -> Result<Vec<Asset>, RepoError>;
    /// Stores a new asset at version 1. Fails with `Duplicate(ASSET_ID)`.
    async fn insert(&self, asset: &mut Asset) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, asset: &mut Asset) -> Result<(), RepoError>;
    /// Removes the asset, with the events it recorded (`Asset::delete`'s
    /// `asset.deleted`) in the same write, as `insert` and `update` write theirs. One that
    /// doesn't exist is fine, and writes no events. Fails with
    /// `Reference(keys::USAGE_ASSET)` while usages point at it, writing nothing.
    async fn delete(&self, asset: &mut Asset) -> Result<(), RepoError>;
}

/// Where assets are used. Written only through `StageAssetUsages`, in the batch that
/// writes the owner.
#[allow(async_fn_in_trait)]
pub trait AssetUsageRepo {
    /// A page of `asset`'s usages, by owner from last to first (kind, then ID: for
    /// products, newest first), below `page.after` when given.
    async fn list_usages(
        &self,
        asset: AssetId,
        page: PageRequest<UsageOwner>,
    ) -> Result<Page<AssetUsage>, RepoError>;
}

/// Stages usage writes in a `hypha_core::HasBatch` batch; the adapter's batch type
/// implements it.
pub trait StageAssetUsages {
    /// Stages making `usages` exactly the usages `owner` has: any others it had are
    /// removed. The caller lists each asset at most once (the commit fails otherwise).
    /// The commit fails with `Reference(keys::USAGE_ASSET)` if one of the assets doesn't
    /// exist.
    fn replace_asset_usages(&mut self, owner: &UsageOwner, usages: &[(AssetId, UsageRole)]);
}

/// Whatever holds the `AssetUsageRepo`.
pub trait HasAssetUsages {
    type AssetUsages: AssetUsageRepo;
    fn asset_usages(&self) -> &Self::AssetUsages;
}

impl<T: HasAssetUsages + ?Sized> HasAssetUsages for &T {
    type AssetUsages = T::AssetUsages;
    fn asset_usages(&self) -> &Self::AssetUsages {
        (**self).asset_usages()
    }
}

/// Whatever holds the `AssetRepo`: a host's per-request ports, a test world.
pub trait HasAssets {
    type Assets: AssetRepo;
    fn assets(&self) -> &Self::Assets;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasAssets + ?Sized> HasAssets for &T {
    type Assets = T::Assets;
    fn assets(&self) -> &Self::Assets {
        (**self).assets()
    }
}

/// Temporary uploads' records; their files live in the `BlobStore`.
#[allow(async_fn_in_trait)]
pub trait TemporaryUploadRepo {
    /// The upload, expired or not.
    async fn get(&self, id: UploadId) -> Result<Option<TemporaryUpload>, RepoError>;
    /// Stores a new upload. Fails with `Duplicate(TEMPORARY_UPLOAD_ID)`.
    async fn insert(&self, upload: &TemporaryUpload) -> Result<(), RepoError>;
    /// Removes it; says whether it was there.
    async fn delete(&self, id: UploadId) -> Result<bool, RepoError>;
    /// Up to `limit` uploads that expired at or before `at`, oldest first (by ID).
    async fn expired(&self, at: Timestamp, limit: u32) -> Result<Vec<TemporaryUpload>, RepoError>;
}

/// Whatever holds the `TemporaryUploadRepo`.
pub trait HasTemporaryUploads {
    type TemporaryUploads: TemporaryUploadRepo;
    fn temporary_uploads(&self) -> &Self::TemporaryUploads;
}

impl<T: HasTemporaryUploads + ?Sized> HasTemporaryUploads for &T {
    type TemporaryUploads = T::TemporaryUploads;
    fn temporary_uploads(&self) -> &Self::TemporaryUploads {
        (**self).temporary_uploads()
    }
}
