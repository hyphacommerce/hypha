use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use hypha_apps::{App, AppKey, AppRepo, keys};
use hypha_core::{RepoError, Version, Versioned, stamp_insert, stamp_update};

use crate::{InMemoryAppProviderRepo, InMemoryOutbox};

/// Apps by key, shared with the linked provider repo.
pub(crate) type Apps = Arc<Mutex<BTreeMap<String, App>>>;

/// Apps by key (design note 0016, section 8). Keys are strings, so the version rules
/// are spelled out here, like `InMemoryAppProviderRepo`'s. Linked to one
/// (`linked`), deleting an app deletes its rate settings too, like the database's
/// `ON DELETE CASCADE`.
#[derive(Default)]
pub struct InMemoryAppRepo {
    apps: Apps,
    /// The providers to delete with their app, when linked.
    providers: Option<InMemoryAppProviderRepo>,
    /// Shared with `InMemoryPorts`' other repos: the clock writes are stamped with.
    outbox: InMemoryOutbox,
}

/// An app repo and a provider repo linked like the database's tables, sharing `outbox`'s
/// clock: what `InMemoryPorts` holds.
pub(crate) fn linked(outbox: &InMemoryOutbox) -> (InMemoryAppRepo, InMemoryAppProviderRepo) {
    let apps = Apps::default();
    let providers = InMemoryAppProviderRepo {
        apps: Some(Arc::clone(&apps)),
        outbox: outbox.clone(),
        ..InMemoryAppProviderRepo::default()
    };
    // A view of the same provider map, for the cascade.
    let cascade = InMemoryAppProviderRepo {
        providers: Arc::clone(&providers.providers),
        apps: None,
        outbox: outbox.clone(),
    };
    let app_repo = InMemoryAppRepo {
        apps,
        providers: Some(cascade),
        outbox: outbox.clone(),
    };
    (app_repo, providers)
}

impl AppRepo for InMemoryAppRepo {
    async fn get(&self, key: &AppKey) -> Result<Option<App>, RepoError> {
        Ok(self.apps.lock().unwrap().get(key.as_str()).cloned())
    }

    async fn list(&self) -> Result<Vec<App>, RepoError> {
        Ok(self.apps.lock().unwrap().values().cloned().collect())
    }

    async fn insert(&self, app: &mut App) -> Result<(), RepoError> {
        stamp_insert(app, self.outbox.now());
        let mut apps = self.apps.lock().unwrap();
        if apps.contains_key(app.key().as_str()) {
            return Err(RepoError::Duplicate(keys::APP_KEY));
        }
        app.set_version(Version::NEW.next());
        apps.insert(app.key().as_str().to_string(), app.clone());
        Ok(())
    }

    async fn update(&self, app: &mut App) -> Result<(), RepoError> {
        stamp_update(app, self.outbox.now());
        let mut apps = self.apps.lock().unwrap();
        match apps.get(app.key().as_str()) {
            Some(stored) if stored.version() == app.version() => {
                app.set_version(app.version().next());
                apps.insert(app.key().as_str().to_string(), app.clone());
                Ok(())
            }
            _ => Err(RepoError::StaleVersion),
        }
    }

    async fn delete(&self, key: &AppKey) -> Result<(), RepoError> {
        self.apps.lock().unwrap().remove(key.as_str());
        if let Some(providers) = &self.providers {
            providers
                .providers
                .lock()
                .unwrap()
                .retain(|k, _| k.as_str() != key.as_str());
        }
        Ok(())
    }
}
