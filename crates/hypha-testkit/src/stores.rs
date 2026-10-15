use hypha_core::{RepoError, StoreId, stamp_insert, stamp_update};
use hypha_stores::{Store, StoreRepo, repo::keys};

use crate::{
    InMemoryOutbox,
    table::{Table, no_unique_keys},
};

#[derive(Default)]
pub struct InMemoryStoreRepo {
    stores: Table<StoreId, Store>,
    /// Shared with `InMemoryPorts`' other repos: the clock writes are stamped with.
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryStoreRepo {
    /// A repo sharing `outbox`'s clock (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }
}

impl StoreRepo for InMemoryStoreRepo {
    async fn get(&self, id: StoreId) -> Result<Option<Store>, RepoError> {
        Ok(self.stores.get(&id))
    }

    async fn list_all(&self) -> Result<Vec<Store>, RepoError> {
        Ok(self.stores.all())
    }

    async fn insert(&self, store: &mut Store) -> Result<(), RepoError> {
        stamp_insert(store, self.outbox.now());
        self.stores
            .insert(store.id(), store, keys::STORE_ID, &no_unique_keys)
    }

    async fn update(&self, store: &mut Store) -> Result<(), RepoError> {
        stamp_update(store, self.outbox.now());
        self.stores.update(store.id(), store, &no_unique_keys)
    }
}
