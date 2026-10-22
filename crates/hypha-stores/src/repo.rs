use hypha_core::{RepoError, StoreId};

use crate::store::Store;

/// Uniqueness rules a `StoreRepo` enforces, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const STORE_ID: &str = "store_id";
}

/// The stores a deployment knows. With one store per deployment (model A) there is exactly
/// one; with a database per store (model C) this would live in a control plane.
#[allow(async_fn_in_trait)]
pub trait StoreRepo {
    async fn get(&self, id: StoreId) -> Result<Option<Store>, RepoError>;
    async fn list_all(&self) -> Result<Vec<Store>, RepoError>;
    /// Stores a new store (version 1). Enforces `STORE_ID`.
    async fn insert(&self, store: &mut Store) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, store: &mut Store) -> Result<(), RepoError>;
}

/// A borrowed repo is a repo too, so a host can lend out `&InMemoryStoreRepo`.
impl<T: StoreRepo + ?Sized> StoreRepo for &T {
    async fn get(&self, id: StoreId) -> Result<Option<Store>, RepoError> {
        (**self).get(id).await
    }
    async fn list_all(&self) -> Result<Vec<Store>, RepoError> {
        (**self).list_all().await
    }
    async fn insert(&self, store: &mut Store) -> Result<(), RepoError> {
        (**self).insert(store).await
    }
    async fn update(&self, store: &mut Store) -> Result<(), RepoError> {
        (**self).update(store).await
    }
}
