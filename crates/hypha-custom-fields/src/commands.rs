//! Custom field commands (design note 0033, §2, §4, §7, §8). Definitions in `custom`
//! need `custom_fields:*`; values are governed by their owner's scopes and `access`
//! (§5). Owners come in already checked to exist (§6.1).

use std::collections::{BTreeMap, HashMap, HashSet};

use hypha_core::{
    AuthError, Ctx, CustomFieldDefinitionId, ErrorCode, FieldFilter, FieldFilterOp, FilterScalar,
    HasBatch, IdGenerator, PageLimit, RepoError, Scope, StageEvents, Version, Versioned,
};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    access::{FieldCaller, ReadAccess, WriteDenied, read_access, write_access},
    decimal::Decimal,
    definition::{CustomFieldDefinition, DefinitionChange, DefinitionSpec},
    error::DefinitionError,
    events::{FieldChange, fields_updated},
    limits::{
        MAX_DEFINITIONS_PER_NAMESPACE, MAX_FILTERABLE, MAX_INVALID_OWNERS_LISTED,
        MAX_KEYS_PER_CALL, MAX_LIST_ITEMS, MAX_LISTED_KEYS, MAX_OWNERS_PER_CALL,
    },
    namespace::{FieldKey, Key, Namespace},
    owner::{Owner, OwnerType},
    repo::{
        DefinitionRepo, HasCustomFieldDefinitions, HasCustomFieldValues, ItemsChange, ItemsProblem,
        ItemsResult, OwnerItems, ReferenceTargets, StageCustomFieldDefinitions,
        StageCustomFieldValues, StoredValue, ValueCount, ValueRepo, ValueRow, keys,
    },
    types::{BaseType, FieldType, Reference},
    value::{FieldValue, ValueProblem, codes},
};

/// One problem with one field in a write (`custom_fields_invalid`'s `details`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldProblem {
    /// `namespace.key` as sent.
    pub key: String,
    /// The owner, in bulk calls (`prod_…`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Where in the value: `custom.sizes[3]`.
    pub path: String,
    pub code: &'static str,
    pub message: String,
}

impl FieldProblem {
    fn new(key: &str, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            key: key.to_string(),
            owner: None,
            path: key.to_string(),
            code,
            message: message.into(),
        }
    }

    fn of_value(key: &str, problem: ValueProblem) -> Self {
        let problem = problem.under(key);
        Self {
            key: key.to_string(),
            owner: None,
            path: problem.path,
            code: problem.code,
            message: problem.message,
        }
    }

    fn for_owner(mut self, owner: &Owner) -> Self {
        self.owner = Some(owner.prefixed());
        self
    }
}

/// Problem codes that aren't about a value's content.
pub mod problem_codes {
    pub const UNKNOWN: &str = "custom_field_unknown";
    pub const INVALID_KEY: &str = "invalid_custom_field_key";
    pub const READ_ONLY: &str = "custom_field_read_only";
    /// A personal field without `customer_data:profile`.
    pub const MISSING_SCOPE: &str = "missing_scope";
    pub const VALUE_TAKEN: &str = "custom_field_value_taken";
    /// An `if_versions` key that the call doesn't write.
    pub const IF_VERSION_UNUSED: &str = "if_version_unused";
    pub const DUPLICATE_OWNER: &str = "duplicate_owner";
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Definition(#[from] DefinitionError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error("custom field definition {0} not found")]
    DefinitionNotFound(CustomFieldDefinitionId),
    #[error("{owner_type} already has a field {key}")]
    KeyTaken { owner_type: OwnerType, key: String },
    #[error("{0} is managed by its app or spore, not through this API")]
    Managed(String),
    #[error("only an app's or a spore's namespace is synced, not {0}")]
    NotManagedNamespace(Namespace),
    #[error("{count} stored values break the change; fix them first")]
    ValuesInvalid { count: usize, owners: Vec<String> },
    #[error("the custom fields are invalid")]
    FieldsInvalid(Vec<FieldProblem>),
    #[error("{0} changed since it was read")]
    Stale(String),
    #[error("at most {max} fields per call")]
    TooManyKeys { max: usize },
    #[error("at most {max} owners per call")]
    TooManyOwners { max: usize },
    #[error("name at least one owner")]
    NoOwners,
    #[error("{owner} would have {count} items; at most {max}")]
    TooManyItems {
        owner: String,
        count: usize,
        max: u32,
    },
    #[error("{owner} would have {count} items; at least {min}")]
    TooFewItems {
        owner: String,
        count: usize,
        min: u32,
    },
    #[error("{0} isn't a list field")]
    NotAList(String),
    #[error("{0} is a {1} field")]
    OwnerTypeMismatch(String, OwnerType),
    #[error("at most {max} {owner_type} fields per namespace")]
    TooManyDefinitions { owner_type: OwnerType, max: usize },
    #[error("at most {max} filterable {owner_type} fields")]
    TooManyFilterable { owner_type: OwnerType, max: usize },
    #[error("{0}'s type can't change: a different type is a new field")]
    TypeChanged(String),
    #[error("{0} is read-only for this caller")]
    ReadOnly(String),
    #[error("no custom field {0}")]
    Unknown(String),
    #[error("{0} isn't filterable")]
    NotFilterable(String),
    #[error("{0}: {1}")]
    InvalidFilter(String, String),
    #[error("at most {max} custom fields in a list")]
    TooManyListedKeys { max: usize },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Definition(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::DefinitionNotFound(_) => "custom_field_definition_not_found",
            CommandError::KeyTaken { .. } => "custom_field_exists",
            CommandError::Managed(_) => "custom_field_managed",
            CommandError::NotManagedNamespace(_) => "namespace_not_managed",
            CommandError::ValuesInvalid { .. } => "custom_field_values_invalid",
            CommandError::FieldsInvalid(_) => "custom_fields_invalid",
            CommandError::Stale(_) => "custom_field_stale",
            CommandError::TooManyKeys { .. } => "too_many_custom_fields_in_call",
            CommandError::TooManyOwners { .. } => "too_many_owners",
            CommandError::NoOwners => "owners_required",
            CommandError::TooManyItems { .. } => "too_many_items",
            CommandError::TooFewItems { .. } => "too_few_items",
            CommandError::NotAList(_) => "custom_field_not_a_list",
            CommandError::OwnerTypeMismatch(..) => "owner_type_mismatch",
            CommandError::TooManyDefinitions { .. } => "too_many_custom_fields",
            CommandError::TooManyFilterable { .. } => "too_many_filterable_custom_fields",
            CommandError::TypeChanged(_) => "custom_field_type_changed",
            CommandError::ReadOnly(_) => "custom_field_read_only",
            CommandError::Unknown(_) => problem_codes::UNKNOWN,
            CommandError::NotFilterable(_) => "custom_field_not_filterable",
            CommandError::InvalidFilter(..) => "invalid_custom_field_filter",
            CommandError::TooManyListedKeys { .. } => "too_many_custom_field_keys",
        }
    }
}

