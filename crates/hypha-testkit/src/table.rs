use std::{
    collections::HashMap,
    hash::Hash,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
};

use hypha_core::{Page, PageRequest, RepoError, Version, Versioned};

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

    /// Removes the row, if there is one.
    pub(crate) fn remove(&self, id: &K) {
        self.rows.lock().unwrap().remove(id);
    }

    pub(crate) fn get_many(&self, ids: &[K]) -> Vec<T> {
        let rows = self.rows.lock().unwrap();
        ids.iter().filter_map(|id| rows.get(id).cloned()).collect()
    }

    /// Makes the next `n` lookups by something other than the ID (`find`, `all`, `page`) see no
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

    /// A page of the rows `matches` keeps, newest first by key (UUIDv7 IDs sort by
    /// creation time), below `page.after` when given, each turned into what the list
    /// returns by `view`: what a database does with `WHERE id < $after ORDER BY id DESC
    /// LIMIT n + 1`. Only the page's rows are viewed. Hidden like `find` and `all`
    /// (`hide_next_lookups`).
    pub(crate) fn page<U>(
        &self,
        matches: impl Fn(&T) -> bool,
        page: PageRequest<K>,
        view: impl Fn(&T) -> U,
    ) -> Page<U>
    where
        K: Ord,
    {
        if self.hiding() {
            return Page::from_overfetch(Vec::new(), page.limit);
        }
        let rows = self.rows.lock().unwrap();
        let mut found: Vec<(&K, &T)> = rows
            .iter()
            .filter(|(id, row)| page.after.is_none_or(|after| **id < after) && matches(row))
            .collect();
        found.sort_unstable_by(|(a, _), (b, _)| b.cmp(a));
        let fetched = found
            .into_iter()
            .take(page.limit.get() as usize + 1)
            .map(|(_, row)| view(row))
            .collect();
        Page::from_overfetch(fetched, page.limit)
    }

    /// Every row, oldest first by key (UUIDv7 IDs sort by creation time): what a
    /// database's `ORDER BY id` returns. Hidden like `all`.
    pub(crate) fn all_oldest_first(&self) -> Vec<T>
    where
        K: Ord,
    {
        if self.hiding() {
            return Vec::new();
        }
        let rows = self.rows.lock().unwrap();
        let mut found: Vec<(&K, &T)> = rows.iter().collect();
        found.sort_unstable_by_key(|(key, _)| *key);
        found.into_iter().map(|(_, row)| row.clone()).collect()
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
        let mut seen = std::collections::HashSet::new();
        if rows_to_write.iter().any(|row| !seen.insert(id_of(row))) {
            // Two versions of one row in one write: no database would pick one quietly.
            return Err(RepoError::Backend(
                "the same row twice in one update_many".to_string(),
            ));
        }
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

/// A batch write naming a parent that doesn't exist: a broken caller precondition,
/// which a database reports as a foreign key violation (`Backend`).
pub(crate) fn missing(what: &str, id: impl std::fmt::Display) -> RepoError {
    RepoError::Backend(format!("{what} {id} does not exist"))
}

/// For tables with no unique keys besides the ID.
pub(crate) fn no_unique_keys<T>(_: &T, _: &T) -> Option<&'static str> {
    None
}

/// A page of `(key, value)` pairs stored apart from their table (membership, fixed
/// prices), newest first by key below `page.after`: `Table::page` for a plain map.
pub(crate) fn page_of<K: Ord + Copy, V: Clone>(
    entries: impl Iterator<Item = (K, V)>,
    page: PageRequest<K>,
) -> Page<(K, V)> {
    let mut found: Vec<(K, V)> = entries
        .filter(|(key, _)| page.after.is_none_or(|after| *key < after))
        .collect();
    found.sort_unstable_by(|(a, _), (b, _)| b.cmp(a));
    found.truncate(page.limit.get() as usize + 1);
    Page::from_overfetch(found, page.limit)
}

#[cfg(test)]
mod tests {
    use hypha_core::{PageLimit, Version, Versioned};

    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Row(u32, Version);

    impl Versioned for Row {
        fn version(&self) -> Version {
            self.1
        }
        fn set_version(&mut self, version: Version) {
            self.1 = version;
        }
    }

    fn table_of(ids: &[u32]) -> Table<u32, Row> {
        let table = Table::default();
        for &id in ids {
            table
                .insert(id, &mut Row(id, Version::NEW), "row_id", &no_unique_keys)
                .unwrap();
        }
        table
    }

    fn request(after: Option<u32>, limit: u32) -> PageRequest<u32> {
        PageRequest {
            after,
            limit: PageLimit::new(limit).unwrap(),
        }
    }

    #[test]
    fn a_page_is_newest_first_below_the_cursor() {
        let table = table_of(&[1, 4, 2, 5, 3]);
        let page = table.page(|_| true, request(Some(4), 2), |r| r.0);
        assert_eq!(
            page,
            Page {
                items: vec![3, 2],
                has_more: true
            }
        );
    }

    #[test]
    fn a_page_keeps_only_matching_rows() {
        let table = table_of(&[1, 2, 3, 4]);
        let page = table.page(|r| r.0 % 2 == 0, request(None, 10), |r| r.0);
        assert_eq!(
            page,
            Page {
                items: vec![4, 2],
                has_more: false
            }
        );
    }

    #[test]
    fn a_hidden_lookup_sees_an_empty_page() {
        let table = table_of(&[1, 2]);
        table.hide_next_lookups(1);
        assert_eq!(
            table.page(|_| true, request(None, 10), |r| r.0).items,
            Vec::<u32>::new()
        );
        // Used up: the next page sees the rows again.
        assert_eq!(
            table.page(|_| true, request(None, 10), |r| r.0).items,
            vec![2, 1]
        );
    }
}
