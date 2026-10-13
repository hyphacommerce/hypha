use std::{collections::HashMap, sync::Mutex};

use hypha_core::{ProductId, RepoError, TaxClassId};
use hypha_tax::{TaxClass, TaxClassRepo, repo::keys};

use crate::table::{Table, no_unique_keys};

#[derive(Default)]
pub struct InMemoryTaxClassRepo {
    classes: Table<TaxClassId, TaxClass>,
    assignments: Mutex<HashMap<ProductId, TaxClassId>>,
}

impl TaxClassRepo for InMemoryTaxClassRepo {
    async fn get(&self, id: TaxClassId) -> Result<Option<TaxClass>, RepoError> {
        Ok(self.classes.get(&id))
    }

    async fn insert(&self, class: &mut TaxClass) -> Result<(), RepoError> {
        self.classes
            .insert(class.id(), class, keys::TAX_CLASS_ID, &no_unique_keys)
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