// ---------------------------------------------------------------------------------
// Definitions
// ---------------------------------------------------------------------------------

async fn load<S: HasCustomFieldDefinitions>(
    ports: &S,
    id: CustomFieldDefinitionId,
) -> Result<CustomFieldDefinition, CommandError> {
    ports
        .custom_field_definitions()
        .get(id)
        .await?
        .ok_or(CommandError::DefinitionNotFound(id))
}

fn key_taken(definition: &CustomFieldDefinition) -> CommandError {
    CommandError::KeyTaken {
        owner_type: definition.owner_type(),
        key: definition.field_key().to_string(),
    }
}

/// The limits on how many definitions an owner type holds (§10), for `definition` about
/// to be added (or made filterable) among `existing` (its owner type's, itself excluded).
fn check_limits(
    existing: &[CustomFieldDefinition],
    definition: &CustomFieldDefinition,
    adding: bool,
) -> Result<(), CommandError> {
    let others = existing.iter().filter(|d| d.id() != definition.id());
    if adding
        && others
            .clone()
            .filter(|d| d.namespace() == definition.namespace())
            .count()
            >= MAX_DEFINITIONS_PER_NAMESPACE
    {
        return Err(CommandError::TooManyDefinitions {
            owner_type: definition.owner_type(),
            max: MAX_DEFINITIONS_PER_NAMESPACE,
        });
    }
    if definition.capabilities().filterable
        && others.filter(|d| d.capabilities().filterable).count() >= MAX_FILTERABLE
    {
        return Err(CommandError::TooManyFilterable {
            owner_type: definition.owner_type(),
            max: MAX_FILTERABLE,
        });
    }
    Ok(())
}

/// Checks every stored value of `current` against `next` (§2), page by page: values
/// that fail its validations, and with `unique` turned on every owner after the first
/// holding a value. `ValuesInvalid` names the first 20.
async fn check_stored_values<S: HasCustomFieldValues>(
    ports: &S,
    current: &CustomFieldDefinition,
    next: &CustomFieldDefinition,
) -> Result<(), CommandError> {
    let mut after = None;
    let mut bad: Vec<Uuid> = Vec::new();
    let mut seen = HashSet::new();
    let checking_unique = next.capabilities().unique && !current.capabilities().unique;
    loop {
        let page = ports
            .custom_field_values()
            .page(current.id(), after, max_page())
            .await?;
        for stored in &page.items {
            match FieldValue::parse(next.field_type(), next.validations(), &stored.value) {
                Ok(value) => {
                    if checking_unique
                        && let Some(key) = value.unique_key()
                        && !seen.insert(key)
                    {
                        bad.push(stored.owner);
                    }
                }
                Err(_) => bad.push(stored.owner),
            }
        }
        after = page.items.last().map(|v| v.owner);
        if !page.has_more {
            break;
        }
    }
    if bad.is_empty() {
        return Ok(());
    }
    Err(CommandError::ValuesInvalid {
        count: bad.len(),
        owners: bad
            .iter()
            .take(MAX_INVALID_OWNERS_LISTED)
            .map(|id| next.owner_type().format_id(*id))
            .collect(),
    })
}

/// Pages of `PageLimit::MAX` for walking every value of a definition.
fn max_page() -> PageLimit {
    PageLimit::new(PageLimit::MAX).unwrap_or_default()
}

/// Creates a definition in the merchant's `custom` namespace, last on its owner type's
/// form. Apps and spores declare theirs (`sync_managed_definitions`).
pub async fn create_definition<S: HasCustomFieldDefinitions>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    spec: DefinitionSpec,
) -> Result<CustomFieldDefinition, CommandError> {
    ctx.require(Scope::CustomFieldsWrite)?;
    let repo = ports.custom_field_definitions();
    let siblings = repo.list(Some(spec.owner_type)).await?;
    let mut definition = CustomFieldDefinition::new(
        CustomFieldDefinitionId::from_uuid(ids.new_uuid()),
        Namespace::Custom,
        spec,
        siblings.len() as u32,
    )?;
    if siblings
        .iter()
        .any(|d| d.field_key() == definition.field_key())
    {
        return Err(key_taken(&definition));
    }
    check_limits(&siblings, &definition, true)?;
    match repo.insert(&mut definition).await {
        Err(RepoError::Duplicate(keys::CUSTOM_FIELD_KEY)) => Err(key_taken(&definition)),
        other => Ok(other?),
    }?;
    Ok(definition)
}

pub async fn get_definition<S: HasCustomFieldDefinitions>(
    ctx: &Ctx,
    ports: &S,
    id: CustomFieldDefinitionId,
) -> Result<CustomFieldDefinition, CommandError> {
    ctx.require(Scope::CustomFieldsRead)?;
    load(ports, id).await
}

/// Every namespace's definitions of `owner_type` (all owner types for `None`), by owner
/// type then position.
pub async fn list_definitions<S: HasCustomFieldDefinitions>(
    ctx: &Ctx,
    ports: &S,
    owner_type: Option<OwnerType>,
) -> Result<Vec<CustomFieldDefinition>, CommandError> {
    ctx.require(Scope::CustomFieldsRead)?;
    Ok(ports.custom_field_definitions().list(owner_type).await?)
}

/// The definitions of `owner_type`, without a scope check: for routes that read or
/// write an owner's values (loaded once per request, §6.2), whose own checks apply.
pub async fn definitions_for<S: HasCustomFieldDefinitions>(
    ports: &S,
    owner_type: OwnerType,
) -> Result<Vec<CustomFieldDefinition>, RepoError> {
    ports
        .custom_field_definitions()
        .list(Some(owner_type))
        .await
}

/// Changes a `custom` definition. A change that tightens its validations (or turns
/// `unique` on) is checked against the stored values first (`ValuesInvalid`).
pub async fn update_definition<S: HasCustomFieldDefinitions + HasCustomFieldValues>(
    ctx: &Ctx,
    ports: &S,
    id: CustomFieldDefinitionId,
    change: DefinitionChange,
) -> Result<CustomFieldDefinition, CommandError> {
    ctx.require(Scope::CustomFieldsWrite)?;
    let mut definition = load(ports, id).await?;
    if definition.is_managed() {
        return Err(CommandError::Managed(definition.field_key().to_string()));
    }
    let (next, fields) = definition.changed(change.clone())?;
    if fields.is_empty() {
        return Ok(definition);
    }
    if next.capabilities().filterable && !definition.capabilities().filterable {
        let siblings = ports
            .custom_field_definitions()
            .list(Some(definition.owner_type()))
            .await?;
        check_limits(&siblings, &next, false)?;
    }
    if definition.tightened_by(&next) {
        check_stored_values(ports, &definition, &next).await?;
    }
    definition.change(change)?;
    ports
        .custom_field_definitions()
        .update(&mut definition)
        .await?;
    Ok(definition)
}

