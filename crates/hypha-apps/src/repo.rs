use hypha_core::{AppId, RepoError};

use crate::{App, AppKey};

/// Uniqueness rules the app repo enforces, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const APP_KEY: &str = "app_key";
    /// An app's `AppId`, unique like its key.
    pub const APP_ID: &str = "app_id";
}

#[allow(async_fn_in_trait)]
pub trait AppRepo {
    async fn get(&self, key: &AppKey) -> Result<Option<App>, RepoError>;
    /// The app acting as `Actor::App(id)`.
    async fn get_by_id(&self, id: AppId) -> Result<Option<App>, RepoError>;
    /// Every app, by key. A store has a handful.
    async fn list(&self) -> Result<Vec<App>, RepoError>;
    /// Stores a new app (version 1), with its grant and events. Enforces `APP_KEY` and
    /// `APP_ID`.
    async fn insert(&self, app: &mut App) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, app: &mut App) -> Result<(), RepoError>;
    /// Removes the app, with what other domains keep under its key in storage they
    /// share with it (shipping's rate settings) and its deliveries in the outbox; one
    /// that doesn't exist is fine.
    async fn delete(&self, key: &AppKey) -> Result<(), RepoError>;
}

/// Stages app writes in a `hypha_core::HasBatch` batch (installing an app writes it with
/// the OAuth request and code). Like the repo's writes, staging takes the app's events.
pub trait StageApps {
    /// What `AppRepo::insert` would write; `APP_KEY` on commit.
    fn insert_app(&mut self, app: &mut App);
    /// What `AppRepo::update` would write; `StaleVersion` on commit if it moved.
    fn update_app(&mut self, app: &mut App);
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
