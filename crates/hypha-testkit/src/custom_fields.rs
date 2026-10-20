use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, MutexGuard},
};

use hypha_core::{
    CustomFieldDefinitionId, FieldFilter, FieldFilterOp, FilterScalar, Page, PageLimit,
    PendingEvent, Recorded, RepoError, Stamps, Version, Versioned, stamp_insert, stamp_update,
};
use hypha_custom_fields::{
    CustomFieldDefinition, Decimal, DefinitionRepo, ItemsChange, ItemsResult, Key, Namespace,
    OwnerItems, OwnerType, StoredValue, ValueCount, ValueRepo, ValueRow, keys,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    InMemoryOutbox,
    table::{Table, missing},
};

/// A value row as stored.
#[derive(Debug, Clone)]
pub(crate) struct ValueRecord {
    pub(crate) row: ValueRow,
    pub(crate) version: Version,
    pub(crate) stamps: Stamps,
}

impl ValueRecord {
    fn stored(&self) -> StoredValue {
        StoredValue {
            definition: self.row.definition,
            owner: self.row.owner,
            owner_parent: self.row.owner_parent,
            value: self.row.value.clone(),
            version: self.version,
            stamps: Some(self.stamps),
        }
    }
}

/// Values by (definition, owner), as the table's primary key.
pub(crate) type Values = BTreeMap<(CustomFieldDefinitionId, Uuid), ValueRecord>;

/// Custom field definitions and their values (design note 0033), in memory. One type
/// for both repos, since deleting a definition takes its values with it.
#[derive(Default)]
pub struct InMemoryCustomFields {
    /// Shared with a linked app repo, which deletes an app's namespace with it.
    pub(crate) definitions: Arc<Table<CustomFieldDefinitionId, CustomFieldDefinition>>,
    pub(crate) values: Arc<Mutex<Values>>,
    pub(crate) outbox: InMemoryOutbox,
}

/// One definition per (owner type, namespace, key).
pub(crate) fn unique_definition(
    new: &CustomFieldDefinition,
    other: &CustomFieldDefinition,
) -> Option<&'static str> {
    (new.owner_type() == other.owner_type()
        && new.namespace() == other.namespace()
        && new.key() == other.key())
    .then_some(keys::CUSTOM_FIELD_KEY)
}

/// What the values table's keys enforce for `row`, against the other stored rows: its
/// definition exists, and a unique value is held once per definition.
pub(crate) fn check_value(
    definitions: &HashMap<CustomFieldDefinitionId, CustomFieldDefinition>,
    values: &Values,
    row: &ValueRow,
) -> Result<(), RepoError> {
    if !definitions.contains_key(&row.definition) {
        return Err(missing("custom field definition", row.definition));
    }
    if let Some(unique) = &row.unique_text
        && values.iter().any(|((definition, owner), other)| {
            *definition == row.definition
                && *owner != row.owner
                && other.row.unique_text.as_ref() == Some(unique)
        })
    {
        return Err(RepoError::Duplicate(keys::CUSTOM_FIELD_UNIQUE_VALUE));
    }
    Ok(())
}

/// Writes `row` if `expected` matches what's stored (`None`: anything).
pub(crate) fn set_value(
    definitions: &HashMap<CustomFieldDefinitionId, CustomFieldDefinition>,
    values: &mut Values,
    row: ValueRow,
    expected: Option<Version>,
    now: hypha_core::Timestamp,
) -> Result<(), RepoError> {
    let id = (row.definition, row.owner);
    let stored = values.get(&id);
    let current = stored.map_or(Version::NEW, |r| r.version);
    if expected.is_some_and(|e| e != current) {
        return Err(RepoError::StaleVersion);
    }
    check_value(definitions, values, &row)?;
    let stamps = stored.map_or_else(|| Stamps::new(now), |r| r.stamps.touched(now));
    values.insert(
        id,
        ValueRecord {
            row,
            version: current.next(),
            stamps,
        },
    );
    Ok(())
}