/// Moves a `custom` definition to `position` on its owner type's form (clamped to the
/// last), renumbering the others densely in the same batch.
pub async fn move_definition<S>(
    ctx: &Ctx,
    ports: &S,
    id: CustomFieldDefinitionId,
    position: u32,
) -> Result<CustomFieldDefinition, CommandError>
where
    S: HasCustomFieldDefinitions + HasBatch<Batch: StageCustomFieldDefinitions>,
{
    ctx.require(Scope::CustomFieldsWrite)?;
    let moving = load(ports, id).await?;
    if moving.is_managed() {
        return Err(CommandError::Managed(moving.field_key().to_string()));
    }
    let mut siblings = ports
        .custom_field_definitions()
        .list(Some(moving.owner_type()))
        .await?;
    siblings.sort_by_key(|d| (d.position(), d.id()));
    let Some(from) = siblings.iter().position(|d| d.id() == id) else {
        return Err(CommandError::DefinitionNotFound(id));
    };
    let mut definition = siblings.remove(from);
    let to = (position as usize).min(siblings.len());
    if definition.position() as usize == to && from == to {
        return Ok(definition);
    }
    siblings.insert(to, definition.clone());
    let mut batch = ports.batch();
    for (i, sibling) in siblings.iter_mut().enumerate() {
        if sibling.id() == id {
            definition.move_to(i as u32);
            batch.update_custom_field_definition(&mut definition);
        } else if sibling.position() != i as u32 {
            sibling.shift_to(i as u32);
            batch.update_custom_field_definition(sibling);
        }
    }
    ports.commit(batch).await?;
    Ok(definition)
}

/// Deletes a `custom` definition and every value of it (§2), renumbering the rest of
/// its owner type's form.
pub async fn delete_definition<S>(
    ctx: &Ctx,
    ports: &S,
    id: CustomFieldDefinitionId,
) -> Result<(), CommandError>
where
    S: HasCustomFieldDefinitions + HasBatch<Batch: StageCustomFieldDefinitions>,
{
    ctx.require(Scope::CustomFieldsWrite)?;
    let definition = load(ports, id).await?;
    if definition.is_managed() {
        return Err(CommandError::Managed(definition.field_key().to_string()));
    }
    let mut batch = ports.batch();
    stage_deletions(ports, &mut batch, &[definition]).await?;
    Ok(ports.commit(batch).await?)
}

/// Stages deleting `gone` and renumbering what's left of their owner types densely.
async fn stage_deletions<S, B>(
    ports: &S,
    batch: &mut B,
    gone: &[CustomFieldDefinition],
) -> Result<(), CommandError>
where
    S: HasCustomFieldDefinitions,
    B: StageCustomFieldDefinitions,
{
    let gone_ids: HashSet<CustomFieldDefinitionId> = gone.iter().map(|d| d.id()).collect();
    let owner_types: HashSet<OwnerType> = gone.iter().map(|d| d.owner_type()).collect();
    for definition in gone {
        let mut definition = definition.clone();
        definition.mark_deleted();
        batch.delete_custom_field_definition(&mut definition);
    }
    for owner_type in owner_types {
        let mut left: Vec<CustomFieldDefinition> = ports
            .custom_field_definitions()
            .list(Some(owner_type))
            .await?
            .into_iter()
            .filter(|d| !gone_ids.contains(&d.id()))
            .collect();
        left.sort_by_key(|d| (d.position(), d.id()));
        for (i, sibling) in left.iter_mut().enumerate() {
            if sibling.position() != i as u32 {
                sibling.shift_to(i as u32);
                batch.update_custom_field_definition(sibling);
            }
        }
    }
    Ok(())
}

/// One field an app or a spore declares, under its namespace.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncedField {
    pub owner_type: OwnerType,
    pub key: String,
}

/// What syncing a namespace would do, or did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncPlan {
    pub create: Vec<SyncedField>,
    /// With the fields that change.
    pub update: Vec<(SyncedField, Vec<&'static str>)>,
    /// Deleted with their values: what a consent screen lists as "removes fields".
    pub delete: Vec<SyncedField>,
}

/// What syncing `specs` into a namespace holding `existing` would do (§8), checking
/// every spec builds a valid definition and none changes an existing key's type
/// (`TypeChanged`, naming the key). Pure, so a manifest's consent screen can show it.
pub fn plan_sync(
    namespace: &Namespace,
    existing: &[CustomFieldDefinition],
    specs: &[DefinitionSpec],
) -> Result<SyncPlan, CommandError> {
    if namespace.managed_by().is_none() {
        return Err(CommandError::NotManagedNamespace(namespace.clone()));
    }
    let mut plan = SyncPlan::default();
    let mut declared = HashSet::new();
    for spec in specs {
        let probe = CustomFieldDefinition::new(
            CustomFieldDefinitionId::from_uuid(Uuid::nil()),
            namespace.clone(),
            spec.clone(),
            0,
        )?;
        let field = SyncedField {
            owner_type: spec.owner_type,
            key: probe.key().to_string(),
        };
        if !declared.insert((spec.owner_type, probe.key().clone())) {
            return Err(key_taken(&probe));
        }
        match existing
            .iter()
            .find(|d| d.owner_type() == spec.owner_type && d.key() == probe.key())
        {
            Some(current) => {
                if current.field_type() != spec.field_type {
                    return Err(CommandError::TypeChanged(current.field_key().to_string()));
                }
                let (_, fields) = current.changed(change_to(spec))?;
                if !fields.is_empty() {
                    plan.update.push((field, fields));
                }
            }
            None => plan.create.push(field),
        }
    }
    for current in existing {
        if !declared.contains(&(current.owner_type(), current.key().clone())) {
            plan.delete.push(SyncedField {
                owner_type: current.owner_type(),
                key: current.key().to_string(),
            });
        }
    }
    Ok(plan)
}

/// The change that makes a definition match `spec`.
fn change_to(spec: &DefinitionSpec) -> DefinitionChange {
    DefinitionChange {
        name: Some(spec.name.clone()),
        description: Some(spec.description.clone()),
        validations: Some(spec.validations.clone()),
        access: Some(spec.access),
        private: Some(spec.private),
        personal: Some(spec.personal),
        capabilities: Some(spec.capabilities),
    }
}

