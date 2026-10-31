//! The custom fields an app declares in its manifest: always in the app's own namespace
//! (`app_<name>.<key>`), created when staff approve the app and updated when they approve a
//! refreshed manifest.
//!
//! This crate is a leaf and can't see `hypha-custom-fields`, so it holds its own plain,
//! checked description of each field (`AppCustomField`); the host turns it into the
//! custom fields crate's definitions. The rules checked here are the manifest's: the
//! key's shape, the type's name, lengths, duplicates, the per-owner limit, `personal`
//! only on customers and orders, `unique` only on text, integer and url fields, and
//! that a type never changes once installed (`check_retypes`). The validations object
//! is the custom fields crate's to check when the host syncs.

use serde::{Deserialize, Serialize};

use crate::manifest::{AppManifest, Checker, ManifestError, ManifestProblem};

/// The scope a manifest with a personal field must ask for. Matched by name, so the check
/// doesn't depend on core's `Scope` naming it yet.
pub const PROFILE_SCOPE: &str = "customer_data:profile";

/// The most fields a manifest declares per owner type (definitions per owner type, per
/// namespace).
pub const MAX_FIELDS_PER_OWNER: usize = 128;
pub const MAX_FIELD_NAME_LEN: usize = 255;
pub const MAX_FIELD_DESCRIPTION_LEN: usize = 1000;

// Strings rather than enums, so a wrong value is reported with its path
// (`custom_fields[2].type`) like every other rule; the schema still lists the values.
/// A custom field the app keeps on Hypha's objects, in its own namespace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ManifestCustomField {
    /// What the field is on.
    #[cfg_attr(test, schemars(with = "CustomFieldOwnerType"))]
    pub owner_type: String,
    /// `[a-z][a-z0-9_]{1,63}`: addressed as `app_<name>.<key>`. Unique per owner type.
    pub key: String,
    /// The value's type (`text`, `choice`, `list.text`, …). Can't change once installed.
    #[serde(rename = "type")]
    #[cfg_attr(test, schemars(with = "TypeName"))]
    pub field_type: String,
    /// Its label on forms, at most 255 characters.
    pub name: String,
    /// Help text on forms, at most 1,000 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The type's validations (`max_length`, `choices`, …).
    #[serde(default = "empty_object", skip_serializing_if = "is_empty_object")]
    #[cfg_attr(test, schemars(with = "serde_json::Map<String, serde_json::Value>"))]
    pub validations: serde_json::Value,
    /// Who besides the app reads or writes the values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<ManifestFieldAccess>,
    /// Only the app and staff read the values (other apps don't, even with the owner's
    /// read scope).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
    /// The value is about a person: only on `customer` and `order` fields, and the
    /// manifest must ask for `customer_data:profile`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub personal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ManifestFieldCapabilities>,
}

fn empty_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

fn is_empty_object(v: &serde_json::Value) -> bool {
    v.as_object().is_some_and(|m| m.is_empty())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ManifestFieldAccess {
    /// `read` (the default) or `read_write`: whether staff may write the values too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, schemars(with = "Option<AdminAccess>"))]
    pub admin: Option<String>,
    /// `none` (the default) or `read`: whether storefronts see the values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, schemars(with = "Option<StorefrontAccess>"))]
    pub storefront: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ManifestFieldCapabilities {
    /// Lists can filter on it.
    #[serde(default)]
    pub filterable: bool,
    /// No two owners hold the same value (text, integer and url fields).
    #[serde(default)]
    pub unique: bool,
}

/// What a field is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CustomFieldOwnerType {
    Product,
    Variant,
    // A product category.
    Category,
    Customer,
    Order,
}

impl CustomFieldOwnerType {
    pub const ALL: [CustomFieldOwnerType; 5] = [
        Self::Product,
        Self::Variant,
        Self::Category,
        Self::Customer,
        Self::Order,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Product => "product",
            Self::Variant => "variant",
            Self::Category => "category",
            Self::Customer => "customer",
            Self::Order => "order",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.as_str() == s)
    }

    /// Whether its fields may be `personal`: only customers' and orders'.
    pub fn allows_personal(self) -> bool {
        matches!(self, Self::Customer | Self::Order)
    }
}

