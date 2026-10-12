use std::{collections::HashMap, hash::Hash, sync::Mutex};

use hypha_core::{RepoError, Version, Versioned};

/// One in-memory "table" of aggregates with the same write rules a database adapter
/// has: `insert` fails on a duplicate ID or unique key, `update` fails with
/// `StaleVersion` when the stored version moved on. Every in-memory repo is built on
/// this, so command tests see the same behavior as production.
pub(crate) struct Table<K, T> {
    rows: Mutex<HashMap<K, T>>,
}

impl<K, T> Default for Table<K, T> {
    fn default() -> Self {
        Self {
            rows: Mutex::new(HashMap::new()),
        }
    }
}

/// Checks a new or changed row against one *other* stored row and returns the unique
/// key it would break, if any. The row's own previous state is never passed in.
pub(crate) type UniqueCheck<'a, T> = &'a dyn Fn(&T, &T) -> Option<&'static str>;

impl<K: Eq + Hash + Copy, T: Clone + Versioned> Table<K, T> {
    pub(crate) fn get(&self, id: &K) -> Option<T> {
        self.rows.lock().unwrap().get(id).cloned()
    }

    pub(crate) fn get_many(&self, ids: &[K]) -> Vec<T> {
        let rows = self.rows.lock().unwrap();
        ids.iter().filter_map(|id| rows.get(id).cloned()).collect()
    }

    pub(crate) fn find(&self, matches: impl Fn(&T) -> bool) -> Option<T> {
        self.rows
            .lock()
            .unwrap()
            .values()
            .find(|r| matches(r))
            .cloned()
    }

    pub(crate) fn all(&self) -> Vec<T> {
        self.rows.lock().unwrap().values().cloned().collect()
    }

    pub(crate) fn insert(
        &self,
        id: K,
        row: &mut T,
        id_key: &'static str,
        unique: UniqueCheck<'_, T>,
    ) -> Result<(), RepoError> {
        let mut rows = self.rows.lock().unwrap();
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

    pub(crate) fn update(
        &self,
        id: K,
        row: &mut T,
        unique: UniqueCheck<'_, T>,
    ) -> Result<(), RepoError> {
        let mut rows = self.rows.lock().unwrap();
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
}

/// For tables with no unique keys besides the ID.
pub(crate) fn no_unique_keys<T>(_: &T, _: &T) -> Option<&'static str> {
    None
}