/// Removes `(definition, owner)` if `expected` matches (`None`: whatever is there).
pub(crate) fn remove_value(
    values: &mut Values,
    definition: CustomFieldDefinitionId,
    owner: Uuid,
    expected: Option<Version>,
) -> Result<(), RepoError> {
    let current = values
        .get(&(definition, owner))
        .map_or(Version::NEW, |r| r.version);
    if expected.is_some_and(|e| e != current) {
        return Err(RepoError::StaleVersion);
    }
    values.remove(&(definition, owner));
    Ok(())
}

impl InMemoryCustomFields {
    /// Repos writing their events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The values, locked, for a batch (`InMemoryPorts::commit`).
    pub(crate) fn lock_values(&self) -> MutexGuard<'_, Values> {
        self.values.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Makes the next `n` lookups other than by ID see no definitions (`find`, `list`):
    /// to lose the race between a command's check and its write.
    pub fn hide_next_lookups(&self, n: usize) {
        self.definitions.hide_next_lookups(n);
    }

    /// Deletes every definition in `namespace` and their values, as the database's
    /// `ON DELETE CASCADE` from `apps` does when the app goes (design note 0033, §8).
    /// No events: storage cascades record none.
    pub(crate) fn drop_namespace(&self, namespace: &Namespace) {
        let mut rows = self.definitions.lock();
        let gone: Vec<CustomFieldDefinitionId> = rows
            .values()
            .filter(|d| d.namespace() == namespace)
            .map(|d| d.id())
            .collect();
        for id in &gone {
            rows.remove(id);
        }
        self.lock_values().retain(|(d, _), _| !gone.contains(d));
    }

    /// A view of the same definitions and values (for the app repo's cascade).
    pub(crate) fn view(&self) -> Self {
        Self {
            definitions: Arc::clone(&self.definitions),
            values: Arc::clone(&self.values),
            outbox: self.outbox.clone(),
        }
    }

    fn sorted(mut all: Vec<CustomFieldDefinition>) -> Vec<CustomFieldDefinition> {
        all.sort_by_key(|d| (d.owner_type(), d.position(), d.id()));
        all
    }
}

impl DefinitionRepo for InMemoryCustomFields {
    async fn get(
        &self,
        id: CustomFieldDefinitionId,
    ) -> Result<Option<CustomFieldDefinition>, RepoError> {
        Ok(self.definitions.get(&id))
    }

    async fn get_by_key(
        &self,
        owner_type: OwnerType,
        namespace: &Namespace,
        key: &Key,
    ) -> Result<Option<CustomFieldDefinition>, RepoError> {
        Ok(self
            .definitions
            .find(|d| d.owner_type() == owner_type && d.namespace() == namespace && d.key() == key))
    }

    async fn list(
        &self,
        owner_type: Option<OwnerType>,
    ) -> Result<Vec<CustomFieldDefinition>, RepoError> {
        let all = self.definitions.all();
        Ok(Self::sorted(
            all.into_iter()
                .filter(|d| owner_type.is_none_or(|t| d.owner_type() == t))
                .collect(),
        ))
    }

    async fn list_namespace(
        &self,
        namespace: &Namespace,
    ) -> Result<Vec<CustomFieldDefinition>, RepoError> {
        let all = self.definitions.all();
        Ok(Self::sorted(
            all.into_iter()
                .filter(|d| d.namespace() == namespace)
                .collect(),
        ))
    }

    async fn insert(&self, definition: &mut CustomFieldDefinition) -> Result<(), RepoError> {
        stamp_insert(definition, self.outbox.now());
        let events = definition.take_events();
        self.definitions.insert(
            definition.id(),
            definition,
            keys::CUSTOM_FIELD_DEFINITION_ID,
            &unique_definition,
        )?;
        self.outbox.write(events, Some(definition.version()));
        Ok(())
    }

    async fn update(&self, definition: &mut CustomFieldDefinition) -> Result<(), RepoError> {
        stamp_update(definition, self.outbox.now());
        let events = definition.take_events();
        self.definitions
            .update(definition.id(), definition, &unique_definition)?;
        self.outbox.write(events, Some(definition.version()));
        Ok(())
    }