impl std::fmt::Display for CustomFieldOwnerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A value type without its list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseFieldType {
    Text,
    MultilineText,
    RichText,
    Integer,
    Decimal,
    Boolean,
    Date,
    DateTime,
    Url,
    Money,
    Measurement,
    Choice,
    Reference,
    Json,
}

impl BaseFieldType {
    pub const ALL: [BaseFieldType; 14] = [
        Self::Text,
        Self::MultilineText,
        Self::RichText,
        Self::Integer,
        Self::Decimal,
        Self::Boolean,
        Self::Date,
        Self::DateTime,
        Self::Url,
        Self::Money,
        Self::Measurement,
        Self::Choice,
        Self::Reference,
        Self::Json,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::MultilineText => "multiline_text",
            Self::RichText => "rich_text",
            Self::Integer => "integer",
            Self::Decimal => "decimal",
            Self::Boolean => "boolean",
            Self::Date => "date",
            Self::DateTime => "date_time",
            Self::Url => "url",
            Self::Money => "money",
            Self::Measurement => "measurement",
            Self::Choice => "choice",
            Self::Reference => "reference",
            Self::Json => "json",
        }
    }

    /// Whether `list.<t>` exists: every type but boolean, rich and multiline text, and
    /// JSON.
    pub fn listable(self) -> bool {
        !matches!(
            self,
            Self::Boolean | Self::RichText | Self::MultilineText | Self::Json
        )
    }
}

/// A field's type: one value, or a list of them (`list.text`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CustomFieldType {
    One(BaseFieldType),
    List(BaseFieldType),
}

impl CustomFieldType {
    pub fn parse(s: &str) -> Option<Self> {
        let base = |name: &str| BaseFieldType::ALL.into_iter().find(|t| t.as_str() == name);
        match s.strip_prefix("list.") {
            Some(inner) => base(inner)
                .filter(|t| t.listable())
                .map(CustomFieldType::List),
            None => base(s).map(CustomFieldType::One),
        }
    }

    /// Whether `capabilities.unique` is allowed: a single text, integer or url.
    pub fn may_be_unique(self) -> bool {
        matches!(
            self,
            Self::One(BaseFieldType::Text | BaseFieldType::Integer | BaseFieldType::Url)
        )
    }
}

impl std::fmt::Display for CustomFieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::One(t) => f.write_str(t.as_str()),
            Self::List(t) => write!(f, "list.{}", t.as_str()),
        }
    }
}

/// Whether staff may write an app's field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AdminAccess {
    #[default]
    Read,
    ReadWrite,
}

/// Whether storefronts see an app's field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StorefrontAccess {
    #[default]
    None,
    Read,
}

/// A custom field's type: one value, or `list.<type>`.
// For the JSON Schema only.
#[cfg(test)]
#[derive(schemars::JsonSchema)]
#[allow(dead_code, non_camel_case_types)]
enum TypeName {
    text,
    multiline_text,
    rich_text,
    integer,
    decimal,
    boolean,
    date,
    date_time,
    url,
    money,
    measurement,
    choice,
    reference,
    json,
    #[serde(rename = "list.text")]
    list_text,
    #[serde(rename = "list.integer")]
    list_integer,
    #[serde(rename = "list.decimal")]
    list_decimal,
    #[serde(rename = "list.date")]
    list_date,
    #[serde(rename = "list.date_time")]
    list_date_time,
    #[serde(rename = "list.url")]
    list_url,
    #[serde(rename = "list.money")]
    list_money,
    #[serde(rename = "list.measurement")]
    list_measurement,
    #[serde(rename = "list.choice")]
    list_choice,
    #[serde(rename = "list.reference")]
    list_reference,
}

