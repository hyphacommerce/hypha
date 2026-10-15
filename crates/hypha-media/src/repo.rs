use hypha_core::{AssetId, Page, PageRequest, RepoError, Timestamp};

use crate::{Asset, AssetKind};

/// Uniqueness rules an `AssetRepo` enforces, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const ASSET_ID: &str = "asset_id";
}

#[allow(async_fn_in_trait)]
pub trait AssetRepo {
    async fn get(&self, id: AssetId) -> Result<Option<Asset>, RepoError>;
    /// A page of `ready` assets, newest first (by ID; design note 0012), only those of
    /// `kind` when given. Pending and failed assets aren't in the library.
    async fn list(
        &self,
        kind: Option<AssetKind>,
        page: PageRequest<AssetId>,
    ) -> Result<Page<Asset>, RepoError>;
    /// Up to `limit` assets whose upload was abandoned: `pending` ones created before
    /// `before`, and `failed` ones last changed before it. Oldest first (by ID).
    async fn abandoned(&self, before: Timestamp, limit: u32) -> Result<Vec<Asset>, RepoError>;
    /// Stores a new asset at version 1. Fails with `Duplicate(ASSET_ID)`.
    async fn insert(&self, asset: &mut Asset) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, asset: &mut Asset) -> Result<(), RepoError>;
    /// Removes the asset; one that doesn't exist is fine.
    async fn delete(&self, id: AssetId) -> Result<(), RepoError>;
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