    async fn delete(&self, definition: &mut CustomFieldDefinition) -> Result<(), RepoError> {
        let events = definition.take_events();
        {
            let mut rows = self.definitions.lock();
            match rows.get(&definition.id()) {
                Some(stored) if stored.version() == definition.version() => {}
                _ => return Err(RepoError::StaleVersion),
            }
            rows.remove(&definition.id());
            self.lock_values().retain(|(d, _), _| *d != definition.id());
        }
        self.outbox.write(events, None);
        Ok(())
    }
}

impl ValueRepo for InMemoryCustomFields {
    async fn get_many(
        &self,
        owners: &[Uuid],
        definitions: Option<&[CustomFieldDefinitionId]>,
    ) -> Result<Vec<StoredValue>, RepoError> {
        Ok(self
            .lock_values()
            .values()
            .filter(|r| owners.contains(&r.row.owner))
            .filter(|r| definitions.is_none_or(|ds| ds.contains(&r.row.definition)))
            .map(ValueRecord::stored)
            .collect())
    }

    async fn page(
        &self,
        definition: CustomFieldDefinitionId,
        after: Option<Uuid>,
        limit: PageLimit,
    ) -> Result<Page<StoredValue>, RepoError> {
        let found: Vec<StoredValue> = self
            .lock_values()
            .range((definition, Uuid::nil())..=(definition, Uuid::from_u128(u128::MAX)))
            .filter(|((_, owner), _)| after.is_none_or(|a| *owner > a))
            .take(limit.get() as usize + 1)
            .map(|(_, r)| r.stored())
            .collect();
        Ok(Page::from_overfetch(found, limit))
    }

    async fn find_unique(
        &self,
        definition: CustomFieldDefinitionId,
        unique_text: &str,
    ) -> Result<Option<StoredValue>, RepoError> {
        Ok(self
            .lock_values()
            .values()
            .find(|r| {
                r.row.definition == definition && r.row.unique_text.as_deref() == Some(unique_text)
            })
            .map(ValueRecord::stored))
    }

    async fn values_in_use(
        &self,
        definition: CustomFieldDefinitionId,
        limit: u32,
    ) -> Result<Vec<ValueCount>, RepoError> {
        let mut counts: BTreeMap<String, (Value, u64)> = BTreeMap::new();
        for record in self
            .lock_values()
            .values()
            .filter(|r| r.row.definition == definition)
        {
            let items = match &record.row.value {
                Value::Array(items) => items.clone(),
                single => vec![single.clone()],
            };
            let mut seen = std::collections::HashSet::new();
            for item in items {
                if seen.insert(item.to_string()) {
                    counts.entry(item.to_string()).or_insert((item, 0)).1 += 1;
                }
            }
        }
        let mut found: Vec<(String, Value, u64)> =
            counts.into_iter().map(|(k, (v, n))| (k, v, n)).collect();
        found.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        Ok(found
            .into_iter()
            .take(limit as usize)
            .map(|(_, value, owners)| ValueCount { value, owners })
            .collect())
    }

    async fn referencing(
        &self,
        definitions: &[CustomFieldDefinitionId],
        target: &str,
    ) -> Result<Vec<StoredValue>, RepoError> {
        let target = Value::String(target.to_string());
        Ok(self
            .lock_values()
            .values()
            .filter(|r| definitions.contains(&r.row.definition))
            .filter(|r| match &r.row.value {
                Value::Array(items) => items.contains(&target),
                single => *single == target,
            })
            .map(ValueRecord::stored)
            .collect())
    }