/// A checked custom field of an app: what the host syncs into a definition in the
/// app's namespace.
#[derive(Debug, Clone, PartialEq)]
pub struct AppCustomField {
    pub owner_type: CustomFieldOwnerType,
    pub key: String,
    pub field_type: CustomFieldType,
    pub name: String,
    pub description: Option<String>,
    /// An object; the custom fields crate checks it against the type.
    pub validations: serde_json::Map<String, serde_json::Value>,
    pub admin: AdminAccess,
    pub storefront: StorefrontAccess,
    pub private: bool,
    pub personal: bool,
    pub filterable: bool,
    pub unique: bool,
}

/// `[a-z][a-z0-9_]{1,63}`.
fn is_field_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && (2..=64).contains(&key.len())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Checks the manifest's `custom_fields`, adding every problem to `c`, and returns the
/// fields that passed (all of them when nothing was added).
pub(crate) fn check(
    c: &mut Checker,
    fields: &[ManifestCustomField],
    scopes: &[String],
) -> Vec<AppCustomField> {
    let mut checked = Vec::new();
    let mut seen: Vec<(CustomFieldOwnerType, &str)> = Vec::new();
    let mut per_owner = [0usize; CustomFieldOwnerType::ALL.len()];
    let mut personal_at = None;
    for (n, field) in fields.iter().enumerate() {
        let path = format!("custom_fields[{n}]");
        let before = c.problems.len();

        let owner = CustomFieldOwnerType::parse(&field.owner_type);
        if owner.is_none() {
            c.add(
                &format!("{path}.owner_type"),
                format!(
                    "{:?} is not an owner type (product, variant, category, customer, order)",
                    field.owner_type
                ),
            );
        }
        if !is_field_key(&field.key) {
            c.add(
                &format!("{path}.key"),
                "is 2 to 64 lowercase letters, digits or `_`, starting with a letter",
            );
        } else if let Some(owner) = owner {
            if seen.contains(&(owner, field.key.as_str())) {
                c.add(
                    &format!("{path}.key"),
                    format!("{owner} field {:?} is declared twice", field.key),
                );
            }
            seen.push((owner, field.key.as_str()));
        }
        if let Some(owner) = owner {
            per_owner[owner as usize] += 1;
        }

        let field_type = CustomFieldType::parse(&field.field_type);
        if field_type.is_none() {
            c.add(
                &format!("{path}.type"),
                format!("{:?} is not a custom field type", field.field_type),
            );
        }
        c.text(&format!("{path}.name"), &field.name, MAX_FIELD_NAME_LEN);
        if let Some(description) = &field.description {
            c.text(
                &format!("{path}.description"),
                description,
                MAX_FIELD_DESCRIPTION_LEN,
            );
        }
        let validations = field.validations.as_object();
        if validations.is_none() {
            c.add(&format!("{path}.validations"), "is an object");
        }

        let access = field.access.as_ref();
        let admin = match access.and_then(|a| a.admin.as_deref()) {
            None | Some("read") => Some(AdminAccess::Read),
            Some("read_write") => Some(AdminAccess::ReadWrite),
            Some(other) => {
                c.add(
                    &format!("{path}.access.admin"),
                    format!("{other:?} is `read` or `read_write`"),
                );
                None
            }
        };
        let storefront = match access.and_then(|a| a.storefront.as_deref()) {
            None | Some("none") => Some(StorefrontAccess::None),
            Some("read") => Some(StorefrontAccess::Read),
            Some(other) => {
                c.add(
                    &format!("{path}.access.storefront"),
                    format!("{other:?} is `none` or `read`"),
                );
                None
            }
        };

        if field.personal {
            if owner.is_some_and(|o| !o.allows_personal()) {
                c.add(
                    &format!("{path}.personal"),
                    "is only for customer and order fields",
                );
            }
            personal_at.get_or_insert(n);
        }
        let capabilities = field.capabilities.clone().unwrap_or_default();
        if capabilities.unique && field_type.is_some_and(|t| !t.may_be_unique()) {
            c.add(
                &format!("{path}.capabilities.unique"),
                "is only for text, integer and url fields",
            );
        }

        if c.problems.len() == before
            && let (Some(owner_type), Some(field_type), Some(validations), Some(admin), Some(sf)) =
                (owner, field_type, validations, admin, storefront)
        {
            checked.push(AppCustomField {
                owner_type,
                key: field.key.clone(),
                field_type,
                name: field.name.clone(),
                description: field.description.clone(),
                validations: validations.clone(),
                admin,
                storefront: sf,
                private: field.private,
                personal: field.personal,
                filterable: capabilities.filterable,
                unique: capabilities.unique,
            });
        }
    }
    for owner in CustomFieldOwnerType::ALL {
        let count = per_owner[owner as usize];
        if count > MAX_FIELDS_PER_OWNER {
            c.add(
                "custom_fields",
                format!(
                    "declares at most {MAX_FIELDS_PER_OWNER} fields per owner type ({owner} has {count})"
                ),
            );
        }
    }
    if let Some(n) = personal_at
        && !scopes.iter().any(|s| s == PROFILE_SCOPE)
    {
        c.add(
            "scopes",
            format!("lists {PROFILE_SCOPE}: custom_fields[{n}] is personal"),
        );
    }
    checked
}

