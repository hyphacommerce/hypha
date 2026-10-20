//! The module's ports (design note 0033, §4): definitions, and values stored one row
//! per (definition, owner) outside the owners' aggregates (CF5).

use std::collections::HashSet;

use hypha_core::{
    CustomFieldDefinitionId, Page, PageLimit, PendingEvent, RepoError, Stamps, Version,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    decimal::Decimal,
    definition::CustomFieldDefinition,
    events::{FieldChange, fields_updated},
    namespace::{Key, Namespace},
    owner::{Owner, OwnerType},
    types::Reference,
    value::FieldValue,
};

/// Uniqueness rules the repos enforce, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const CUSTOM_FIELD_DEFINITION_ID: &str = "custom_field_definition_id";
    /// One definition per (owner type, namespace, key).
    pub const CUSTOM_FIELD_KEY: &str = "custom_field_key";
    /// A `unique` definition's values: no two owners share one (`ValueRow::unique_text`,
    /// case-insensitive).
    pub const CUSTOM_FIELD_UNIQUE_VALUE: &str = "custom_field_unique_value";
}

/// A value as stored: its row's JSON and version. Read back with
/// `FieldValue::from_stored` and the definition's type.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredValue {
    pub definition: CustomFieldDefinitionId,
    pub owner: Uuid,
    /// A variant owner's product (`Owner::variant`), so events about it can be recorded
    /// without the products module; `None` for other owners.
    pub owner_parent: Option<Uuid>,
    pub value: Value,
    /// 1 when first written, one more each change: what `if_versions` compares.
    pub version: Version,
    pub stamps: Option<Stamps>,
}

/// A value row to write: the value and its derived index columns (§4), computed by
/// `ValueRow::new` so every adapter stores the same thing.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueRow {
    pub definition: CustomFieldDefinitionId,
    pub owner: Uuid,
    /// A variant's product, as `StoredValue::owner_parent`.
    pub owner_parent: Option<Uuid>,
    pub value: Value,
    /// Lowercased text, choice, url, reference; booleans; integers' digits. For filters.
    pub scalar_text: Option<String>,
    /// Exact numbers, dates as microseconds. For range filters.
    pub scalar_num: Option<Decimal>,
    /// What `unique` compares, only when the definition is unique: adapters index
    /// `(definition, unique_text)` uniquely (`keys::CUSTOM_FIELD_UNIQUE_VALUE`, checked
    /// when the batch commits, so one batch may move a value between owners); `None`
    /// never clashes.
    pub unique_text: Option<String>,
    /// A list's items as `contains` filters compare them (`list_items`): strings
    /// lowercased. `None` for a single value.
    pub list_items: Option<Value>,
}

impl ValueRow {
    pub fn new(definition: &CustomFieldDefinition, owner: &Owner, value: &FieldValue) -> Self {
        let json = value.to_json();
        Self {
            definition: definition.id(),
            owner: owner.id(),
            owner_parent: owner.product_of_variant().map(|p| p.as_uuid()),
            list_items: value.items().and_then(|_| list_items(&json)),
            value: json,
            scalar_text: value.scalar_text(),
            scalar_num: value.scalar_num(),
            unique_text: definition
                .capabilities()
                .unique
                .then(|| value.unique_key())
                .flatten(),
        }
    }
}

/// An item as list filters compare it (`FieldFilterOp::ContainsAny`): a string
/// lowercased (as `scalar_text` lowercases single values), anything else as is.
pub fn list_item(item: &Value) -> Value {
    match item {
        Value::String(s) => Value::String(s.to_lowercase()),
        other => other.clone(),
    }
}

/// The `list_items` column of a stored list: each item as `list_item`; `None` for a
/// value that isn't a list. What adapters write with every list, including after a
/// delta (`ValueRepo::change_items`).
pub fn list_items(value: &Value) -> Option<Value> {
    value
        .as_array()
        .map(|items| Value::Array(items.iter().map(list_item).collect()))
}

/// A distinct value (or list item) of a definition and how many owners hold it, for
/// autocomplete (`values-in-use`, §4).
#[derive(Debug, Clone, PartialEq)]
pub struct ValueCount {
    pub value: Value,
    pub owners: u64,
}

/// What list items compare by: their JSON, lowercased when `case_insensitive` and the
/// item is a string.
pub fn item_key(item: &Value, case_insensitive: bool) -> String {
    match item {
        Value::String(s) if case_insensitive => Value::String(s.to_lowercase()).to_string(),
        other => other.to_string(),
    }
}

