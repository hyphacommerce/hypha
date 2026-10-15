use hypha_core::RepoError;

use crate::{App, AppKey};

/// Uniqueness rules the app repo enforces, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const APP_KEY: &str = "app_key";
}

#[allow(async_fn_in_trait)]
pub trait AppRepo {
    async fn get(&self, key: &AppKey) -> Result<Option<App>, RepoError>;
    /// Every app, by key. A store has a handful.
    async fn list(&self) -> Result<Vec<App>, RepoError>;
    /// Stores a new app (version 1). Enforces `APP_KEY`.
    async fn insert(&self, app: &mut App) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, app: &mut App) -> Result<(), RepoError>;
    /// Removes the app, with what other domains keep under its key in storage they
    /// share with it (shipping's rate settings) and its deliveries in the outbox; one
    /// that doesn't exist is fine.
    async fn delete(&self, key: &AppKey) -> Result<(), RepoError>;
}

/// Whatever holds the `AppRepo`: a host's per-request ports, a test world.
pub trait HasApps {
    type Apps: AppRepo;
    fn apps(&self) -> &Self::Apps;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasApps + ?Sized> HasApps for &T {
    type Apps = T::Apps;
    fn apps(&self) -> &Self::Apps {
        (**self).apps()
    }
}
