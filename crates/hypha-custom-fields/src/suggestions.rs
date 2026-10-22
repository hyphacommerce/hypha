//! Suggested definitions: the well-known fields the admin offers a merchant in one click
//! and `pnpm seed` creates. They're conventions, not core: a suggestion is an ordinary
//! `DefinitionSpec` for the merchant's `custom` namespace, created through
//! `create_definition` like any other, and a merchant may rename, delete or never create
//! it. Apps read the fields like any other.
//!
//! Customers get `custom.tags` (labels: a `list.text`, unique ignoring case, filterable)
//! and `custom.note` (the staff note: `multiline_text`, `personal`, so it's redacted,
//! erased and exported like the name).

use hypha_core::{Ctx, CustomFieldDefinitionId, Scope};

use crate::{
    commands::CommandError,
    definition::{Access, Capabilities, DefinitionSpec},
    namespace::Namespace,
    owner::OwnerType,
    repo::{DefinitionRepo, HasCustomFieldDefinitions},
    types::{BaseType, FieldType, RawValidations},
};

/// Shopify's limit for a customer note, in characters.
const NOTE_MAX_CHARS: u32 = 5_000;

/// A field worth having, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub spec: DefinitionSpec,
    /// One sentence for staff: what the field is for.
    pub why: &'static str,
}

/// A suggestion next to the merchant's definition of its key, when there is one.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggested {
    pub suggestion: Suggestion,
    pub existing: Option<CustomFieldDefinitionId>,
}

fn list_of(base: BaseType) -> FieldType {
    // Every base type suggested here is listable: a mistake shows up in the tests below.
    FieldType::list_of(base).unwrap_or_else(|_| FieldType::single(base))
}

/// The suggestions for `owner_type`, or every owner type's for `None`, in the order the
/// admin shows them.
pub fn suggestions(owner_type: Option<OwnerType>) -> Vec<Suggestion> {
    let all = [
        Suggestion {
            spec: DefinitionSpec {
                owner_type: OwnerType::Customer,
                key: "tags".to_string(),
                field_type: list_of(BaseType::Text),
                name: "Labels".to_string(),
                description: Some(
                    "Words staff put on a customer to find them again: vip, wholesale, newsletter."
                        .to_string(),
                ),
                validations: RawValidations {
                    unique_items: Some(true),
                    ..RawValidations::default()
                },
                access: Access::default(),
                private: false,
                personal: false,
                capabilities: Capabilities {
                    filterable: true,
                    unique: false,
                },
            },
            why: "Shown under the customer's name, a filter on the customers list, and \
                  what apps like a mailing list sync with.",
        },
        Suggestion {
            spec: DefinitionSpec {
                owner_type: OwnerType::Customer,
                key: "note".to_string(),
                field_type: FieldType::single(BaseType::MultilineText),
                name: "Note".to_string(),
                description: Some("What staff should know about this customer.".to_string()),
                validations: RawValidations {
                    max_length: Some(NOTE_MAX_CHARS),
                    ..RawValidations::default()
                },
                access: Access::default(),
                private: false,
                personal: true,
                capabilities: Capabilities::default(),
            },
            why: "A note card on the customer's page. Personal, so only callers with \
                  customer_data:profile read it, and erasing the customer removes it.",
        },
    ];
    all.into_iter()
        .filter(|s| owner_type.is_none_or(|t| t == s.spec.owner_type))
        .collect()
}

/// The suggestions for `owner_type` (every owner type's for `None`), each with the
/// merchant's `custom` definition of its key when one exists, so the admin offers only
/// what's missing and shows where the rest went.
pub async fn suggested_definitions<S: HasCustomFieldDefinitions>(
    ctx: &Ctx,
    ports: &S,
    owner_type: Option<OwnerType>,
) -> Result<Vec<Suggested>, CommandError> {
    ctx.require(Scope::CustomFieldsRead)?;
    let existing = ports.custom_field_definitions().list(owner_type).await?;
    Ok(suggestions(owner_type)
        .into_iter()
        .map(|suggestion| {
            let existing = existing
                .iter()
                .find(|d| {
                    d.owner_type() == suggestion.spec.owner_type
                        && *d.namespace() == Namespace::Custom
                        && d.key().as_str() == suggestion.spec.key
                })
                .map(|d| d.id());
            Suggested {
                suggestion,
                existing,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use hypha_core::CustomFieldDefinitionId;
    use uuid::Uuid;

    use super::*;
    use crate::definition::CustomFieldDefinition;

    #[test]
    fn every_suggestion_is_a_valid_custom_definition() {
        for (i, s) in suggestions(None).into_iter().enumerate() {
            let id = CustomFieldDefinitionId::from_uuid(Uuid::from_u128(i as u128 + 1));
            let built = CustomFieldDefinition::new(id, Namespace::Custom, s.spec.clone(), 0);
            assert!(built.is_ok(), "{}: {:?}", s.spec.key, built.err());
        }
    }

    #[test]
    fn keys_are_unique_per_owner_type() {
        let all = suggestions(None);
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(
                    a.spec.owner_type != b.spec.owner_type || a.spec.key != b.spec.key,
                    "{} suggested twice",
                    a.spec.key
                );
            }
        }
    }

    #[test]
    fn filtered_by_owner_type() {
        let customers = suggestions(Some(OwnerType::Customer));
        let keys: Vec<&str> = customers.iter().map(|s| s.spec.key.as_str()).collect();
        assert_eq!(keys, ["tags", "note"]);
        assert!(suggestions(Some(OwnerType::Product)).is_empty());
    }

    #[test]
    fn the_note_is_personal_and_the_labels_are_not() {
        let by_key = |key: &str| {
            suggestions(Some(OwnerType::Customer))
                .into_iter()
                .find(|s| s.spec.key == key)
                .unwrap()
        };
        assert!(by_key("note").spec.personal);
        assert!(!by_key("tags").spec.personal);
        assert!(by_key("tags").spec.capabilities.filterable);
        assert_eq!(by_key("tags").spec.field_type.to_string(), "list.text");
    }
}