/// A field as the consent screen names it.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomFieldSummary {
    pub owner_type: CustomFieldOwnerType,
    pub key: String,
    pub name: String,
    pub field_type: CustomFieldType,
    pub personal: bool,
}

impl From<&AppCustomField> for CustomFieldSummary {
    fn from(f: &AppCustomField) -> Self {
        Self {
            owner_type: f.owner_type,
            key: f.key.clone(),
            name: f.name.clone(),
            field_type: f.field_type,
            personal: f.personal,
        }
    }
}

/// A field whose type a manifest changed: such a manifest can't be applied (the type is
/// part of a field's identity).
#[derive(Debug, Clone, PartialEq)]
pub struct RetypedField {
    pub owner_type: CustomFieldOwnerType,
    pub key: String,
    pub name: String,
    pub from: CustomFieldType,
    pub to: CustomFieldType,
}

/// What applying a manifest does to the app's fields, compared with the manifest it
/// has: for the consent screen ("adds fields to …", "removes fields: …"). Each list
/// is in the new (or, for `removed`, the old) manifest's order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomFieldChanges {
    /// Declared now and not before (everything, on a first install).
    pub added: Vec<CustomFieldSummary>,
    /// Declared before and still, with the same type.
    pub kept: Vec<CustomFieldSummary>,
    /// Declared before and gone: applying the manifest deletes them with their values.
    pub removed: Vec<CustomFieldSummary>,
    /// Declared before with another type: the manifest is refused when applied.
    pub retyped: Vec<RetypedField>,
}

/// What `next` changes in the fields `previous` (the app's installed manifest; `None`
/// on a first install) declared.
pub fn custom_field_changes(
    previous: Option<&AppManifest>,
    next: &AppManifest,
) -> CustomFieldChanges {
    let before = previous.map(AppManifest::custom_fields).unwrap_or(&[]);
    let find = |fields: &'_ [AppCustomField], f: &AppCustomField| {
        fields
            .iter()
            .find(|o| o.owner_type == f.owner_type && o.key == f.key)
            .map(|o| o.field_type)
    };
    let mut changes = CustomFieldChanges::default();
    for field in next.custom_fields() {
        match find(before, field) {
            None => changes.added.push(field.into()),
            Some(t) if t == field.field_type => changes.kept.push(field.into()),
            Some(from) => changes.retyped.push(RetypedField {
                owner_type: field.owner_type,
                key: field.key.clone(),
                name: field.name.clone(),
                from,
                to: field.field_type,
            }),
        }
    }
    for field in before {
        if find(next.custom_fields(), field).is_none() {
            changes.removed.push(field.into());
        }
    }
    changes
}

