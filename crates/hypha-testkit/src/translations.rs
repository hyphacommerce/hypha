use std::{collections::HashMap, sync::Mutex};

use hypha_core::{
    Language, Page, PageRequest, Recorded, RepoError, Versioned, stamp_insert, stamp_update,
};
use hypha_translations::{
    ResourceRef, Translation, TranslationFilter, TranslationKey, TranslationRepo, keys,
};

use crate::{
    InMemoryOutbox,
    table::{insert_row, no_unique_keys, update_row},
};

/// The store's translations.
#[derive(Default)]
pub struct InMemoryTranslations {
    rows: Mutex<HashMap<TranslationKey, Translation>>,
    outbox: InMemoryOutbox,
}

impl InMemoryTranslations {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The translations `keep` keeps, in key order.
    fn sorted(&self, keep: impl Fn(&Translation) -> bool) -> Vec<Translation> {
        let rows = self.rows.lock().unwrap();
        let mut found: Vec<(&TranslationKey, &Translation)> =
            rows.iter().filter(|(_, t)| keep(t)).collect();
        found.sort_by(|a, b| a.0.cmp(b.0));
        found.into_iter().map(|(_, t)| t.clone()).collect()
    }
}

impl TranslationRepo for InMemoryTranslations {
    async fn get(
        &self,
        resource: ResourceRef,
        language: &Language,
    ) -> Result<Option<Translation>, RepoError> {
        let key = TranslationKey {
            resource,
            language: language.clone(),
        };
        Ok(self.rows.lock().unwrap().get(&key).cloned())
    }

    async fn for_resource(&self, resource: ResourceRef) -> Result<Vec<Translation>, RepoError> {
        Ok(self.sorted(|t| t.resource() == resource))
    }

    async fn load(
        &self,
        resources: &[ResourceRef],
        languages: &[Language],
    ) -> Result<Vec<Translation>, RepoError> {
        Ok(self.sorted(|t| resources.contains(&t.resource()) && languages.contains(t.language())))
    }

    async fn list(
        &self,
        filter: &TranslationFilter,
        page: PageRequest<TranslationKey>,
    ) -> Result<Page<Translation>, RepoError> {
        let mut found = self.sorted(|t| {
            filter.matches(t)
                && page
                    .after
                    .as_ref()
                    .is_none_or(|after| TranslationKey::of(t) > *after)
        });
        found.truncate(page.limit.get() as usize + 1);
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn any_in(&self, languages: &[Language]) -> Result<bool, RepoError> {
        let rows = self.rows.lock().unwrap();
        Ok(rows.keys().any(|key| languages.contains(&key.language)))
    }

    async fn insert(&self, translation: &mut Translation) -> Result<(), RepoError> {
        stamp_insert(translation, self.outbox.now());
        let events = translation.take_events();
        insert_row(
            &mut self.rows.lock().unwrap(),
            TranslationKey::of(translation),
            translation,
            keys::TRANSLATION,
            &no_unique_keys,
        )?;
        // No subject version: a resource's languages each count their own.
        self.outbox.write(events, None);
        Ok(())
    }

    async fn update(&self, translation: &mut Translation) -> Result<(), RepoError> {
        stamp_update(translation, self.outbox.now());
        let events = translation.take_events();
        update_row(
            &mut self.rows.lock().unwrap(),
            TranslationKey::of(translation),
            translation,
            &no_unique_keys,
        )?;
        // No subject version: a resource's languages each count their own.
        self.outbox.write(events, None);
        Ok(())
    }

    async fn delete(&self, translation: &mut Translation) -> Result<(), RepoError> {
        let key = TranslationKey::of(translation);
        let mut rows = self.rows.lock().unwrap();
        match rows.get(&key) {
            Some(stored) if stored.version() == translation.version() => {}
            _ => return Err(RepoError::StaleVersion),
        }
        rows.remove(&key);
        self.outbox.write(translation.take_events(), None);
        Ok(())
    }

    async fn delete_resources(&self, resources: &[ResourceRef]) -> Result<(), RepoError> {
        self.rows
            .lock()
            .unwrap()
            .retain(|key, _| !resources.contains(&key.resource));
        Ok(())
    }

    async fn retain_languages(&self, languages: &[Language]) -> Result<(), RepoError> {
        self.rows
            .lock()
            .unwrap()
            .retain(|key, _| languages.contains(&key.language));
        Ok(())
    }
}