/// Makes an app's or a spore's namespace hold exactly `specs` (§8): new keys created,
/// changed ones updated (a tightening checked against stored values like any other),
/// missing ones deleted with their values, in one batch. A changed type is
/// `TypeChanged`. Run when an app's manifest is approved or refreshed, and by hosts for
/// spores at startup.
pub async fn sync_managed_definitions<S>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    namespace: &Namespace,
    specs: Vec<DefinitionSpec>,
) -> Result<SyncPlan, CommandError>
where
    S: HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasBatch<Batch: StageCustomFieldDefinitions>,
{
    ctx.require(Scope::AppsWrite)?;
    let repo = ports.custom_field_definitions();
    let existing = repo.list_namespace(namespace).await?;
    let plan = plan_sync(namespace, &existing, &specs)?;
    if plan.create.is_empty() && plan.update.is_empty() && plan.delete.is_empty() {
        return Ok(plan);
    }
    let all = repo.list(None).await?;
    let gone: Vec<CustomFieldDefinition> = existing
        .iter()
        .filter(|d| {
            plan.delete
                .iter()
                .any(|f| f.owner_type == d.owner_type() && f.key == d.key().as_str())
        })
        .cloned()
        .collect();
    let gone_ids: HashSet<CustomFieldDefinitionId> = gone.iter().map(|d| d.id()).collect();
    let mut batch = ports.batch();
    // What each owner type holds after the sync, for positions and limits.
    let mut kept: HashMap<OwnerType, Vec<CustomFieldDefinition>> = HashMap::new();
    for d in all.iter().filter(|d| !gone_ids.contains(&d.id())) {
        kept.entry(d.owner_type()).or_default().push(d.clone());
    }
    for spec in specs {
        let current = existing
            .iter()
            .find(|d| d.owner_type() == spec.owner_type && d.key().as_str() == spec.key);
        match current {
            Some(current) => {
                let (next, fields) = current.changed(change_to(&spec))?;
                if fields.is_empty() {
                    continue;
                }
                let siblings = kept.get(&spec.owner_type).cloned().unwrap_or_default();
                check_limits(&siblings, &next, false)?;
                if current.tightened_by(&next) {
                    check_stored_values(ports, current, &next).await?;
                }
                let mut updated = current.clone();
                updated.change(change_to(&spec))?;
                batch.update_custom_field_definition(&mut updated);
                if let Some(list) = kept.get_mut(&spec.owner_type)
                    && let Some(slot) = list.iter_mut().find(|d| d.id() == updated.id())
                {
                    *slot = updated;
                }
            }
            None => {
                let siblings = kept.entry(spec.owner_type).or_default();
                let mut created = CustomFieldDefinition::new(
                    CustomFieldDefinitionId::from_uuid(ids.new_uuid()),
                    namespace.clone(),
                    spec,
                    siblings.len() as u32,
                )?;
                check_limits(siblings, &created, true)?;
                batch.insert_custom_field_definition(&mut created);
                siblings.push(created);
            }
        }
    }
    if !gone.is_empty() {
        for definition in &gone {
            let mut definition = definition.clone();
            definition.mark_deleted();
            batch.delete_custom_field_definition(&mut definition);
        }
        // Renumber what's left of the owner types that lost fields; definitions this
        // sync updates were staged already, so only untouched ones shift.
        let touched: HashSet<CustomFieldDefinitionId> = existing.iter().map(|d| d.id()).collect();
        for owner_type in gone.iter().map(|d| d.owner_type()).collect::<HashSet<_>>() {
            let mut left: Vec<CustomFieldDefinition> = kept
                .get(&owner_type)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|d| d.version() != Version::NEW)
                .collect();
            left.sort_by_key(|d| (d.position(), d.id()));
            for (i, sibling) in left.iter_mut().enumerate() {
                if sibling.position() != i as u32 && !touched.contains(&sibling.id()) {
                    sibling.shift_to(i as u32);
                    batch.update_custom_field_definition(sibling);
                }
            }
        }
    }
    match ports.commit(batch).await {
        Err(RepoError::Duplicate(keys::CUSTOM_FIELD_KEY)) => Err(CommandError::KeyTaken {
            owner_type: plan
                .create
                .first()
                .map_or(OwnerType::Product, |f| f.owner_type),
            key: format!("{namespace}.{}", plan.create.first().map_or("", |f| &f.key)),
        }),
        other => Ok(other?),
    }?;
    Ok(plan)
}

// ---------------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------------

/// One value an owner holds, with its definition and version.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldEntry {
    pub definition: CustomFieldDefinition,
    pub value: FieldValue,
    pub version: Version,
}

impl FieldEntry {
    pub fn key(&self) -> FieldKey {
        self.definition.field_key()
    }
}

/// An owner's values as one caller sees them (§6.2): the ones it may read, by position,
/// and the personal ones it may not (`redacted`, so responses list them).
#[derive(Debug, Clone, PartialEq)]
pub struct OwnerFields {
    pub owner: Uuid,
    pub entries: Vec<FieldEntry>,
    pub redacted: Vec<FieldKey>,
}

impl OwnerFields {
    /// `{"custom.material": "wool", …}`.
    pub fn to_json(&self) -> serde_json::Map<String, Value> {
        self.entries
            .iter()
            .map(|e| (e.key().to_string(), e.value.to_json()))
            .collect()
    }

    /// `{"custom.material": 3, …}`: what `if_versions` compares.
    pub fn versions(&self) -> BTreeMap<String, u64> {
        self.entries
            .iter()
            .map(|e| (e.key().to_string(), e.version.value()))
            .collect()
    }
}

/// Builds what `caller` sees of `stored` values of `owners`.
fn assemble(
    caller: &FieldCaller<'_>,
    definitions: &[CustomFieldDefinition],
    owners: &[Uuid],
    stored: Vec<StoredValue>,
) -> Result<HashMap<Uuid, OwnerFields>, CommandError> {
    let by_id: HashMap<CustomFieldDefinitionId, &CustomFieldDefinition> =
        definitions.iter().map(|d| (d.id(), d)).collect();
    let mut result: HashMap<Uuid, OwnerFields> = owners
        .iter()
        .map(|o| {
            (
                *o,
                OwnerFields {
                    owner: *o,
                    entries: Vec::new(),
                    redacted: Vec::new(),
                },
            )
        })
        .collect();
    for value in stored {
        let (Some(definition), Some(fields)) =
            (by_id.get(&value.definition), result.get_mut(&value.owner))
        else {
            continue;
        };
        match read_access(definition, caller) {
            ReadAccess::Hidden => {}
            ReadAccess::Redacted => fields.redacted.push(definition.field_key()),
            ReadAccess::Visible => {
                let parsed = FieldValue::from_stored(definition.field_type(), &value.value)
                    .map_err(|problems| {
                        RepoError::Backend(format!(
                            "stored value of {} for {} doesn't load: {problems:?}",
                            definition.field_key(),
                            value.owner
                        ))
                    })?;
                fields.entries.push(FieldEntry {
                    definition: (*definition).clone(),
                    value: parsed,
                    version: value.version,
                });
            }
        }
    }
    for fields in result.values_mut() {
        fields
            .entries
            .sort_by_key(|e| (e.definition.position(), e.definition.id()));
        fields.redacted.sort();
    }
    Ok(result)
}

/// The values `owner` holds that `caller` may read (§5), with versions.
pub async fn get_custom_fields<S>(
    caller: &FieldCaller<'_>,
    ports: &S,
    owner: &Owner,
) -> Result<OwnerFields, CommandError>
where
    S: HasCustomFieldDefinitions + HasCustomFieldValues,
{
    caller.ctx().require(owner.kind().read_scope())?;
    let definitions = definitions_for(ports, owner.kind()).await?;
    let stored = ports
        .custom_field_values()
        .get_many(&[owner.id()], None)
        .await?;
    let mut all = assemble(caller, &definitions, &[owner.id()], stored)?;
    Ok(all.remove(&owner.id()).unwrap_or(OwnerFields {
        owner: owner.id(),
        entries: Vec::new(),
        redacted: Vec::new(),
    }))
}

