use std::{
    collections::HashMap,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
};

use hypha_core::{
    Actor, IdempotencyKey, IdempotencyRecord, IdempotencyRepo, RepoError, Timestamp,
    idempotency::keys,
};

pub(crate) type Records = HashMap<(Actor, IdempotencyKey), IdempotencyRecord>;

/// Idempotency records in memory, keyed like the unique key: one per actor and key.
/// Records are only written through a batch (`InMemoryPorts::commit`).
#[derive(Default)]
pub struct InMemoryIdempotencyRepo {
    records: Mutex<Records>,
    /// How many of the next `get`s find nothing (`hide_next_lookups`).
    hidden: AtomicUsize,
}

impl InMemoryIdempotencyRepo {
    pub(crate) fn lock(&self) -> MutexGuard<'_, Records> {
        self.records.lock().unwrap()
    }

    /// The next `n` lookups find nothing, as if another request with the same key
    /// committed right after: for testing a command that loses that race.
    pub fn hide_next_lookups(&self, n: usize) {
        self.hidden.store(n, Ordering::SeqCst);
    }
}

/// Adds `record`, or fails on `IDEMPOTENCY_KEY` if its actor already used its key.
pub(crate) fn insert_record(
    records: &mut Records,
    record: IdempotencyRecord,
) -> Result<(), RepoError> {
    let id = (record.actor, record.key.clone());
    if records.contains_key(&id) {
        return Err(RepoError::Duplicate(keys::IDEMPOTENCY_KEY));
    }
    records.insert(id, record);
    Ok(())
}

impl IdempotencyRepo for InMemoryIdempotencyRepo {
    async fn get(
        &self,
        actor: Actor,
        key: &IdempotencyKey,
    ) -> Result<Option<IdempotencyRecord>, RepoError> {
        let hiding = self
            .hidden
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if hiding {
            return Ok(None);
        }
        Ok(self.lock().get(&(actor, key.clone())).cloned())
    }

    async fn prune(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError> {
        let mut records = self.lock();
        let old: Vec<_> = records
            .iter()
            .filter(|(_, r)| r.created_at < before)
            .map(|(id, _)| id.clone())
            .take(limit as usize)
            .collect();
        for id in &old {
            records.remove(id);
        }
        Ok(old.len() as u32)
    }
}
