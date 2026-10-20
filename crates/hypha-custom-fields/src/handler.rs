//! `CustomFieldCleanup` (design note 0033, §6.3): values have no foreign key to their
//! owners (they live in other modules' tables) nor to what references point at, so
//! deletions are followed by event. Eventually consistent, like search sync: readers
//! ignore values whose owner doesn't load and tolerate dangling references meanwhile.

use std::collections::BTreeMap;

use hypha_core::{
    AssetId, CategoryId, CustomFieldDefinitionId, CustomerId, EventHandler, HasBatch, OrderId,
    ProductId, RepoError, StageEvents, StoredEvent, VariantId,
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    events::{FieldChange, fields_updated},
    owner::Owner,
    repo::{
        DefinitionRepo, HasCustomFieldDefinitions, HasCustomFieldValues, StageCustomFieldValues,
        StoredValue, ValueRepo, ValueRow,
    },
    types::{BaseType, Reference, ReferenceKind},
    value::FieldValue,
};

/// The deletions it follows. `order.deleted` doesn't exist yet; wanting it costs
/// nothing and covers orders when they can be deleted.
pub mod wanted {
    pub const PRODUCT_DELETED: &str = "product.deleted";
    pub const VARIANT_REMOVED: &str = "product.variant_removed";
    pub const CATEGORY_DELETED: &str = "category.deleted";
    pub const CUSTOMER_DELETED: &str = "customer.deleted";
    pub const ORDER_DELETED: &str = "order.deleted";
    pub const ASSET_DELETED: &str = "asset.deleted";
    pub const ALL: [&str; 6] = [
        PRODUCT_DELETED,
        VARIANT_REMOVED,
        CATEGORY_DELETED,
        CUSTOMER_DELETED,
        ORDER_DELETED,
        ASSET_DELETED,
    ];
}

/// Deletes deleted owners' values and removes references to deleted targets (an item
/// from a list, or the whole value), recording the usual
/// `<owner>.custom_fields_updated` for the latter. Idempotent: a second run finds
/// nothing left to do.
#[derive(Debug, Clone, Copy, Default)]
pub struct CustomFieldCleanup;

/// How many times a reference cleanup reloads after losing a race with a writer.
const ATTEMPTS: u32 = 3;

#[derive(Deserialize)]
struct ProductDeleted {
    product: ProductId,
    #[serde(default)]
    variants: Vec<VariantId>,
}

#[derive(Deserialize)]
struct VariantRemoved {
    variant: VariantId,
}

#[derive(Deserialize)]
struct CategoryDeleted {
    category: CategoryId,
}

#[derive(Deserialize)]
struct CustomerDeleted {
    customer: CustomerId,
}

#[derive(Deserialize)]
struct OrderDeleted {
    order: OrderId,
}

#[derive(Deserialize)]
struct AssetDeleted {
    asset: AssetId,
}

fn payload<T: for<'de> Deserialize<'de>>(event: &StoredEvent) -> Result<T, String> {
    serde_json::from_value(event.data.clone())
        .map_err(|e| format!("{} payload unreadable: {e}", event.event_type))
}

/// What a deletion event means here: owners whose values go, targets whose references go.
fn what_went(event: &StoredEvent) -> Result<(Vec<Uuid>, Vec<Reference>), String> {
    let reference = |kind, id| Reference { kind, id };
    Ok(match event.event_type.as_str() {
        wanted::PRODUCT_DELETED => {
            let p: ProductDeleted = payload(event)?;
            let mut owners = vec![p.product.as_uuid()];
            let mut targets = vec![reference(ReferenceKind::Product, p.product.as_uuid())];
            for v in p.variants {
                owners.push(v.as_uuid());
                targets.push(reference(ReferenceKind::Variant, v.as_uuid()));
            }
            (owners, targets)
        }
        wanted::VARIANT_REMOVED => {
            let v: VariantRemoved = payload(event)?;
            let id = v.variant.as_uuid();
            (vec![id], vec![reference(ReferenceKind::Variant, id)])
        }
        wanted::CATEGORY_DELETED => {
            let c: CategoryDeleted = payload(event)?;
            let id = c.category.as_uuid();
            (vec![id], vec![reference(ReferenceKind::Category, id)])
        }
        wanted::CUSTOMER_DELETED => {
            let c: CustomerDeleted = payload(event)?;
            let id = c.customer.as_uuid();
            (vec![id], vec![reference(ReferenceKind::Customer, id)])
        }
        wanted::ORDER_DELETED => {
            let o: OrderDeleted = payload(event)?;
            (vec![o.order.as_uuid()], Vec::new())
        }
        wanted::ASSET_DELETED => {
            let a: AssetDeleted = payload(event)?;
            (
                Vec::new(),
                vec![reference(ReferenceKind::Asset, a.asset.as_uuid())],
            )
        }
        _ => (Vec::new(), Vec::new()),
    })
}