/// Refuses `next` when it changes the type of a field `previous` declared, with a
/// problem at that field's `type` (`custom_fields[i].type`).
pub fn check_retypes(previous: &AppManifest, next: &AppManifest) -> Result<(), ManifestError> {
    let problems: Vec<ManifestProblem> = next
        .custom_fields()
        .iter()
        .enumerate()
        .filter_map(|(n, field)| {
            let old = previous
                .custom_fields()
                .iter()
                .find(|o| o.owner_type == field.owner_type && o.key == field.key)?;
            (old.field_type != field.field_type).then(|| ManifestProblem {
                path: format!("custom_fields[{n}].type"),
                message: format!(
                    "was {}: a field's type can't change (declare a new key)",
                    old.field_type
                ),
            })
        })
        .collect();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(ManifestError { problems })
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{Actor, AppId, Sealed, Timestamp, UserId};
    use serde_json::{Value, json};
    use uuid::Uuid;

    use super::*;
    use crate::{App, Grant, InstalledManifest};

    fn manifest(fields: Value) -> Value {
        json!({
            "key": "app_mailchimp",
            "name": "Mailchimp",
            "version": "1.0.0",
            "app_url": "https://mailchimp-app.example.com",
            "redirect_uris": ["/callback"],
            "scopes": ["customers:read"],
            "privacy_url": "/privacy",
            "custom_fields": fields
        })
    }

    fn member_id() -> Value {
        json!({ "owner_type": "customer", "key": "member_id", "type": "text",
                "name": "Mailchimp member",
                "access": { "admin": "read", "storefront": "none" }, "private": false,
                "capabilities": { "unique": true, "filterable": true } })
    }

    fn parse(value: Value) -> Result<AppManifest, ManifestError> {
        AppManifest::parse(&serde_json::to_vec(&value).unwrap())
    }

    fn paths(value: Value) -> Vec<(String, String)> {
        parse(value)
            .unwrap_err()
            .problems
            .into_iter()
            .map(|p| (p.path, p.message))
            .collect()
    }

    fn one(path: &str, message: &str) -> Vec<(String, String)> {
        vec![(path.to_string(), message.to_string())]
    }

    #[test]
    fn a_declared_field_is_checked_and_typed_with_its_defaults() {
        let m = parse(manifest(json!([
            member_id(),
            { "owner_type": "product", "key": "sizes", "type": "list.choice", "name": "Sizes",
              "validations": { "choices": [{ "key": "s", "label": "S" }] } }
        ])))
        .unwrap();
        let fields = m.custom_fields();
        assert_eq!(
            fields[0],
            AppCustomField {
                owner_type: CustomFieldOwnerType::Customer,
                key: "member_id".to_string(),
                field_type: CustomFieldType::One(BaseFieldType::Text),
                name: "Mailchimp member".to_string(),
                description: None,
                validations: serde_json::Map::new(),
                admin: AdminAccess::Read,
                storefront: StorefrontAccess::None,
                private: false,
                personal: false,
                filterable: true,
                unique: true,
            }
        );
        assert_eq!(
            fields[1].field_type,
            CustomFieldType::List(BaseFieldType::Choice)
        );
        assert_eq!(fields[1].admin, AdminAccess::Read);
        assert_eq!(fields[1].storefront, StorefrontAccess::None);
        assert!(!fields[1].unique && !fields[1].filterable);
    }

    #[test]
    fn a_manifest_without_fields_stays_as_it_was_written() {
        let m = parse(manifest(json!([]))).unwrap();
        assert!(m.custom_fields().is_empty());
        assert!(m.to_value().get("custom_fields").is_none());
        let m = parse(manifest(json!([member_id()]))).unwrap();
        assert_eq!(AppManifest::from_value(m.to_value()), Ok(m));
    }

    #[test]
    fn every_type_name_parses_and_round_trips() {
        for t in BaseFieldType::ALL {
            let one = CustomFieldType::parse(t.as_str()).unwrap();
            assert_eq!(one.to_string(), t.as_str());
            let list = format!("list.{}", t.as_str());
            assert_eq!(
                CustomFieldType::parse(&list).map(|l| l.to_string()),
                t.listable().then_some(list.clone()),
                "{list}"
            );
        }
        for bad in [
            "",
            "list.",
            "list.list.text",
            "Text",
            "list.boolean",
            "color",
        ] {
            assert_eq!(CustomFieldType::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn owner_type_key_and_type_are_reported_at_their_paths() {
        let found = paths(manifest(json!([
            member_id(),
            { "owner_type": "market", "key": "x", "type": "colour", "name": "" }
        ])));
        assert_eq!(
            found,
            vec![
                (
                    "custom_fields[1].owner_type".to_string(),
                    "\"market\" is not an owner type (product, variant, category, customer, order)"
                        .to_string()
                ),
                (
                    "custom_fields[1].key".to_string(),
                    "is 2 to 64 lowercase letters, digits or `_`, starting with a letter"
                        .to_string()
                ),
                (
                    "custom_fields[1].type".to_string(),
                    "\"colour\" is not a custom field type".to_string()
                ),
                (
                    "custom_fields[1].name".to_string(),
                    "is 1 to 255 characters".to_string()
                ),
            ]
        );
    }

    #[test]
    fn keys_are_two_to_sixty_four_characters_starting_with_a_letter() {
        assert!(is_field_key("ab"));
        assert!(is_field_key("member_id_2"));
        assert!(is_field_key(&"a".repeat(64)));
        for bad in ["a", "1ab", "_ab", "Ab", "a-b", "a.b", &"a".repeat(65)] {
            assert!(!is_field_key(bad), "{bad}");
        }
    }

    #[test]
    fn name_and_description_have_limits() {
        let mut f = member_id();
        f["name"] = json!("n".repeat(256));
        f["description"] = json!("d".repeat(1001));
        assert_eq!(
            paths(manifest(json!([f]))),
            vec![
                (
                    "custom_fields[0].name".to_string(),
                    "is 1 to 255 characters".to_string()
                ),
                (
                    "custom_fields[0].description".to_string(),
                    "is 1 to 1000 characters".to_string()
                ),
            ]
        );
        let mut f = member_id();
        f["name"] = json!("n".repeat(255));
        f["description"] = json!("d".repeat(1000));
        assert!(parse(manifest(json!([f]))).is_ok());
    }

    #[test]
    fn validations_are_an_object() {
        let mut f = member_id();
        f["validations"] = json!([1]);
        assert_eq!(
            paths(manifest(json!([f]))),
            one("custom_fields[0].validations", "is an object")
        );
    }

    #[test]
    fn access_takes_only_its_values() {
        let mut f = member_id();
        f["access"] = json!({ "admin": "write", "storefront": "read_write" });
        assert_eq!(
            paths(manifest(json!([f]))),
            vec![
                (
                    "custom_fields[0].access.admin".to_string(),
                    "\"write\" is `read` or `read_write`".to_string()
                ),
                (
                    "custom_fields[0].access.storefront".to_string(),
                    "\"read_write\" is `none` or `read`".to_string()
                ),
            ]
        );
        let mut f = member_id();
        f["access"] = json!({ "admin": "read_write", "storefront": "read" });
        let m = parse(manifest(json!([f]))).unwrap();
        assert_eq!(m.custom_fields()[0].admin, AdminAccess::ReadWrite);
        assert_eq!(m.custom_fields()[0].storefront, StorefrontAccess::Read);
    }

    #[test]
    fn a_key_is_declared_once_per_owner_type() {
        let mut on_orders = member_id();
        on_orders["owner_type"] = json!("order");
        assert!(parse(manifest(json!([member_id(), on_orders]))).is_ok());
        assert_eq!(
            paths(manifest(json!([member_id(), member_id()]))),
            one(
                "custom_fields[1].key",
                "customer field \"member_id\" is declared twice"
            )
        );
    }

    #[test]
    fn at_most_128_fields_per_owner_type() {
        let fields = |n: usize, owner: &str| -> Vec<Value> {
            (0..n)
                .map(|i| json!({ "owner_type": owner, "key": format!("f{i}"), "type": "integer", "name": "F" }))
                .collect()
        };
        let mut ok = fields(128, "product");
        ok.extend(fields(128, "variant"));
        assert!(parse(manifest(json!(ok))).is_ok());
        assert_eq!(
            paths(manifest(json!(fields(129, "product")))),
            one(
                "custom_fields",
                "declares at most 128 fields per owner type (product has 129)"
            )
        );
    }

    #[test]
    fn personal_is_only_for_customers_and_orders() {
        let f = json!({ "owner_type": "product", "key": "gift", "type": "text",
                        "name": "Gift", "personal": true });
        let found = paths(manifest(json!([f])));
        assert_eq!(
            found[0],
            (
                "custom_fields[0].personal".to_string(),
                "is only for customer and order fields".to_string()
            )
        );
    }

    #[test]
    fn a_personal_field_needs_the_profile_scope() {
        let f = json!({ "owner_type": "order", "key": "gift_message", "type": "multiline_text",
                        "name": "Gift message", "personal": true });
        assert_eq!(
            paths(manifest(json!([member_id(), f]))),
            one(
                "scopes",
                "lists customer_data:profile: custom_fields[1] is personal"
            )
        );
        // With the scope asked for (and, once core names it, its reason), that problem
        // is gone; whatever else is reported is about the scope itself.
        let mut m = manifest(json!([f]));
        m["scopes"] = json!(["customers:read", PROFILE_SCOPE]);
        m["scope_reasons"] = json!({ PROFILE_SCOPE: "Gift messages are printed on the slip" });
        let problems = parse(m).err().map(|e| e.problems).unwrap_or_default();
        assert!(
            problems.iter().all(|p| p.path.starts_with("scope")
                && p.path != "scopes"
                && !p.message.contains("personal")),
            "{problems:?}"
        );
    }

    #[test]
    fn unique_is_only_for_single_text_integer_and_url_fields() {
        for (t, allowed) in [
            ("text", true),
            ("integer", true),
            ("url", true),
            ("decimal", false),
            ("choice", false),
            ("list.text", false),
        ] {
            let mut f = member_id();
            f["type"] = json!(t);
            let result = parse(manifest(json!([f])));
            if allowed {
                assert!(result.is_ok(), "{t}");
            } else {
                assert_eq!(
                    paths(manifest(
                        json!([{ "owner_type": "customer", "key": "member_id",
                        "type": t, "name": "M", "capabilities": { "unique": true } }])
                    )),
                    one(
                        "custom_fields[0].capabilities.unique",
                        "is only for text, integer and url fields"
                    ),
                    "{t}"
                );
            }
        }
    }

    #[test]
    fn a_namespace_or_an_unknown_field_is_refused() {
        let mut f = member_id();
        f["namespace"] = json!("custom");
        let found = paths(manifest(json!([f])));
        assert_eq!(found[0].0, "");
        assert!(
            found[0].1.contains("unknown field `namespace`"),
            "{found:?}"
        );
    }

    fn summary(
        owner: CustomFieldOwnerType,
        key: &str,
        name: &str,
        t: CustomFieldType,
    ) -> CustomFieldSummary {
        CustomFieldSummary {
            owner_type: owner,
            key: key.to_string(),
            name: name.to_string(),
            field_type: t,
            personal: false,
        }
    }

    #[test]
    fn changes_list_added_kept_removed_and_retyped_fields() {
        let text = CustomFieldType::One(BaseFieldType::Text);
        let old = parse(manifest(json!([
            member_id(),
            { "owner_type": "order", "key": "erp_id", "type": "text", "name": "ERP" },
            { "owner_type": "product", "key": "rating", "type": "integer", "name": "Rating" }
        ])))
        .unwrap();
        let new = parse(manifest(json!([
            member_id(),
            { "owner_type": "product", "key": "rating", "type": "decimal", "name": "Rating" },
            { "owner_type": "variant", "key": "sku_ref", "type": "text", "name": "SKU ref" }
        ])))
        .unwrap();

        let first = custom_field_changes(None, &old);
        assert_eq!(first.added.len(), 3);
        assert!(first.kept.is_empty() && first.removed.is_empty() && first.retyped.is_empty());

        assert_eq!(
            custom_field_changes(Some(&old), &new),
            CustomFieldChanges {
                added: vec![summary(
                    CustomFieldOwnerType::Variant,
                    "sku_ref",
                    "SKU ref",
                    text
                )],
                kept: vec![summary(
                    CustomFieldOwnerType::Customer,
                    "member_id",
                    "Mailchimp member",
                    text
                )],
                removed: vec![summary(CustomFieldOwnerType::Order, "erp_id", "ERP", text)],
                retyped: vec![RetypedField {
                    owner_type: CustomFieldOwnerType::Product,
                    key: "rating".to_string(),
                    name: "Rating".to_string(),
                    from: CustomFieldType::One(BaseFieldType::Integer),
                    to: CustomFieldType::One(BaseFieldType::Decimal),
                }],
            }
        );
    }

    #[test]
    fn a_field_is_its_owner_type_and_key() {
        let old = parse(manifest(json!([member_id()]))).unwrap();
        let mut moved = member_id();
        moved["owner_type"] = json!("order");
        let new = parse(manifest(json!([moved]))).unwrap();
        let changes = custom_field_changes(Some(&old), &new);
        assert_eq!(changes.added[0].owner_type, CustomFieldOwnerType::Order);
        assert_eq!(
            changes.removed[0].owner_type,
            CustomFieldOwnerType::Customer
        );
        assert_eq!(check_retypes(&old, &new), Ok(()));
    }

    fn installed(manifest: AppManifest) -> InstalledManifest {
        InstalledManifest {
            url: "https://mailchimp-app.example.com/hypha-app.json".to_string(),
            manifest,
            fetched_at: Timestamp::from_unix_micros(1_800_000_000_000_000),
        }
    }

    #[test]
    fn applying_a_manifest_that_retypes_a_field_is_refused_at_its_type() {
        let old = parse(manifest(json!([
            { "owner_type": "order", "key": "erp_id", "type": "text", "name": "ERP" },
            member_id()
        ])))
        .unwrap();
        let mut retyped = member_id();
        retyped["type"] = json!("integer");
        let new = parse(manifest(json!([retyped]))).unwrap();
        let mut app = App::install(
            AppId::from_uuid(Uuid::from_u128(1)),
            Sealed::from_bytes(vec![1]),
            installed(old),
            Grant::new(
                &[],
                Actor::User(UserId::from_uuid(Uuid::from_u128(2))),
                Timestamp::from_unix_micros(1_800_000_000_000_000),
            ),
        )
        .unwrap();
        assert_eq!(
            app.apply_manifest(installed(new)),
            Err(crate::AppError::Manifest(ManifestError {
                problems: vec![ManifestProblem {
                    path: "custom_fields[0].type".to_string(),
                    message: "was text: a field's type can't change (declare a new key)"
                        .to_string(),
                }],
            }))
        );
        // Dropping a field, or keeping its type, applies.
        let kept = parse(manifest(json!([member_id()]))).unwrap();
        assert_eq!(app.apply_manifest(installed(kept)), Ok(()));
    }
}