/// Items added to and removed from one owner's list (CF6), applied by the adapter under
/// the rows' locks (`ValueRepo::change_items`). Its rules are here, in `apply`, so every
/// adapter applies them the same way.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemsChange {
    pub definition: CustomFieldDefinitionId,
    /// `namespace.key`, for the events.
    pub key: String,
    pub personal: bool,
    pub owners: Vec<Owner>,
    /// Each as stored (canonical JSON), checked by the item type already.
    pub add: Vec<Value>,
    pub remove: Vec<Value>,
    /// Text compared ignoring case (`unique_items` on a text list).
    pub case_insensitive: bool,
    pub min_items: Option<u32>,
    pub max_items: u32,
}

/// One owner's list after an `ItemsChange`.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemsApplied {
    /// The new list; `None` when it's empty, so the row goes.
    pub value: Option<Value>,
    pub added: Vec<Value>,
    pub removed: Vec<Value>,
}

impl ItemsApplied {
    pub fn changed(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty()
    }
}

/// Why an owner's list can't take the change: the whole call fails, naming the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemsProblem {
    TooMany { count: usize, max: u32 },
    TooFew { count: usize, min: u32 },
}

/// What an owner's list did.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnerItems {
    pub owner: Uuid,
    pub added: Vec<Value>,
    pub removed: Vec<Value>,
    /// The row's version after; `None` when the list is now empty (the row went) or
    /// never existed.
    pub version: Option<Version>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemsResult {
    /// Every owner's list, in the call's order; unchanged owners have empty deltas.
    Applied(Vec<OwnerItems>),
    /// Nothing was written: this owner's list would break `max_items`/`min_items`.
    Refused { owner: Uuid, problem: ItemsProblem },
}

impl ItemsChange {
    /// `current`'s list after the change: removals first, then additions not present
    /// already (each once). Removing what's absent or adding what's present changes
    /// nothing.
    pub fn apply(&self, current: Option<&Value>) -> ItemsApplied {
        let ci = self.case_insensitive;
        let mut items: Vec<Value> = current
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let removing: HashSet<String> = self.remove.iter().map(|v| item_key(v, ci)).collect();
        let mut removed = Vec::new();
        items.retain(|item| {
            let gone = removing.contains(&item_key(item, ci));
            if gone {
                removed.push(item.clone());
            }
            !gone
        });
        let mut present: HashSet<String> = items.iter().map(|v| item_key(v, ci)).collect();
        let mut added = Vec::new();
        for item in &self.add {
            if present.insert(item_key(item, ci)) {
                items.push(item.clone());
                added.push(item.clone());
            }
        }
        // An item both removed and added back is no change to report.
        let readded: HashSet<String> = added.iter().map(|v| item_key(v, ci)).collect();
        let back: HashSet<String> = removed
            .iter()
            .map(|v| item_key(v, ci))
            .filter(|k| readded.contains(k))
            .collect();
        if !back.is_empty() {
            removed.retain(|v| !back.contains(&item_key(v, ci)));
            added.retain(|v| !back.contains(&item_key(v, ci)));
        }
        ItemsApplied {
            value: (!items.is_empty()).then_some(Value::Array(items)),
            added,
            removed,
        }
    }

    /// Whether the applied list breaks the item limits (an empty list is no value, so it
    /// never does).
    pub fn problem(&self, applied: &ItemsApplied) -> Option<ItemsProblem> {
        let count = applied
            .value
            .as_ref()
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        if count > self.max_items as usize {
            return Some(ItemsProblem::TooMany {
                count,
                max: self.max_items,
            });
        }
        match self.min_items {
            Some(min) if count > 0 && count < min as usize => {
                Some(ItemsProblem::TooFew { count, min })
            }
            _ => None,
        }
    }

    /// The event for `owner`'s change; `None` when nothing changed.
    pub fn event(&self, owner: &Owner, applied: &ItemsApplied) -> Option<PendingEvent> {
        if !applied.changed() {
            return None;
        }
        fields_updated(
            owner,
            vec![FieldChange::delta(
                &self.key,
                applied.added.clone(),
                applied.removed.clone(),
                self.personal,
            )],
        )
    }
}