/// A page of owners' values for a list (`?custom_fields=…`, §6.2): only `keys` (at most
/// 10), one read for the page. Every owner of `owners` is in the result.
pub async fn custom_fields_of<S>(
    caller: &FieldCaller<'_>,
    ports: &S,
    owner_type: OwnerType,
    owners: &[Uuid],
    keys: &[FieldKey],
) -> Result<HashMap<Uuid, OwnerFields>, CommandError>
where
    S: HasCustomFieldDefinitions + HasCustomFieldValues,
{
    caller.ctx().require(owner_type.read_scope())?;
    if keys.len() > MAX_LISTED_KEYS {
        return Err(CommandError::TooManyListedKeys {
            max: MAX_LISTED_KEYS,
        });
    }
    let definitions = definitions_for(ports, owner_type).await?;
    let mut wanted = Vec::new();
    for key in keys {
        let found = definitions
            .iter()
            .find(|d| &d.field_key() == key && read_access(d, caller) != ReadAccess::Hidden)
            .ok_or_else(|| CommandError::Unknown(key.to_string()))?;
        wanted.push(found.id());
    }
    let stored = if owners.is_empty() || wanted.is_empty() {
        Vec::new()
    } else {
        ports
            .custom_field_values()
            .get_many(owners, Some(&wanted))
            .await?
    };
    assemble(caller, &definitions, owners, stored)
}

/// A write to an owner's fields: `values` is a JSON Merge Patch (`null` removes, absent
/// keeps); `if_versions` makes a key's write conditional on its current version (0: no
/// value yet).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetCustomFields {
    pub values: BTreeMap<String, Value>,
    pub if_versions: BTreeMap<String, u64>,
}

/// One checked write of one field: the value to store, or `None` to remove it.
struct Write<'d> {
    key: String,
    definition: &'d CustomFieldDefinition,
    value: Option<FieldValue>,
}

/// `definition`'s write access for `caller` as a problem on `key`.
fn access_problem(
    definition: &CustomFieldDefinition,
    caller: &FieldCaller<'_>,
    key: &str,
) -> Option<FieldProblem> {
    match write_access(definition, caller) {
        Ok(()) => None,
        Err(WriteDenied::ReadOnly) => Some(FieldProblem::new(
            key,
            problem_codes::READ_ONLY,
            format!("{key} is read-only for this caller"),
        )),
        Err(WriteDenied::Scope(scope)) => Some(FieldProblem::new(
            key,
            problem_codes::MISSING_SCOPE,
            format!("needs {scope}"),
        )),
        Err(WriteDenied::Personal) => Some(FieldProblem::new(
            key,
            problem_codes::MISSING_SCOPE,
            format!("{key} is personal: needs {}", Scope::CustomerDataProfile),
        )),
    }
}

