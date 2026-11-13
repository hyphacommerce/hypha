use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Mutex,
        atomic::{AtomicI64, Ordering},
    },
};

use hypha_core::{
    Language, Page, PageRequest, Recorded, RepoError, Versioned, stamp_insert, stamp_update,
};
use hypha_translations::{
    FieldCounts, LanguageProgress, QueuedSource, ResourceKind, ResourceRef, SourceDigest,
    Translation, TranslationFilter, TranslationKey, TranslationRepo, TranslationSourceRepo,
    WorkItem, WorklistFilter, keys,
};

use crate::{
    InMemoryOutbox,
    table::{insert_row, no_unique_keys, update_row},
};

/// The store's translations.
#[derive(Default)]
pub struct InMemoryTranslations {
    rows: Mutex<HashMap<TranslationKey, Translation>>,
    /// Each resource's digests, by key.
    sources: Mutex<BTreeMap<ResourceRef, Vec<SourceDigest>>>,
    /// The resources waiting to be read again, with their marks.
    queue: Mutex<HashMap<ResourceRef, i64>>,
    marks: AtomicI64,
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

impl InMemoryTranslations {
    fn mark(&self) -> i64 {
        self.marks.fetch_add(1, Ordering::Relaxed) + 1
    }
}

impl TranslationSourceRepo for InMemoryTranslations {
    async fn replace_sources(
        &self,
        resource: ResourceRef,
        digests: &[SourceDigest],
    ) -> Result<(), RepoError> {
        let mut sources = self.sources.lock().unwrap();
        if digests.is_empty() {
            sources.remove(&resource);
        } else {
            let mut digests = digests.to_vec();
            digests.sort();
            sources.insert(resource, digests);
        }
        Ok(())
    }

    async fn sources(
        &self,
        resources: &[ResourceRef],
    ) -> Result<Vec<(ResourceRef, SourceDigest)>, RepoError> {
        let sources = self.sources.lock().unwrap();
        Ok(sources
            .iter()
            .filter(|(resource, _)| resources.contains(resource))
            .flat_map(|(resource, digests)| digests.iter().map(|d| (*resource, d.clone())))
            .collect())
    }

    async fn worklist(
        &self,
        filter: &WorklistFilter,
        page: PageRequest<ResourceRef>,
    ) -> Result<Page<WorkItem>, RepoError> {
        let sources = self.sources.lock().unwrap().clone();
        let rows = self.rows.lock().unwrap();
        let mut found = Vec::new();
        // `BTreeMap` order is `ResourceRef`'s: the catalog's kinds, then the ID.
        for (resource, digests) in sources {
            if page.after.is_some_and(|after| resource <= after) {
                continue;
            }
            let translation = rows
                .get(&TranslationKey {
                    resource,
                    language: filter.language.clone(),
                })
                .cloned();
            let counts = FieldCounts::of(&digests, translation.as_ref());
            if !filter.keeps(resource, &counts) {
                continue;
            }
            found.push(WorkItem {
                resource,
                counts,
                translation,
            });
            if found.len() > page.limit.get() as usize {
                break;
            }
        }
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn progress(
        &self,
        kinds: &[ResourceKind],
        languages: &[Language],
    ) -> Result<Vec<LanguageProgress>, RepoError> {
        let sources = self.sources.lock().unwrap();
        let rows = self.rows.lock().unwrap();
        Ok(languages
            .iter()
            .map(|language| {
                let mut progress = LanguageProgress {
                    language: language.clone(),
                    total: 0,
                    translated: 0,
                    outdated: 0,
                };
                for (resource, digests) in sources.iter().filter(|(r, _)| kinds.contains(&r.kind)) {
                    let translation = rows.get(&TranslationKey {
                        resource: *resource,
                        language: language.clone(),
                    });
                    let counts = FieldCounts::of(digests, translation);
                    progress.total += u64::from(counts.total);
                    progress.translated += u64::from(counts.translated());
                    progress.outdated += u64::from(counts.outdated);
                }
                progress
            })
            .collect())
    }

    async fn queue(&self, resources: &[ResourceRef]) -> Result<(), RepoError> {
        let mut queue = self.queue.lock().unwrap();
        for resource in resources {
            queue.insert(*resource, self.mark());
        }
        Ok(())
    }

    async fn queue_kind(&self, kind: ResourceKind) -> Result<(), RepoError> {
        let resources: Vec<ResourceRef> = self
            .sources
            .lock()
            .unwrap()
            .keys()
            .filter(|r| r.kind == kind)
            .copied()
            .collect();
        self.queue(&resources).await
    }

    async fn queued(&self, limit: u32) -> Result<Vec<QueuedSource>, RepoError> {
        let mut queued: Vec<QueuedSource> = self
            .queue
            .lock()
            .unwrap()
            .iter()
            .map(|(resource, mark)| QueuedSource {
                resource: *resource,
                mark: *mark,
            })
            .collect();
        queued.sort_by_key(|q| q.mark);
        queued.truncate(limit as usize);
        Ok(queued)
    }

    async fn unqueue(&self, done: &[QueuedSource]) -> Result<(), RepoError> {
        let mut queue = self.queue.lock().unwrap();
        for q in done {
            if queue.get(&q.resource) == Some(&q.mark) {
                queue.remove(&q.resource);
            }
        }
        Ok(())
    }
}
