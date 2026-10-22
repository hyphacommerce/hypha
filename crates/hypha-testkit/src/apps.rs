use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use hypha_apps::{App, AppKey, AppRepo, ExtensionEndpoint, ExtensionEndpointRepo, keys};
use hypha_core::{AppId, Recorded, RepoError, Version, Versioned, stamp_insert, stamp_update};

use crate::{InMemoryAppProviderRepo, InMemoryOAuth, InMemoryOutbox};

/// Apps by key, shared with the linked provider repo.
pub(crate) type Apps = Arc<Mutex<BTreeMap<String, App>>>;

/// Apps by key. Keys are strings, so the version rules are spelled out here, like
/// `InMemoryAppProviderRepo`'s. Linked to one (`linked`), deleting an app deletes its rate
/// settings and OAuth tokens too, like the database's `ON DELETE CASCADE`, and joined to
/// custom fields (`joining_fields`), the definitions in its namespace.
#[derive(Default)]
pub struct InMemoryAppRepo {
    pub(crate) apps: Apps,
    /// The providers to delete with their app, when linked.
    providers: Option<InMemoryAppProviderRepo>,
    /// The tokens to delete with their app, when linked.
    oauth: Option<InMemoryOAuth>,
    /// The extension endpoints to delete with their app, when linked.
    endpoints: Option<Endpoints>,
    /// The custom fields whose app namespace goes with its app, when linked.
    custom_fields: Option<crate::InMemoryCustomFields>,
    /// Shared with `InMemoryPorts`' other repos: the clock writes are stamped with.
    outbox: InMemoryOutbox,
}

impl InMemoryAppRepo {
    /// Deleting an app deletes the custom field definitions in its namespace and their
    /// values, as the database's foreign key does.
    pub fn joining_fields(self, fields: &crate::InMemoryCustomFields) -> Self {
        Self {
            custom_fields: Some(fields.view()),
            ..self
        }
    }
}

/// An app repo, a provider repo, the OAuth repos and an endpoint repo linked like the
/// database's tables, sharing `outbox`'s clock: what `InMemoryPorts` holds.
pub(crate) fn linked(
    outbox: &InMemoryOutbox,
) -> (
    InMemoryAppRepo,
    InMemoryAppProviderRepo,
    InMemoryOAuth,
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
    let oauth = InMemoryOAuth::linked(&apps);
    let endpoints = InMemoryExtensionEndpointRepo {
        endpoints: Endpoints::default(),
        apps: Some(Arc::clone(&apps)),
    };
    let app_repo = InMemoryAppRepo {
        apps,
        providers: Some(cascade),
        oauth: Some(oauth.clone()),
        endpoints: Some(Arc::clone(&endpoints.endpoints)),
        custom_fields: None,
        outbox: outbox.clone(),
    };
    (app_repo, providers, oauth, endpoints)
}

/// The `APP_ID` rule: another app with the same ID.
pub(crate) fn same_id(apps: &BTreeMap<String, App>, app: &App) -> bool {
    apps.values()
        .any(|other| other.id() == app.id() && other.key() != app.key())
}

impl AppRepo for InMemoryAppRepo {
    async fn get(&self, key: &AppKey) -> Result<Option<App>, RepoError> {
        Ok(self.apps.lock().unwrap().get(key.as_str()).cloned())
    }

    async fn get_by_id(&self, id: AppId) -> Result<Option<App>, RepoError> {
        Ok(self
            .apps
            .lock()
            .unwrap()
            .values()
            .find(|a| a.id() == id)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<App>, RepoError> {
        Ok(self.apps.lock().unwrap().values().cloned().collect())
    }

    async fn insert(&self, app: &mut App) -> Result<(), RepoError> {
        stamp_insert(app, self.outbox.now());
        let mut apps = self.apps.lock().unwrap();
        insert_app(&mut apps, app)?;
        self.outbox.write(app.take_events(), Some(app.version()));
        Ok(())
    }

    async fn update(&self, app: &mut App) -> Result<(), RepoError> {
        stamp_update(app, self.outbox.now());
        let mut apps = self.apps.lock().unwrap();
        update_app(&mut apps, app)?;
        self.outbox.write(app.take_events(), Some(app.version()));
        Ok(())
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
        if let Some(oauth) = &self.oauth {
            oauth.drop_app_tokens(key);
        }
        if let Some(endpoints) = &self.endpoints {
            endpoints.lock().unwrap().remove(key.as_str());
        }
        if let Some(fields) = &self.custom_fields {
            fields.drop_namespace(&hypha_custom_fields::Namespace::of_app(key.clone()));
        }
        Ok(())
    }
}

/// Extension endpoints by app key, then point.
pub(crate) type Endpoints = Arc<Mutex<BTreeMap<String, Vec<ExtensionEndpoint>>>>;

/// Apps' extension endpoints. Linked to an app repo (`InMemoryPorts`), an endpoint needs
/// its app, and goes with it, like the database's foreign key; alone, any app will do.
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

/// `AppRepo::insert` on locked apps (events aside).
pub(crate) fn insert_app(apps: &mut BTreeMap<String, App>, app: &mut App) -> Result<(), RepoError> {
    if apps.contains_key(app.key().as_str()) {
        return Err(RepoError::Duplicate(keys::APP_KEY));
    }
    if same_id(apps, app) {
        return Err(RepoError::Duplicate(keys::APP_ID));
    }
    app.set_version(Version::NEW.next());
    let mut stored = app.clone();
    stored.take_events();
    apps.insert(app.key().as_str().to_string(), stored);
    Ok(())
}

/// `AppRepo::update` on locked apps (events aside).
pub(crate) fn update_app(apps: &mut BTreeMap<String, App>, app: &mut App) -> Result<(), RepoError> {
    match apps.get(app.key().as_str()) {
        Some(stored) if stored.version() == app.version() => {
            app.set_version(app.version().next());
            let mut stored = app.clone();
            stored.take_events();
            apps.insert(app.key().as_str().to_string(), stored);
            Ok(())
        }
        _ => Err(RepoError::StaleVersion),
    }
}