/// The references in `values` whose targets don't exist, as problems at their paths.
async fn missing_references<P: ReferenceTargets>(
    ports: &P,
    values: &[(String, Option<String>, &FieldValue)],
) -> Result<Vec<FieldProblem>, RepoError> {
    let all: Vec<Reference> = values.iter().flat_map(|(_, _, v)| v.references()).collect();
    if all.is_empty() {
        return Ok(Vec::new());
    }
    let existing = ports.existing(&all).await?;
    let mut problems = Vec::new();
    for (key, owner, value) in values {
        let paths: Vec<(String, Reference)> = match value {
            FieldValue::Reference(r) => vec![(key.clone(), *r)],
            FieldValue::List(items) => items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| match item {
                    FieldValue::Reference(r) => Some((format!("{key}[{i}]"), *r)),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        for (path, reference) in paths {
            if !existing.contains(&reference) {
                problems.push(FieldProblem {
                    key: key.clone(),
                    owner: owner.clone(),
                    path,
                    code: codes::REFERENCE_NOT_FOUND,
                    message: format!("{reference} doesn't exist"),
                });
            }
        }
    }
    Ok(problems)
}

/// `unique` values another owner holds already, as problems.
async fn taken_values<S: HasCustomFieldValues>(
    ports: &S,
    writes: &[(Owner, &Write<'_>)],
    name_owners: bool,
) -> Result<Vec<FieldProblem>, RepoError> {
    let mut problems = Vec::new();
    let mut in_call: HashMap<(CustomFieldDefinitionId, String), Uuid> = HashMap::new();
    for (owner, write) in writes {
        let (Some(value), true) = (&write.value, write.definition.capabilities().unique) else {
            continue;
        };
        let Some(unique) = value.unique_key() else {
            continue;
        };
        let taken = match in_call.get(&(write.definition.id(), unique.clone())) {
            Some(other) => *other != owner.id(),
            None => ports
                .custom_field_values()
                .find_unique(write.definition.id(), &unique)
                .await?
                .is_some_and(|v| v.owner != owner.id()),
        };
        in_call.insert((write.definition.id(), unique), owner.id());
        if taken {
            let problem = FieldProblem::new(
                &write.key,
                problem_codes::VALUE_TAKEN,
                format!("another {} has this {}", owner.kind(), write.key),
            );
            problems.push(if name_owners {
                problem.for_owner(owner)
            } else {
                problem
            });
        }
    }
    Ok(problems)
}

/// Writes an owner's fields (§4): every key checked for access and parsed, every
/// problem reported at once (`FieldsInvalid`), then the changed rows and one
/// `<owner>.custom_fields_updated` event staged in one batch. At most 50 keys. Returns
/// the owner's fields as `caller` sees them after.
pub async fn set_custom_fields<S>(
    caller: &FieldCaller<'_>,
    ports: &S,
    owner: &Owner,
    input: SetCustomFields,
) -> Result<OwnerFields, CommandError>
where
    S: HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasBatch<Batch: StageCustomFieldValues + StageEvents>
        + ReferenceTargets,
{
    caller.ctx().require(owner.kind().write_scope())?;
    if input.values.len() > MAX_KEYS_PER_CALL {
        return Err(CommandError::TooManyKeys {
            max: MAX_KEYS_PER_CALL,
        });
    }
    let definitions = definitions_for(ports, owner.kind()).await?;
    let mut problems = Vec::new();
    let mut writes: Vec<Write<'_>> = Vec::new();
    for (key, json) in &input.values {
        let Some(definition) = find_definition(&definitions, key, &mut problems) else {
            continue;
        };
        if let Some(problem) = access_problem(definition, caller, key) {
            problems.push(problem);
            continue;
        }
        if json.is_null() {
            writes.push(Write {
                key: key.clone(),
                definition,
                value: None,
            });
            continue;
        }
        match FieldValue::parse(definition.field_type(), definition.validations(), json) {
            Ok(value) => writes.push(Write {
                key: key.clone(),
                definition,
                value: Some(value),
            }),
            Err(found) => {
                problems.extend(found.into_iter().map(|p| FieldProblem::of_value(key, p)))
            }
        }
    }
    for key in input.if_versions.keys() {
        if !input.values.contains_key(key) {
            problems.push(FieldProblem::new(
                key,
                problem_codes::IF_VERSION_UNUSED,
                format!("{key} has a version to compare but no value to write"),
            ));
        }
    }
    let with_values: Vec<(String, Option<String>, &FieldValue)> = writes
        .iter()
        .filter_map(|w| w.value.as_ref().map(|v| (w.key.clone(), None, v)))
        .collect();
    problems.extend(missing_references(ports, &with_values).await?);
    let owned: Vec<(Owner, &Write<'_>)> = writes.iter().map(|w| (*owner, w)).collect();
    problems.extend(taken_values(ports, &owned, false).await?);
    if !problems.is_empty() {
        return Err(CommandError::FieldsInvalid(problems));
    }

    let current: HashMap<CustomFieldDefinitionId, StoredValue> = ports
        .custom_field_values()
        .get_many(&[owner.id()], None)
        .await?
        .into_iter()
        .map(|v| (v.definition, v))
        .collect();
    for (key, expected) in &input.if_versions {
        let Some(write) = writes.iter().find(|w| &w.key == key) else {
            continue;
        };
        let now = current
            .get(&write.definition.id())
            .map_or(0, |v| v.version.value());
        if now != *expected {
            return Err(CommandError::Stale(key.clone()));
        }
    }

    let mut batch = ports.batch();
    let mut changes = Vec::new();
    for write in &writes {
        let stored = current.get(&write.definition.id());
        let expected = input.if_versions.get(&write.key).map(|v| Version::new(*v));
        match &write.value {
            Some(value) => {
                let row = ValueRow::new(write.definition, owner, value);
                if stored.is_some_and(|s| s.value == row.value) {
                    continue;
                }
                changes.push(FieldChange::value(
                    &write.key,
                    Some(row.value.clone()),
                    write.definition.personal(),
                ));
                batch.set_custom_field_value(row, expected);
            }
            None => {
                if stored.is_none() {
                    continue;
                }
                changes.push(FieldChange::value(
                    &write.key,
                    None,
                    write.definition.personal(),
                ));
                batch.remove_custom_field_value(write.definition.id(), owner.id(), expected);
            }
        }
    }
    if let Some(event) = fields_updated(owner, changes) {
        batch.record_events(vec![event]);
        match ports.commit(batch).await {
            Ok(()) => {}
            Err(RepoError::Duplicate(keys::CUSTOM_FIELD_UNIQUE_VALUE)) => {
                let found = taken_values(ports, &owned, false).await?;
                return Err(if found.is_empty() {
                    RepoError::Duplicate(keys::CUSTOM_FIELD_UNIQUE_VALUE).into()
                } else {
                    CommandError::FieldsInvalid(found)
                });
            }
            Err(RepoError::StaleVersion) if !input.if_versions.is_empty() => {
                let key = input.if_versions.keys().next().cloned().unwrap_or_default();
                return Err(CommandError::Stale(key));
            }
            Err(e) => return Err(e.into()),
        }
    }
    get_custom_fields(caller, ports, owner).await
}

/// The definition `key` names among `definitions`, or a problem.
fn find_definition<'d>(
    definitions: &'d [CustomFieldDefinition],
    key: &str,
    problems: &mut Vec<FieldProblem>,
) -> Option<&'d CustomFieldDefinition> {
    let Ok(field_key) = FieldKey::parse(key) else {
        problems.push(FieldProblem::new(
            key,
            problem_codes::INVALID_KEY,
            format!("{key:?} is not a field: \"namespace.key\", like \"custom.material\""),
        ));
        return None;
    };
    let found = definitions.iter().find(|d| d.field_key() == field_key);
    if found.is_none() {
        problems.push(FieldProblem::new(
            key,
            problem_codes::UNKNOWN,
            format!("no custom field {key}"),
        ));
    }
    found
}

/// A definition loaded for a bulk call on `owners`: it exists, they're all its owner
/// type, there are 1 to 250 of them, and `caller` may write it.
async fn bulk_target<S: HasCustomFieldDefinitions>(
    caller: &FieldCaller<'_>,
    ports: &S,
    definition: CustomFieldDefinitionId,
    owners: &[Owner],
) -> Result<CustomFieldDefinition, CommandError> {
    if owners.is_empty() {
        return Err(CommandError::NoOwners);
    }
    if owners.len() > MAX_OWNERS_PER_CALL {
        return Err(CommandError::TooManyOwners {
            max: MAX_OWNERS_PER_CALL,
        });
    }
    let definition = load(ports, definition).await?;
    let key = definition.field_key().to_string();
    if let Some(other) = owners.iter().find(|o| o.kind() != definition.owner_type()) {
        return Err(CommandError::OwnerTypeMismatch(
            format!(
                "{key} belongs to {} owners, not {}",
                definition.owner_type(),
                other.kind()
            ),
            definition.owner_type(),
        ));
    }
    match write_access(&definition, caller) {
        Ok(()) => Ok(definition),
        Err(WriteDenied::Scope(scope)) => Err(caller
            .ctx()
            .require(scope)
            .err()
            .map_or(CommandError::ReadOnly(key), CommandError::Auth)),
        Err(WriteDenied::Personal) => Err(caller
            .ctx()
            .require(Scope::CustomerDataProfile)
            .err()
            .map_or(CommandError::ReadOnly(key), CommandError::Auth)),
        Err(WriteDenied::ReadOnly) => Err(CommandError::ReadOnly(key)),
    }
}

/// Adds and removes items of one list field on 1 to 250 owners (CF6, §4): idempotent
/// (adding what's there or removing what isn't changes nothing and records nothing),
/// done by the repository under the rows' locks so concurrent writers both land. An
/// owner whose list would break `max_items` (or `min_items`) fails the whole call.
pub async fn change_list_items<S>(
    caller: &FieldCaller<'_>,
    ports: &S,
    definition: CustomFieldDefinitionId,
    owners: Vec<Owner>,
    add: Vec<Value>,
    remove: Vec<Value>,
) -> Result<Vec<OwnerItems>, CommandError>
where
    S: HasCustomFieldDefinitions + HasCustomFieldValues + ReferenceTargets,
{
    let definition = bulk_target(caller, ports, definition, &owners).await?;
    let key = definition.field_key().to_string();
    if !definition.field_type().is_list() {
        return Err(CommandError::NotAList(key));
    }
    let item_type = FieldType::single(definition.field_type().base());
    let mut problems = Vec::new();
    let mut added = Vec::new();
    for (i, item) in add.iter().enumerate() {
        match FieldValue::parse(item_type, definition.validations(), item) {
            Ok(value) => added.push(value),
            Err(found) => problems.extend(
                found
                    .into_iter()
                    .map(|p| FieldProblem::of_value(&format!("{key}.add[{i}]"), p)),
            ),
        }
    }
    let mut removed = Vec::new();
    for (i, item) in remove.iter().enumerate() {
        // Read by type alone: an item that no longer passes the validations can still go.
        match FieldValue::from_stored(item_type, item) {
            Ok(value) => removed.push(value.to_json()),
            Err(found) => problems.extend(
                found
                    .into_iter()
                    .map(|p| FieldProblem::of_value(&format!("{key}.remove[{i}]"), p)),
            ),
        }
    }
    let list = FieldValue::List(added.clone());
    problems.extend(missing_references(ports, &[(key.clone(), None, &list)]).await?);
    if !problems.is_empty() {
        return Err(CommandError::FieldsInvalid(problems));
    }
    let mut seen = HashSet::new();
    let owners: Vec<Owner> = owners.into_iter().filter(|o| seen.insert(o.id())).collect();
    let validations = definition.validations();
    let change = ItemsChange {
        definition: definition.id(),
        key: key.clone(),
        personal: definition.personal(),
        owners: owners.clone(),
        add: added.iter().map(FieldValue::to_json).collect(),
        remove: removed,
        case_insensitive: validations.unique_items()
            && definition.field_type().base() == BaseType::Text,
        min_items: validations.min_items(),
        max_items: validations.max_items().unwrap_or(MAX_LIST_ITEMS as u32),
    };
    match ports.custom_field_values().change_items(&change).await? {
        ItemsResult::Applied(outcomes) => Ok(outcomes),
        ItemsResult::Refused { owner, problem } => {
            let owner = definition.owner_type().format_id(owner);
            Err(match problem {
                ItemsProblem::TooMany { count, max } => {
                    CommandError::TooManyItems { owner, count, max }
                }
                ItemsProblem::TooFew { count, min } => {
                    CommandError::TooFewItems { owner, count, min }
                }
            })
        }
    }
}

/// Sets (or, with `None`, removes) one field on 1 to 250 owners (§4): imports and the
/// admin's "set on selected". All or nothing, one event per owner that changed.
pub async fn set_field_values<S>(
    caller: &FieldCaller<'_>,
    ports: &S,
    definition: CustomFieldDefinitionId,
    values: Vec<(Owner, Option<Value>)>,
) -> Result<(), CommandError>
where
    S: HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasBatch<Batch: StageCustomFieldValues + StageEvents>
        + ReferenceTargets,
{
    let owners: Vec<Owner> = values.iter().map(|(o, _)| *o).collect();
    let definition = bulk_target(caller, ports, definition, &owners).await?;
    let key = definition.field_key().to_string();
    let mut problems = Vec::new();
    let mut seen = HashSet::new();
    let mut writes: Vec<(Owner, Write<'_>)> = Vec::new();
    for (owner, json) in &values {
        if !seen.insert(owner.id()) {
            problems.push(
                FieldProblem::new(
                    &key,
                    problem_codes::DUPLICATE_OWNER,
                    format!("{owner} is in the call twice"),
                )
                .for_owner(owner),
            );
            continue;
        }
        let value = match json {
            None | Some(Value::Null) => None,
            Some(json) => {
                match FieldValue::parse(definition.field_type(), definition.validations(), json) {
                    Ok(value) => Some(value),
                    Err(found) => {
                        problems.extend(
                            found
                                .into_iter()
                                .map(|p| FieldProblem::of_value(&key, p).for_owner(owner)),
                        );
                        continue;
                    }
                }
            }
        };
        writes.push((
            *owner,
            Write {
                key: key.clone(),
                definition: &definition,
                value,
            },
        ));
    }
    let with_values: Vec<(String, Option<String>, &FieldValue)> = writes
        .iter()
        .filter_map(|(o, w)| {
            w.value
                .as_ref()
                .map(|v| (key.clone(), Some(o.prefixed()), v))
        })
        .collect();
    problems.extend(missing_references(ports, &with_values).await?);
    let owned: Vec<(Owner, &Write<'_>)> = writes.iter().map(|(o, w)| (*o, w)).collect();
    problems.extend(taken_values(ports, &owned, true).await?);
    if !problems.is_empty() {
        return Err(CommandError::FieldsInvalid(problems));
    }
    let ids: Vec<Uuid> = owners.iter().map(Owner::id).collect();
    let current: HashMap<Uuid, StoredValue> = ports
        .custom_field_values()
        .get_many(&ids, Some(&[definition.id()]))
        .await?
        .into_iter()
        .map(|v| (v.owner, v))
        .collect();
    let mut batch = ports.batch();
    let mut events = Vec::new();
    for (owner, write) in &writes {
        let stored = current.get(&owner.id());
        let change = match &write.value {
            Some(value) => {
                let row = ValueRow::new(&definition, owner, value);
                if stored.is_some_and(|s| s.value == row.value) {
                    continue;
                }
                let change =
                    FieldChange::value(&key, Some(row.value.clone()), definition.personal());
                batch.set_custom_field_value(row, None);
                change
            }
            None => {
                if stored.is_none() {
                    continue;
                }
                batch.remove_custom_field_value(definition.id(), owner.id(), None);
                FieldChange::value(&key, None, definition.personal())
            }
        };
        events.extend(fields_updated(owner, vec![change]));
    }
    if events.is_empty() {
        return Ok(());
    }
    batch.record_events(events);
    match ports.commit(batch).await {
        Err(RepoError::Duplicate(keys::CUSTOM_FIELD_UNIQUE_VALUE)) => {
            let found = taken_values(ports, &owned, true).await?;
            Err(if found.is_empty() {
                RepoError::Duplicate(keys::CUSTOM_FIELD_UNIQUE_VALUE).into()
            } else {
                CommandError::FieldsInvalid(found)
            })
        }
        other => Ok(other?),
    }
}

/// The distinct values (list items, for a list) of a filterable field with how many
/// owners hold each, most held first: autocomplete for filters and labels (§4).
pub async fn values_in_use<S>(
    caller: &FieldCaller<'_>,
    ports: &S,
    definition: CustomFieldDefinitionId,
    limit: PageLimit,
) -> Result<Vec<ValueCount>, CommandError>
where
    S: HasCustomFieldDefinitions + HasCustomFieldValues,
{
    let definition = load(ports, definition).await?;
    caller.ctx().require(definition.owner_type().read_scope())?;
    let key = definition.field_key().to_string();
    match read_access(&definition, caller) {
        ReadAccess::Visible => {}
        ReadAccess::Redacted => caller.ctx().require(Scope::CustomerDataProfile)?,
        ReadAccess::Hidden => return Err(CommandError::Unknown(key)),
    }
    if !definition.capabilities().filterable {
        return Err(CommandError::NotFilterable(key));
    }
    Ok(ports
        .custom_field_values()
        .values_in_use(definition.id(), limit.get())
        .await?)
}

// ---------------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------------

/// A list filter as a query string spells it (§7): `field[custom.material]=wool,cotton`
/// is `Eq` with two operands, `field[custom.weight][gte]=1.5kg` is `Gte`,
/// `field[custom.erp_id][exists]=false` is `Exists`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawFilterOp {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
    Exists,
}

impl RawFilterOp {
    /// The operator in `field[key][op]`; `None` for one that isn't.
    pub fn parse(op: &str) -> Option<RawFilterOp> {
        Some(match op {
            "eq" => RawFilterOp::Eq,
            "gt" => RawFilterOp::Gt,
            "gte" => RawFilterOp::Gte,
            "lt" => RawFilterOp::Lt,
            "lte" => RawFilterOp::Lte,
            "exists" => RawFilterOp::Exists,
            _ => return None,
        })
    }
}

/// Turns a query's filter into a `FieldFilter` (§7, CF7), or refuses it: an unknown or
/// hidden key (`custom_field_unknown`), a field that isn't filterable
/// (`custom_field_not_filterable`), a personal one without `customer_data:profile`, an
/// operator or operand the type doesn't take (`invalid_custom_field_filter`). Never
/// ignored. `definitions` are the list's owner type's.
pub fn field_filter(
    caller: &FieldCaller<'_>,
    definitions: &[CustomFieldDefinition],
    key: &str,
    op: RawFilterOp,
    operands: &[String],
) -> Result<FieldFilter, CommandError> {
    let unknown = || CommandError::Unknown(key.to_string());
    let field_key = FieldKey::parse(key).map_err(|_| unknown())?;
    let definition = definitions
        .iter()
        .find(|d| d.field_key() == field_key)
        .ok_or_else(unknown)?;
    match read_access(definition, caller) {
        ReadAccess::Visible => {}
        ReadAccess::Redacted => caller.ctx().require(Scope::CustomerDataProfile)?,
        ReadAccess::Hidden => return Err(unknown()),
    }
    if !definition.capabilities().filterable {
        return Err(CommandError::NotFilterable(key.to_string()));
    }
    let invalid = |why: &str| CommandError::InvalidFilter(key.to_string(), why.to_string());
    let field_type = definition.field_type();
    let base = field_type.base();
    let op = match op {
        RawFilterOp::Exists => match operands {
            [one] if one == "true" => FieldFilterOp::Exists(true),
            [one] if one == "false" => FieldFilterOp::Exists(false),
            _ => return Err(invalid("exists takes true or false")),
        },
        RawFilterOp::Eq => {
            if operands.is_empty() {
                return Err(invalid("give at least one value"));
            }
            if field_type.is_list() {
                let items = operands
                    .iter()
                    .map(|o| operand_value(base, definition, o).map(|v| v.to_json()))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|why| invalid(&why))?;
                FieldFilterOp::ContainsAny(items)
            } else {
                let scalars = operands
                    .iter()
                    .map(|o| operand_scalar(base, definition, o))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|why| invalid(&why))?;
                FieldFilterOp::AnyOf(scalars)
            }
        }
        RawFilterOp::Gt | RawFilterOp::Gte | RawFilterOp::Lt | RawFilterOp::Lte => {
            let ranged = matches!(
                base,
                BaseType::Integer
                    | BaseType::Decimal
                    | BaseType::Money
                    | BaseType::Measurement
                    | BaseType::Date
                    | BaseType::DateTime
            );
            if field_type.is_list() || !ranged {
                return Err(invalid(
                    "ranges apply to single numbers, money, measurements and dates",
                ));
            }
            let [operand] = operands else {
                return Err(invalid("a range takes one value"));
            };
            let scalar = operand_scalar(base, definition, operand).map_err(|why| invalid(&why))?;
            match op {
                RawFilterOp::Gt => FieldFilterOp::Gt(scalar),
                RawFilterOp::Gte => FieldFilterOp::Gte(scalar),
                RawFilterOp::Lt => FieldFilterOp::Lt(scalar),
                _ => FieldFilterOp::Lte(scalar),
            }
        }
    };
    Ok(FieldFilter {
        definition: definition.id(),
        key: key.to_string(),
        op,
    })
}

/// A query operand as a value of `base` (as stored), read by type alone.
fn operand_value(
    base: BaseType,
    definition: &CustomFieldDefinition,
    operand: &str,
) -> Result<FieldValue, String> {
    let json = match base {
        BaseType::Integer => operand
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| format!("{operand:?} is not a whole number"))?,
        BaseType::Boolean => match operand {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => return Err(format!("{operand:?} is not true or false")),
        },
        BaseType::Money => {
            let amount = operand
                .parse::<i64>()
                .map_err(|_| format!("{operand:?} is not an amount in minor units"))?;
            return Ok(FieldValue::Integer(amount));
        }
        BaseType::Measurement => {
            let unit_at = operand
                .find(|c: char| c.is_ascii_alphabetic())
                .unwrap_or(operand.len());
            let (number, unit) = operand.split_at(unit_at);
            let unit = if unit.is_empty() {
                definition
                    .validations()
                    .dimension()
                    .map_or("g", |d| d.base_unit().as_str())
            } else {
                unit.trim()
            };
            serde_json::json!({"value": number.trim(), "unit": unit})
        }
        _ => Value::String(operand.to_string()),
    };
    let mut value =
        FieldValue::from_stored(FieldType::single(base), &json).map_err(|problems| {
            problems
                .first()
                .map_or_else(String::new, |p| p.message.clone())
        })?;
    if let (FieldValue::Reference(r), Some(kind)) = (&value, definition.validations().kind())
        && r.kind != kind
    {
        return Err(format!("{operand:?} is not a {} ID", kind.as_str()));
    }
    if let FieldValue::Measurement(m) = &value
        && definition
            .validations()
            .dimension()
            .is_some_and(|d| d != m.unit.dimension())
    {
        return Err(format!("{} isn't a unit of this field", m.unit.as_str()));
    }
    // Text compares lowercased in the index: so does the operand.
    if let FieldValue::Text(t) = &mut value {
        *t = t.to_lowercase();
    }
    Ok(value)
}

/// A query operand in index form: `scalar_text` or `scalar_num`, as the value's.
fn operand_scalar(
    base: BaseType,
    definition: &CustomFieldDefinition,
    operand: &str,
) -> Result<FilterScalar, String> {
    let value = operand_value(base, definition, operand)?;
    let number = |d: Decimal| FilterScalar::Number(d.normalized().to_string());
    Ok(match base {
        BaseType::Integer | BaseType::Money => number(value.scalar_num().unwrap_or(Decimal::ZERO)),
        BaseType::Decimal | BaseType::Measurement | BaseType::Date | BaseType::DateTime => number(
            value
                .scalar_num()
                .ok_or_else(|| format!("{operand:?} is out of range"))?,
        ),
        _ => FilterScalar::Text(value.scalar_text().unwrap_or_default()),
    })
}

/// A key as a `Key`, for callers that build `FieldKey`s from parts.
pub fn field_key(namespace: Namespace, key: &str) -> Result<FieldKey, CommandError> {
    Ok(FieldKey::new(
        namespace,
        Key::new(key).map_err(DefinitionError::from)?,
    ))
}
