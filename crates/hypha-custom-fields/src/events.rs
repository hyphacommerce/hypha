//! What happens to custom fields, as events. Contract types: a renamed or reshaped field is
//! a new `VERSION`, never an edit.

use hypha_core::{
    CategoryId, CustomFieldDefinitionId, CustomerId, Event, OrderId, PendingEvent, ProductId,
    Subject, VariantId,
};
use serde::{Serialize, ser::SerializeMap};
use serde_json::Value;

use crate::{
    definition::CustomFieldDefinition,
    limits::MAX_EVENT_VALUE_BYTES,
    owner::{Owner, OwnerType},
};

pub const CUSTOM_FIELD_DEFINITION: &str = "custom_field_definition";

/// `custom_field_definition.created`.
#[derive(Debug, Serialize)]
pub struct CustomFieldDefinitionCreated {
    pub definition: CustomFieldDefinitionId,
    pub owner_type: &'static str,
    pub namespace: String,
    pub key: String,
}

impl CustomFieldDefinitionCreated {
    pub(crate) fn of(d: &CustomFieldDefinition) -> Self {
        Self {
            definition: d.id(),
            owner_type: d.owner_type().as_str(),
            namespace: d.namespace().to_string(),
            key: d.key().to_string(),
        }
    }
}

impl Event for CustomFieldDefinitionCreated {
    const TYPE: &'static str = "custom_field_definition.created";
    const VERSION: u16 = 1;
}

/// `custom_field_definition.updated`: which of `name`, `description`, `validations`,
/// `access`, `private`, `personal`, `capabilities`, `position` changed.
#[derive(Debug, Serialize)]
pub struct CustomFieldDefinitionUpdated {
    pub definition: CustomFieldDefinitionId,
    pub owner_type: &'static str,
    pub namespace: String,
    pub key: String,
    pub fields: Vec<&'static str>,
}

impl CustomFieldDefinitionUpdated {
    pub(crate) fn of(d: &CustomFieldDefinition, fields: Vec<&'static str>) -> Self {
        Self {
            definition: d.id(),
            owner_type: d.owner_type().as_str(),
            namespace: d.namespace().to_string(),
            key: d.key().to_string(),
            fields,
        }
    }
}

impl Event for CustomFieldDefinitionUpdated {
    const TYPE: &'static str = "custom_field_definition.updated";
    const VERSION: u16 = 1;
}

/// `custom_field_definition.deleted`: with its values.
#[derive(Debug, Serialize)]
pub struct CustomFieldDefinitionDeleted {
    pub definition: CustomFieldDefinitionId,
    pub owner_type: &'static str,
    pub namespace: String,
    pub key: String,
}

impl CustomFieldDefinitionDeleted {
    pub(crate) fn of(d: &CustomFieldDefinition) -> Self {
        Self {
            definition: d.id(),
            owner_type: d.owner_type().as_str(),
            namespace: d.namespace().to_string(),
            key: d.key().to_string(),
        }
    }
}

impl Event for CustomFieldDefinitionDeleted {
    const TYPE: &'static str = "custom_field_definition.deleted";
    const VERSION: u16 = 1;
}

/// One field's change in a `<owner>.custom_fields_updated` event: `{"key":
/// "custom.material", "value": "wool"}` (`null` when removed), `{"key": "custom.tags",
/// "added": [...], "removed": [...]}` for list operations, or `{"key": "custom.birthday"}`
/// alone for personal fields and values over 1 KiB.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldChange {
    pub key: String,
    pub change: ChangeData,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChangeData {
    /// The new value; `Null` when it was removed.
    Value(Value),
    Delta {
        added: Vec<Value>,
        removed: Vec<Value>,
    },
    /// Only the key: personal, or too big to carry.
    KeyOnly,
}

fn small(value: &Value) -> bool {
    value.to_string().len() <= MAX_EVENT_VALUE_BYTES
}

impl FieldChange {
    /// A value set (`Some`) or removed (`None`).
    pub fn value(key: &str, value: Option<Value>, personal: bool) -> Self {
        let value = value.unwrap_or(Value::Null);
        let change = if personal || !small(&value) {
            ChangeData::KeyOnly
        } else {
            ChangeData::Value(value)
        };
        Self {
            key: key.to_string(),
            change,
        }
    }

    /// Items added to and removed from a list.
    pub fn delta(key: &str, added: Vec<Value>, removed: Vec<Value>, personal: bool) -> Self {
        let size = Value::Array(added.clone()).to_string().len()
            + Value::Array(removed.clone()).to_string().len();
        let change = if personal || size > MAX_EVENT_VALUE_BYTES {
            ChangeData::KeyOnly
        } else {
            ChangeData::Delta { added, removed }
        };
        Self {
            key: key.to_string(),
            change,
        }
    }
}

impl Serialize for FieldChange {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("key", &self.key)?;
        match &self.change {
            ChangeData::Value(value) => map.serialize_entry("value", value)?,
            ChangeData::Delta { added, removed } => {
                map.serialize_entry("added", added)?;
                map.serialize_entry("removed", removed)?;
            }
            ChangeData::KeyOnly => {}
        }
        map.end()
    }
}

/// `product.custom_fields_updated`: a product's fields, or one of its variants'
/// (`variant`).
#[derive(Debug, Serialize)]
pub struct ProductCustomFieldsUpdated {
    pub product: ProductId,
    pub variant: Option<VariantId>,
    pub changes: Vec<FieldChange>,
}

impl Event for ProductCustomFieldsUpdated {
    const TYPE: &'static str = "product.custom_fields_updated";
    const VERSION: u16 = 1;
}

