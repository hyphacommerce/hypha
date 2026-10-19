//! Spores' settings in memory (design note 0030, §8).

use std::collections::HashMap;
use std::sync::Mutex;

use hypha_apps::spore_settings::keys;
use hypha_apps::{AppKey, SporeSettings, SporeSettingsRepo};
use hypha_core::{RepoError, stamp_insert, stamp_update};

use crate::{
    InMemoryOutbox,
    table::{insert_row, no_unique_keys, update_row},
};

#[derive(Default)]
pub struct InMemorySporeSettingsRepo {
    rows: Mutex<HashMap<AppKey, SporeSettings>>,
    outbox: InMemoryOutbox,
}

impl InMemorySporeSettingsRepo {
    /// A repo stamping its writes with `outbox`'s clock (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }
}

impl SporeSettingsRepo for InMemorySporeSettingsRepo {
    async fn get(&self, key: &AppKey) -> Result<Option<SporeSettings>, RepoError> {
        Ok(self.rows.lock().unwrap().get(key).cloned())
    }

    async fn insert(&self, settings: &mut SporeSettings) -> Result<(), RepoError> {
        stamp_insert(settings, self.outbox.now());
        let key = settings.key().clone();
        insert_row(
            &mut self.rows.lock().unwrap(),
            key,
            settings,
            keys::SPORE_SETTINGS,
            &no_unique_keys,
        )
    }

    async fn update(&self, settings: &mut SporeSettings) -> Result<(), RepoError> {
        stamp_update(settings, self.outbox.now());
        let key = settings.key().clone();
        update_row(
            &mut self.rows.lock().unwrap(),
            key,
            settings,
            &no_unique_keys,
        )
    }
}
