use std::{
    collections::HashMap,
    hash::Hash,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
};

use hypha_core::{RepoError, Version, Versioned};

/// One in-memory "table" of aggregates with the same write rules a database adapter
/// has: `insert` fails on a duplicate ID or unique key, `update` fails with
/// `StaleVersion` when the stored version moved on. Every in-memory repo is built on
/// this, so command tests see the same behavior as production.
pub(crate) struct Table<K, T> {
    rows: Mutex<HashMap<K, T>>,
    /// How many of the next lookups see no rows (`hide_next_lookups`).
    hidden: AtomicUsize,
}

impl<K, T> Default for Table<K, T> {
    fn default() -> Self {
        Self {
            rows: Mutex::new(HashMap::new()),
            hidden: AtomicUsize::new(0),
        }
    }
}

/// Checks a new or changed row against one *other* stored row and returns the unique
/// key it would break, if any. The row's own previous state is never passed in.
pub(crate) type UniqueCheck<'a, T> = &'a dyn Fn(&T, &T) -> Option<&'static str>;

impl<K: Eq + Hash + Copy, T: Clone + Versioned> Table<K, T> {
    /// The rows, locked, for a batch that writes several tables in one step
    /// (`InMemoryPorts::commit`).
    pub(crate) fn lock(&self) -> MutexGuard<'_, HashMap<K, T>> {
        self.rows.lock().unwrap()
    }

    pub(crate) fn get(&self, id: &K) -> Option<T> {
        self.rows.lock().unwrap().get(id).cloned()
    }

    pub(crate) fn get_many(&self, ids: &[K]) -> Vec<T> {
        let rows = self.rows.lock().unwrap();
        ids.iter().filter_map(|id| rows.get(id).cloned()).collect()
    }

    /// Makes the next `n` lookups by something other than the ID (`find`, `all`) see no
    /// rows, as if another request inserted them right after. Tests use it to lose the
    /// race between a command's pre-check and its write, which the unique keys must
    /// still catch.
    pub(crate) fn hide_next_lookups(&self, n: usize) {
        self.hidden.store(n, Ordering::SeqCst);
    }

    /// Whether this lookup is one of the hidden ones (and uses it up).
    fn hiding(&self) -> bool {
        self.hidden
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
    }

    pub(crate) fn find(&self, matches: impl Fn(&T) -> bool) -> Option<T> {
        if self.hiding() {
            return None;
        }
        self.rows
            .lock()
            .unwrap()
            .values()
            .find(|r| matches(r))
            .cloned()
    }

    pub(crate) fn all(&self) -> Vec<T> {
        if self.hiding() {
            return Vec::new();
        }
        self.rows.lock().unwrap().values().cloned().collect()
    }

    pub(crate) fn insert(
        &self,
        id: K,
        row: &mut T,
        id_key: &'static str,
        unique: UniqueCheck<'_, T>,
    ) -> Result<(), RepoError> {
        insert_row(&mut self.rows.lock().unwrap(), id, row, id_key, unique)
    }

    pub(crate) fn update(
        &self,
        id: K,
        row: &mut T,
        unique: UniqueCheck<'_, T>,
    ) -> Result<(), RepoError> {
        update_row(&mut self.rows.lock().unwrap(), id, row, unique)
    }

    /// Updates every row or none: all versions are checked (and unique keys too) before
    /// anything is written, like one database transaction.
    pub(crate) fn update_many(
        &self,
        rows_to_write: &mut [T],
        id_of: impl Fn(&T) -> K,
        unique: UniqueCheck<'_, T>,
    ) -> Result<(), RepoError> {
        let mut rows = self.rows.lock().unwrap();
        for row in rows_to_write.iter() {
            let id = id_of(row);
            match rows.get(&id) {
                Some(stored) if stored.version() == row.version() => {}
                _ => return Err(RepoError::StaleVersion),
            }
            if let Some(key) = rows
                .iter()
                .filter(|(other_id, _)| **other_id != id)
                .find_map(|(_, other)| unique(row, other))
            {
                return Err(RepoError::Duplicate(key));
            }
        }
        for row in rows_to_write.iter_mut() {
            row.set_version(row.version().next());
            rows.insert(id_of(row), row.clone());
        }
        Ok(())
    }
}

/// `Table::insert` on rows that are already locked.
pub(crate) fn insert_row<K: Eq + Hash, T: Clone + Versioned>(
    rows: &mut HashMap<K, T>,
    id: K,
    row: &mut T,
    id_key: &'static str,
    unique: UniqueCheck<'_, T>,
) -> Result<(), RepoError> {
    if rows.contains_key(&id) {
        return Err(RepoError::Duplicate(id_key));
    }
    if let Some(key) = rows.values().find_map(|other| unique(row, other)) {
        return Err(RepoError::Duplicate(key));
    }
    row.set_version(Version::NEW.next());
    rows.insert(id, row.clone());
    Ok(())
}

/// `Table::update` on rows that are already locked.
pub(crate) fn update_row<K: Eq + Hash, T: Clone + Versioned>(
    rows: &mut HashMap<K, T>,
    id: K,
    row: &mut T,
    unique: UniqueCheck<'_, T>,
) -> Result<(), RepoError> {
    match rows.get(&id) {
        Some(stored) if stored.version() == row.version() => {}
        // Changed by someone else, or deleted: either way this write is stale.
        _ => return Err(RepoError::StaleVersion),
    }
    if let Some(key) = rows
        .iter()
        .filter(|(other_id, _)| **other_id != id)
        .find_map(|(_, other)| unique(row, other))
    {
        return Err(RepoError::Duplicate(key));
    }
    row.set_version(row.version().next());
    rows.insert(id, row.clone());
    Ok(())
}

/// For tables with no unique keys besides the ID.
pub(crate) fn no_unique_keys<T>(_: &T, _: &T) -> Option<&'static str> {
    None
}
