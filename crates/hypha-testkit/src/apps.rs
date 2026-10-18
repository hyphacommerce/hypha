use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use hypha_apps::{App, AppKey, AppRepo, ExtensionEndpoint, ExtensionEndpointRepo, keys};
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
    /// The extension endpoints to delete with their app, when linked.
    endpoints: Option<Endpoints>,
    /// Shared with `InMemoryPorts`' other repos: the clock writes are stamped with.
    outbox: InMemoryOutbox,
}

/// An app repo, a provider repo and an endpoint repo linked like the database's tables,
/// sharing `outbox`'s clock: what `InMemoryPorts` holds.
pub(crate) fn linked(
    outbox: &InMemoryOutbox,
) -> (
    InMemoryAppRepo,
    InMemoryAppProviderRepo,
    InMemoryExtensionEndpointRepo,
) {
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
    let endpoints = InMemoryExtensionEndpointRepo {
        endpoints: Endpoints::default(),
        apps: Some(Arc::clone(&apps)),
    };
    let app_repo = InMemoryAppRepo {
        apps,
        providers: Some(cascade),
        endpoints: Some(Arc::clone(&endpoints.endpoints)),
        outbox: outbox.clone(),
    };
    (app_repo, providers, endpoints)
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
        self.outbox.drop_deliveries(&key.destination());
        if let Some(providers) = &self.providers {
            providers
                .providers
                .lock()
                .unwrap()
                .retain(|k, _| k.as_str() != key.as_str());
        }
        if let Some(endpoints) = &self.endpoints {
            endpoints.lock().unwrap().remove(key.as_str());
        }
        Ok(())
    }
}

/// Extension endpoints by app key, then point.
pub(crate) type Endpoints = Arc<Mutex<BTreeMap<String, Vec<ExtensionEndpoint>>>>;

/// Apps' extension endpoints (design note 0029, §3). Linked to an app repo
/// (`InMemoryPorts`), an endpoint needs its app, and goes with it, like the database's
/// foreign key; alone, any app will do.
#[derive(Default)]
pub struct InMemoryExtensionEndpointRepo {
    endpoints: Endpoints,
    apps: Option<Apps>,
}

impl ExtensionEndpointRepo for InMemoryExtensionEndpointRepo {
    async fn list_for_app(&self, app: &AppKey) -> Result<Vec<ExtensionEndpoint>, RepoError> {
        let mut found = self
            .endpoints
            .lock()
            .unwrap()
            .get(app.as_str())
            .cloned()
            .unwrap_or_default();
        found.sort_by(|a, b| a.point.cmp(&b.point));
        Ok(found)
    }

    async fn get(&self, app: &AppKey, point: &str) -> Result<Option<ExtensionEndpoint>, RepoError> {
        Ok(self
            .endpoints
            .lock()
            .unwrap()
            .get(app.as_str())
            .and_then(|all| all.iter().find(|e| e.point == point).cloned()))
    }

    async fn replace_for_app(
        &self,
        app: &AppKey,
        endpoints: &[ExtensionEndpoint],
    ) -> Result<(), RepoError> {
        if let Some(apps) = &self.apps
            && !apps.lock().unwrap().contains_key(app.as_str())
        {
            return Err(RepoError::Reference(
                hypha_apps::endpoints::keys::ENDPOINT_APP,
            ));
        }
        let mut stored = self.endpoints.lock().unwrap();
        if endpoints.is_empty() {
            stored.remove(app.as_str());
        } else {
            stored.insert(app.as_str().to_string(), endpoints.to_vec());
        }
        Ok(())
    }
}
