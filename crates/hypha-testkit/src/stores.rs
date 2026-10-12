use hypha_core::{RepoError, StoreId};
use hypha_stores::{Store, StoreRepo, repo::keys};

use crate::table::{Table, no_unique_keys};

#[derive(Default)]
pub struct InMemoryStoreRepo {
    stores: Table<StoreId, Store>,
}

impl StoreRepo for InMemoryStoreRepo {
    async fn get(&self, id: StoreId) -> Result<Option<Store>, RepoError> {
        Ok(self.stores.get(&id))
    }

    async fn list_all(&self) -> Result<Vec<Store>, RepoError> {
        Ok(self.stores.all())
    }

    async fn insert(&self, store: &mut Store) -> Result<(), RepoError> {
        self.stores
            .insert(store.id(), store, keys::STORE_ID, &no_unique_keys)
    }

    async fn update(&self, store: &mut Store) -> Result<(), RepoError> {
        self.stores.update(store.id(), store, &no_unique_keys)
    }
}