    async fn change_items(&self, change: &ItemsChange) -> Result<ItemsResult, RepoError> {
        let definitions = self.definitions.lock();
        if !definitions.contains_key(&change.definition) {
            return Err(missing("custom field definition", change.definition));
        }
        let mut values = self.lock_values();
        let now = self.outbox.now();
        let mut applied = Vec::with_capacity(change.owners.len());
        for owner in &change.owners {
            let current = values.get(&(change.definition, owner.id()));
            let result = change.apply(current.map(|r| &r.row.value));
            if let Some(problem) = change.problem(&result) {
                return Ok(ItemsResult::Refused {
                    owner: owner.id(),
                    problem,
                });
            }
            applied.push((owner, result));
        }
        let mut events: Vec<PendingEvent> = Vec::new();
        let mut outcomes = Vec::with_capacity(applied.len());
        for (owner, result) in applied {
            let id = (change.definition, owner.id());
            let mut version = values.get(&id).map(|r| r.version);
            if result.changed() {
                match &result.value {
                    Some(list) => {
                        let row = ValueRow {
                            definition: change.definition,
                            owner: owner.id(),
                            owner_parent: owner.product_of_variant().map(|p| p.as_uuid()),
                            value: list.clone(),
                            scalar_text: None,
                            scalar_num: None,
                            unique_text: None,
                        };
                        set_value(&definitions, &mut values, row, None, now)?;
                        version = values.get(&id).map(|r| r.version);
                    }
                    None => {
                        values.remove(&id);
                        version = None;
                    }
                }
                events.extend(change.event(owner, &result));
            }
            outcomes.push(OwnerItems {
                owner: owner.id(),
                added: result.added,
                removed: result.removed,
                version,
            });
        }
        drop(values);
        drop(definitions);
        self.outbox.write(events, None);
        Ok(ItemsResult::Applied(outcomes))
    }

    async fn delete_for_owners(&self, owners: &[Uuid]) -> Result<u32, RepoError> {
        let mut values = self.lock_values();
        let before = values.len();
        values.retain(|(_, owner), _| !owners.contains(owner));
        Ok((before - values.len()) as u32)
    }
}

/// The values, shared with the repos whose lists filter on them (`FieldFilter`), as a
/// database joins them. Empty unless shared (`InMemoryPorts` shares its own).
pub(crate) type SharedValues = Arc<Mutex<Values>>;

impl InMemoryCustomFields {
    /// The values, for a repo whose list filters on them (`joining_fields`).
    pub(crate) fn shared_values(&self) -> SharedValues {
        self.values.clone()
    }
}

/// A copy of `values` when there are `filters` to apply (so no lock is held while the
/// listed table's is), `None` when there are none.
pub(crate) fn values_for(values: &SharedValues, filters: &[FieldFilter]) -> Option<Values> {
    (!filters.is_empty()).then(|| values.lock().unwrap_or_else(|e| e.into_inner()).clone())
}

/// Whether `owner`'s values match every filter, as the Postgres adapter's predicates
/// do: equality and ranges on the derived `scalar_text` / `scalar_num` columns, list
/// containment on the value.
pub(crate) fn fields_match(values: Option<&Values>, owner: Uuid, filters: &[FieldFilter]) -> bool {
    let Some(values) = values else {
        return true;
    };
    filters.iter().all(|filter| {
        let row = values.get(&(filter.definition, owner)).map(|r| &r.row);
        match &filter.op {
            FieldFilterOp::Exists(exists) => row.is_some() == *exists,
            FieldFilterOp::AnyOf(scalars) => {
                row.is_some_and(|row| scalars.iter().any(|s| compares(row, s, Ordering::is_eq)))
            }
            FieldFilterOp::ContainsAny(items) => row.is_some_and(|row| match &row.value {
                Value::Array(held) => items.iter().any(|item| held.contains(item)),
                _ => false,
            }),
            FieldFilterOp::Gt(s) => row.is_some_and(|row| compares(row, s, Ordering::is_gt)),
            FieldFilterOp::Gte(s) => row.is_some_and(|row| compares(row, s, Ordering::is_ge)),
            FieldFilterOp::Lt(s) => row.is_some_and(|row| compares(row, s, Ordering::is_lt)),
            FieldFilterOp::Lte(s) => row.is_some_and(|row| compares(row, s, Ordering::is_le)),
        }
    })
}

/// Whether the row's scalar compared with the operand (row first) passes `test`; a row
/// without that scalar never does.
fn compares(row: &ValueRow, scalar: &FilterScalar, test: fn(Ordering) -> bool) -> bool {
    match scalar {
        FilterScalar::Text(t) => row
            .scalar_text
            .as_deref()
            .is_some_and(|held| test(held.cmp(t.as_str()))),
        FilterScalar::Number(n) => match (row.scalar_num, Decimal::parse(n)) {
            (Some(held), Ok(n)) => test(held.cmp(&n)),
            _ => false,
        },
    }
}