#[allow(async_fn_in_trait)]
pub trait DefinitionRepo {
    async fn get(
        &self,
        id: CustomFieldDefinitionId,
    ) -> Result<Option<CustomFieldDefinition>, RepoError>;
    async fn get_by_key(
        &self,
        owner_type: OwnerType,
        namespace: &Namespace,
        key: &Key,
    ) -> Result<Option<CustomFieldDefinition>, RepoError>;
    /// Definitions of `owner_type` (all of them for `None`), by owner type then
    /// position. A store has tens per owner type.
    async fn list(
        &self,
        owner_type: Option<OwnerType>,
    ) -> Result<Vec<CustomFieldDefinition>, RepoError>;
    /// Every definition in `namespace`, any owner type: what an app's or a spore's sync
    /// compares with.
    async fn list_namespace(
        &self,
        namespace: &Namespace,
    ) -> Result<Vec<CustomFieldDefinition>, RepoError>;
    /// Stores a new definition (version 1). Enforces `keys`.
    async fn insert(&self, definition: &mut CustomFieldDefinition) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, definition: &mut CustomFieldDefinition) -> Result<(), RepoError>;
    /// Deletes the definition and every value of it, writing its events
    /// (`mark_deleted` first); `StaleVersion` from an old copy.
    async fn delete(&self, definition: &mut CustomFieldDefinition) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait ValueRepo {
    /// The values `owners` hold, of `definitions` only when given: one query for a page
    /// of owners (`WHERE owner_id = ANY($1)`). In no particular order.
    async fn get_many(
        &self,
        owners: &[Uuid],
        definitions: Option<&[CustomFieldDefinitionId]>,
    ) -> Result<Vec<StoredValue>, RepoError>;
    /// A page of the definition's values by owner ID, ascending, after `after`: for
    /// checking a tightened validation against every stored value (§2).
    async fn page(
        &self,
        definition: CustomFieldDefinitionId,
        after: Option<Uuid>,
        limit: PageLimit,
    ) -> Result<Page<StoredValue>, RepoError>;
    /// The value of a `unique` definition whose `unique_text` is this, if any: the
    /// pre-check before a write, and lookups by value.
    async fn find_unique(
        &self,
        definition: CustomFieldDefinitionId,
        unique_text: &str,
    ) -> Result<Option<StoredValue>, RepoError>;
    /// Distinct values of the definition (each item, for a list) with how many owners
    /// hold each, most held first then by value, at most `limit`.
    async fn values_in_use(
        &self,
        definition: CustomFieldDefinitionId,
        limit: u32,
    ) -> Result<Vec<ValueCount>, RepoError>;
    /// Values of `definitions` that are one of `targets` or hold one as an item (a
    /// reference's prefixed ID), each value once, in one query: what the cleanup clears
    /// when targets are deleted (§6.3).
    async fn referencing(
        &self,
        definitions: &[CustomFieldDefinitionId],
        targets: &[String],
    ) -> Result<Vec<StoredValue>, RepoError>;
    /// Applies a list delta to each owner's row (CF6) in one transaction, under the rows'
    /// locks, so concurrent changes both land: `ItemsChange::apply` per owner, then, if
    /// any owner's list breaks `ItemsChange::problem`, nothing is written and the first
    /// such owner is `Refused`. Otherwise rows are inserted (version 1), updated (one
    /// more) or deleted (empty), with `ItemsChange::event` per changed owner written to
    /// the outbox in the same transaction.
    async fn change_items(&self, change: &ItemsChange) -> Result<ItemsResult, RepoError>;
    /// Deletes every value the owners hold (they were deleted), recording nothing.
    /// Returns how many rows went. Running it again deletes nothing.
    async fn delete_for_owners(&self, owners: &[Uuid]) -> Result<u32, RepoError>;
}

/// Value writes as a batch stages them (with their events, through
/// `hypha_core::StageEvents`).
pub trait StageCustomFieldValues {
    /// Inserts or replaces `(row.definition, row.owner)`. `expected`: `None` writes
    /// whatever is stored (last write wins); `Some(Version::NEW)` only if there's no
    /// row; `Some(v)` only if the row is at `v`; else the commit fails with
    /// `StaleVersion`. The commit enforces `CUSTOM_FIELD_UNIQUE_VALUE`. Precondition: the
    /// definition exists.
    fn set_custom_field_value(&mut self, row: ValueRow, expected: Option<Version>);
    /// Removes `(definition, owner)`; `expected` as for `set_custom_field_value`
    /// (`None`: whatever is there, if anything).
    fn remove_custom_field_value(
        &mut self,
        definition: CustomFieldDefinitionId,
        owner: Uuid,
        expected: Option<Version>,
    );
    /// Rewrites the derived columns of `(row.definition, row.owner)` from `row` (its
    /// value is what's stored; the version and `updated_at` stay: the value didn't
    /// change, only how its definition indexes it, e.g. `unique` turned on or off), if
    /// the row is still at `at`; else the commit fails with `StaleVersion`. The commit
    /// enforces `CUSTOM_FIELD_UNIQUE_VALUE`. Precondition: the definition exists.
    fn reindex_custom_field_value(&mut self, row: ValueRow, at: Version);
}