/// `category.custom_fields_updated`.
#[derive(Debug, Serialize)]
pub struct CategoryCustomFieldsUpdated {
    pub category: CategoryId,
    pub changes: Vec<FieldChange>,
}

impl Event for CategoryCustomFieldsUpdated {
    const TYPE: &'static str = "category.custom_fields_updated";
    const VERSION: u16 = 1;
}

/// `customer.custom_fields_updated`.
#[derive(Debug, Serialize)]
pub struct CustomerCustomFieldsUpdated {
    pub customer: CustomerId,
    pub changes: Vec<FieldChange>,
}

impl Event for CustomerCustomFieldsUpdated {
    const TYPE: &'static str = "customer.custom_fields_updated";
    const VERSION: u16 = 1;
}

/// `order.custom_fields_updated`.
#[derive(Debug, Serialize)]
pub struct OrderCustomFieldsUpdated {
    pub order: OrderId,
    pub changes: Vec<FieldChange>,
}

impl Event for OrderCustomFieldsUpdated {
    const TYPE: &'static str = "order.custom_fields_updated";
    const VERSION: u16 = 1;
}

/// The one event a write records for `owner`: about the owner (a variant's product),
/// listing every change. `None` when nothing changed: a write that changes nothing records
/// nothing.
pub fn fields_updated(owner: &Owner, changes: Vec<FieldChange>) -> Option<PendingEvent> {
    if changes.is_empty() {
        return None;
    }
    let id = owner.id();
    Some(match owner.kind() {
        OwnerType::Product => PendingEvent::new(
            Subject::new("product", id),
            &ProductCustomFieldsUpdated {
                product: ProductId::from_uuid(id),
                variant: None,
                changes,
            },
        ),
        OwnerType::Variant => {
            // Variants are always built with their product (`Owner::variant`).
            let product = owner
                .product_of_variant()
                .unwrap_or(ProductId::from_uuid(id));
            PendingEvent::new(
                Subject::new("product", product.as_uuid()),
                &ProductCustomFieldsUpdated {
                    product,
                    variant: Some(VariantId::from_uuid(id)),
                    changes,
                },
            )
        }
        OwnerType::Category => PendingEvent::new(
            Subject::new("category", id),
            &CategoryCustomFieldsUpdated {
                category: CategoryId::from_uuid(id),
                changes,
            },
        ),
        OwnerType::Customer => PendingEvent::new(
            Subject::new("customer", id),
            &CustomerCustomFieldsUpdated {
                customer: CustomerId::from_uuid(id),
                changes,
            },
        ),
        OwnerType::Order => PendingEvent::new(
            Subject::new("order", id),
            &OrderCustomFieldsUpdated {
                order: OrderId::from_uuid(id),
                changes,
            },
        ),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn changes_carry_values_deltas_or_only_the_key() {
        let product = ProductId::from_uuid(Uuid::from_u128(1));
        let event = fields_updated(
            &Owner::product(product),
            vec![
                FieldChange::value("custom.material", Some(json!("wool")), false),
                FieldChange::value("custom.care", None, false),
                FieldChange::delta("custom.tags", vec![json!("vip")], vec![], false),
                FieldChange::value("custom.secret", Some(json!("x")), true),
                FieldChange::value("custom.big", Some(json!("a".repeat(1100))), false),
            ],
        )
        .unwrap();
        assert_eq!(event.event_type, "product.custom_fields_updated");
        assert_eq!(
            event.data,
            json!({"product": product.to_string(), "variant": null, "changes": [
                {"key": "custom.material", "value": "wool"},
                {"key": "custom.care", "value": null},
                {"key": "custom.tags", "added": ["vip"], "removed": []},
                {"key": "custom.secret"},
                {"key": "custom.big"},
            ]})
        );
    }

    #[test]
    fn a_variant_reports_on_its_product() {
        let product = ProductId::from_uuid(Uuid::from_u128(1));
        let variant = VariantId::from_uuid(Uuid::from_u128(2));
        let event = fields_updated(
            &Owner::variant(product, variant),
            vec![FieldChange::value("custom.size", Some(json!("M")), false)],
        )
        .unwrap();
        assert_eq!(event.subject, Subject::new("product", product.as_uuid()));
        assert_eq!(event.data["variant"], json!(variant.to_string()));
    }

    #[test]
    fn no_changes_no_event() {
        let owner = Owner::customer(CustomerId::from_uuid(Uuid::from_u128(1)));
        assert_eq!(fields_updated(&owner, vec![]), None);
    }

    #[test]
    fn a_category_gets_its_own_event() {
        let category = CategoryId::from_uuid(Uuid::from_u128(1));
        let event = fields_updated(
            &Owner::category(category),
            vec![FieldChange::value("custom.banner", Some(json!("x")), false)],
        )
        .unwrap();
        assert_eq!(
            (event.event_type, event.subject),
            (
                "category.custom_fields_updated",
                Subject::new("category", category.as_uuid())
            )
        );
        assert_eq!(event.data["category"], json!(category.to_string()));
    }

    #[test]
    fn an_order_gets_its_own_event() {
        let order = OrderId::from_uuid(Uuid::from_u128(1));
        let event = fields_updated(
            &Owner::order(order),
            vec![FieldChange::value("custom.gift", Some(json!(true)), false)],
        )
        .unwrap();
        assert_eq!(
            (event.event_type, event.subject),
            (
                "order.custom_fields_updated",
                Subject::new("order", order.as_uuid())
            )
        );
        assert_eq!(event.data["order"], json!(order.to_string()));
    }
}