impl<P> EventHandler<P> for CustomFieldCleanup
where
    P: HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasBatch<Batch: StageCustomFieldValues + StageEvents>,
{
    fn name(&self) -> &'static str {
        "custom_field_cleanup"
    }

    fn wants(&self, event_type: &str) -> bool {
        wanted::ALL.contains(&event_type)
    }

    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String> {
        let (owners, targets) = what_went(event)?;
        if !owners.is_empty() {
            ports
                .custom_field_values()
                .delete_for_owners(&owners)
                .await
                .map_err(|e| e.to_string())?;
        }
        if !targets.is_empty() {
            clear_references(ports, &targets)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Removes references to `targets` from every value of a reference field of their kind,
/// in one batch with an event per owner; reloads and tries again when a writer got
/// there first.
pub async fn clear_references<P>(ports: &P, targets: &[Reference]) -> Result<(), RepoError>
where
    P: HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasBatch<Batch: StageCustomFieldValues + StageEvents>,
{
    let definitions: Vec<_> = ports
        .custom_field_definitions()
        .list(None)
        .await?
        .into_iter()
        .filter(|d| d.field_type().base() == BaseType::Reference)
        .collect();
    let mut attempt = 0;
    loop {
        attempt += 1;
        let mut batch = ports.batch();
        // Every value hit, once, with the targets it holds: a list may hold several.
        let mut hits: BTreeMap<(CustomFieldDefinitionId, Uuid), (StoredValue, Vec<Value>)> =
            BTreeMap::new();
        for target in targets {
            let ids: Vec<_> = definitions
                .iter()
                .filter(|d| d.validations().kind() == Some(target.kind))
                .map(|d| d.id())
                .collect();
            if ids.is_empty() {
                continue;
            }
            for stored in ports
                .custom_field_values()
                .referencing(&ids, &target.to_string())
                .await?
            {
                hits.entry((stored.definition, stored.owner))
                    .or_insert_with(|| (stored, Vec::new()))
                    .1
                    .push(Value::String(target.to_string()));
            }
        }
        // Changes per owner, so each owner gets one event.
        let mut changes: BTreeMap<Uuid, (Owner, Vec<FieldChange>)> = BTreeMap::new();
        let mut staged = false;
        for (stored, gone) in hits.into_values() {
            let Some(definition) = definitions.iter().find(|d| d.id() == stored.definition) else {
                continue;
            };
            let owner =
                Owner::from_parts(definition.owner_type(), stored.owner, stored.owner_parent);
            let key = definition.field_key().to_string();
            let change = match &stored.value {
                Value::Array(items) => {
                    let left: Vec<Value> = items
                        .iter()
                        .filter(|i| !gone.contains(i))
                        .cloned()
                        .collect();
                    if left.len() == items.len() {
                        continue;
                    }
                    if left.is_empty() {
                        batch.remove_custom_field_value(
                            definition.id(),
                            stored.owner,
                            Some(stored.version),
                        );
                    } else {
                        let value =
                            FieldValue::from_stored(definition.field_type(), &Value::Array(left))
                                .map_err(|p| RepoError::Backend(format!("{p:?}")))?;
                        batch.set_custom_field_value(
                            ValueRow::new(definition, &owner, &value),
                            Some(stored.version),
                        );
                    }
                    FieldChange::delta(&key, vec![], gone, definition.personal())
                }
                _ => {
                    batch.remove_custom_field_value(
                        definition.id(),
                        stored.owner,
                        Some(stored.version),
                    );
                    FieldChange::value(&key, None, definition.personal())
                }
            };
            staged = true;
            changes
                .entry(owner.id())
                .or_insert_with(|| (owner, Vec::new()))
                .1
                .push(change);
        }
        if !staged {
            return Ok(());
        }
        let events = changes
            .into_values()
            .filter_map(|(owner, changes)| fields_updated(&owner, changes))
            .collect();
        batch.record_events(events);
        match ports.commit(batch).await {
            Err(RepoError::StaleVersion) if attempt < ATTEMPTS => continue,
            other => return other,
        }
    }
}