/// Definition writes as a batch stages them: a move renumbers siblings in one commit.
/// Staging moves a definition to the version it gets on commit.
pub trait StageCustomFieldDefinitions {
    /// A new definition; the commit enforces `keys`.
    fn insert_custom_field_definition(&mut self, definition: &mut CustomFieldDefinition);
    /// A changed one; `StaleVersion` from an old copy.
    fn update_custom_field_definition(&mut self, definition: &mut CustomFieldDefinition);
    /// Deletes it and its values (`mark_deleted` first for the event); `StaleVersion`
    /// from an old copy.
    fn delete_custom_field_definition(&mut self, definition: &mut CustomFieldDefinition);
}

/// Whether references' targets exist (§3: checked when a value is written). The leaf
/// can't see products or assets: the host answers from their repositories.
#[allow(async_fn_in_trait)]
pub trait ReferenceTargets {
    /// Which of `references` exist, with their kind.
    async fn existing(&self, references: &[Reference]) -> Result<HashSet<Reference>, RepoError>;
}

impl<T: ReferenceTargets + ?Sized> ReferenceTargets for &T {
    async fn existing(&self, references: &[Reference]) -> Result<HashSet<Reference>, RepoError> {
        (**self).existing(references).await
    }
}

pub trait HasCustomFieldDefinitions {
    type CustomFieldDefinitions: DefinitionRepo;
    fn custom_field_definitions(&self) -> &Self::CustomFieldDefinitions;
}

impl<T: HasCustomFieldDefinitions + ?Sized> HasCustomFieldDefinitions for &T {
    type CustomFieldDefinitions = T::CustomFieldDefinitions;
    fn custom_field_definitions(&self) -> &Self::CustomFieldDefinitions {
        (**self).custom_field_definitions()
    }
}

pub trait HasCustomFieldValues {
    type CustomFieldValues: ValueRepo;
    fn custom_field_values(&self) -> &Self::CustomFieldValues;
}

impl<T: HasCustomFieldValues + ?Sized> HasCustomFieldValues for &T {
    type CustomFieldValues = T::CustomFieldValues;
    fn custom_field_values(&self) -> &Self::CustomFieldValues {
        (**self).custom_field_values()
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::CustomerId;
    use serde_json::json;

    use super::*;

    fn change(add: Value, remove: Value, ci: bool) -> ItemsChange {
        ItemsChange {
            definition: CustomFieldDefinitionId::from_uuid(Uuid::from_u128(1)),
            key: "custom.tags".to_string(),
            personal: false,
            owners: vec![Owner::customer(CustomerId::from_uuid(Uuid::from_u128(2)))],
            add: add.as_array().cloned().unwrap_or_default(),
            remove: remove.as_array().cloned().unwrap_or_default(),
            case_insensitive: ci,
            min_items: None,
            max_items: 3,
        }
    }

    #[test]
    fn adding_what_is_there_changes_nothing() {
        let applied = change(json!(["vip"]), json!([]), false).apply(Some(&json!(["vip"])));
        assert!(!applied.changed());
        assert_eq!(applied.value, Some(json!(["vip"])));
    }

    #[test]
    fn case_is_ignored_only_when_asked() {
        let current = json!(["VIP"]);
        assert!(
            !change(json!(["vip"]), json!([]), true)
                .apply(Some(&current))
                .changed()
        );
        assert_eq!(
            change(json!(["vip"]), json!([]), false)
                .apply(Some(&current))
                .value,
            Some(json!(["VIP", "vip"]))
        );
        assert_eq!(
            change(json!([]), json!(["vip"]), true)
                .apply(Some(&current))
                .removed,
            vec![json!("VIP")]
        );
    }

    #[test]
    fn removing_the_last_item_removes_the_value() {
        let applied = change(json!([]), json!(["a"]), false).apply(Some(&json!(["a"])));
        assert_eq!(applied.value, None);
        assert_eq!(applied.removed, vec![json!("a")]);
    }

    #[test]
    fn max_items_is_checked_after_the_change() {
        let c = change(json!(["d"]), json!(["a"]), false);
        assert_eq!(c.problem(&c.apply(Some(&json!(["a", "b", "c"])))), None);
        let c = change(json!(["d"]), json!([]), false);
        assert_eq!(
            c.problem(&c.apply(Some(&json!(["a", "b", "c"])))),
            Some(ItemsProblem::TooMany { count: 4, max: 3 })
        );
    }
}
