use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use hypha_core::{ProductId, Recorded, RepoError, TaxClassId, Versioned, stamp_insert};
use hypha_tax::{TaxClass, TaxClassRepo, repo::keys};

use crate::{
    InMemoryOutbox,
    table::{Table, no_unique_keys},
};

#[derive(Default)]
pub struct InMemoryTaxClassRepo {
    classes: Table<TaxClassId, TaxClass>,
    assignments: Mutex<HashMap<ProductId, TaxClassId>>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryTaxClassRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The events this repo wrote (into its outbox, shared or not).
    pub fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }

    /// Products' classes, locked, for a batch (`InMemoryPorts::commit`).
    pub(crate) fn lock_assignments(&self) -> MutexGuard<'_, HashMap<ProductId, TaxClassId>> {
        self.assignments.lock().unwrap()
    }
}

impl TaxClassRepo for InMemoryTaxClassRepo {
    async fn get(&self, id: TaxClassId) -> Result<Option<TaxClass>, RepoError> {
        Ok(self.classes.get(&id))
    }

    async fn list_all(&self) -> Result<Vec<TaxClass>, RepoError> {
        Ok(self.classes.all_oldest_first())
    }

    async fn insert(&self, class: &mut TaxClass) -> Result<(), RepoError> {
        stamp_insert(class, self.outbox.now());
        let events = class.take_events();
        self.classes
            .insert(class.id(), class, keys::TAX_CLASS_ID, &no_unique_keys)?;
        self.outbox.write(events, Some(class.version()));
        Ok(())
    }

    async fn assign(
        &self,
        products: &[ProductId],
        class: Option<TaxClassId>,
    ) -> Result<(), RepoError> {
        let mut assignments = self.assignments.lock().unwrap();
        for product in products {
            match class {
                Some(class) => assignments.insert(*product, class),
                None => assignments.remove(product),
            };
        }
        Ok(())
    }

    async fn classes_for(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, TaxClassId>, RepoError> {
        let assignments = self.assignments.lock().unwrap();
        Ok(products
            .iter()
            .filter_map(|p| assignments.get(p).map(|c| (*p, *c)))
            .collect())
    }
}
