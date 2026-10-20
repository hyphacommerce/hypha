//! `CustomFieldDefinition` (design note 0033, §2): what a field is, on which owner
//! type, in which namespace, and who sees it. Identity (owner type, namespace, key,
//! type) is immutable (CF3); the rest changes through `change`.

use hypha_core::{
    CustomFieldDefinitionId, PendingEvent, Recorded, Recorder, Stamped, Stamps, Subject, TextError,
    Version, Versioned, required_text,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::DefinitionError,
    events::{
        CUSTOM_FIELD_DEFINITION, CustomFieldDefinitionCreated, CustomFieldDefinitionDeleted,
        CustomFieldDefinitionUpdated,
    },
    limits::{MAX_DESCRIPTION_CHARS, MAX_NAME_CHARS},
    namespace::{FieldKey, Key, Namespace},
    owner::OwnerType,
    types::{FieldType, RawValidations, Validations},
};

/// What staff may do with an app's or a spore's field in the admin (§5.1). The
/// merchant's own `custom` fields are always `read_write`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminAccess {
    Read,
    ReadWrite,
}

/// Whether the storefront surface sees the field (opt-in, CF8). `read_write` is reserved
/// for cart fields (§11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorefrontAccess {
    None,
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Access {
    pub admin: AdminAccess,
    pub storefront: StorefrontAccess,
}

impl Default for Access {
    fn default() -> Self {
        Self {
            admin: AdminAccess::ReadWrite,
            storefront: StorefrontAccess::None,
        }
    }
}

/// What a field's values can be used for (§7).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// Indexed, and a filter on its owner's list (`field[custom.material]=wool`).
    #[serde(default)]
    pub filterable: bool,
    /// No two owners hold the same value (case-insensitively): text, integer, url.
    #[serde(default)]
    pub unique: bool,
}

/// What a definition is made from: the API's body for `custom`, a manifest's entry or a
/// spore's `FieldSpec` for the others (their namespace comes from who declares them).
#[derive(Debug, Clone, PartialEq)]
pub struct DefinitionSpec {
    pub owner_type: OwnerType,
    pub key: String,
    pub field_type: FieldType,
    pub name: String,
    pub description: Option<String>,
    pub validations: RawValidations,
    pub access: Access,
    /// Only the namespace's app (and staff) read it (§5.1); app and spore namespaces only.
    pub private: bool,
    /// About a person (§5.2); customer and order owners only.
    pub personal: bool,
    pub capabilities: Capabilities,
}

/// Changes only what's `Some` (`description: Some(None)` clears it), like a JSON Merge
/// Patch.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DefinitionChange {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub validations: Option<RawValidations>,
    pub access: Option<Access>,
    pub private: Option<bool>,
    pub personal: Option<bool>,
    pub capabilities: Option<Capabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomFieldDefinition {
    id: CustomFieldDefinitionId,
    version: Version,
    stamps: Option<Stamps>,
    owner_type: OwnerType,
    namespace: Namespace,
    key: Key,
    name: String,
    description: Option<String>,
    field_type: FieldType,
    validations: Validations,
    access: Access,
    private: bool,
    personal: bool,
    capabilities: Capabilities,
    position: u32,
    events: Recorder,
}

fn name_of(name: &str) -> Result<String, DefinitionError> {
    let name = required_text("name", name)?;
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(TextError::TooLong("name").into());
    }
    Ok(name)
}

fn description_of(description: Option<String>) -> Result<Option<String>, DefinitionError> {
    let Some(text) = description else {
        return Ok(None);
    };
    let text = text.trim();
    if text.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(DefinitionError::DescriptionTooLong {
            max: MAX_DESCRIPTION_CHARS,
        });
    }
    Ok((!text.is_empty()).then(|| text.to_string()))
}

impl CustomFieldDefinition {
    /// A new definition in `namespace`, at `position` on its owner type's form. Records
    /// `custom_field_definition.created`.
    pub fn new(
        id: CustomFieldDefinitionId,
        namespace: Namespace,
        spec: DefinitionSpec,
        position: u32,
    ) -> Result<Self, DefinitionError> {
        let mut definition = Self::build(id, namespace, spec, position)?;
        let event = CustomFieldDefinitionCreated::of(&definition);
        definition.events.record(definition.subject(), &event);
        Ok(definition)
    }

    fn build(
        id: CustomFieldDefinitionId,
        namespace: Namespace,
        spec: DefinitionSpec,
        position: u32,
    ) -> Result<Self, DefinitionError> {
        let definition = Self {
            id,
            version: Version::NEW,
            stamps: None,
            owner_type: spec.owner_type,
            key: Key::new(&spec.key)?,
            name: name_of(&spec.name)?,
            description: description_of(spec.description)?,
            validations: Validations::new(spec.field_type, spec.validations)?,
            field_type: spec.field_type,
            access: spec.access,
            private: spec.private,
            personal: spec.personal,
            capabilities: spec.capabilities,
            namespace,
            position,
            events: Recorder::default(),
        };
        definition.check()?;
        Ok(definition)
    }

    /// The rules across fields.
    fn check(&self) -> Result<(), DefinitionError> {
        if self.private && self.namespace == Namespace::Custom {
            return Err(DefinitionError::PrivateNotAllowed);
        }
        if self.personal && !self.owner_type.allows_personal() {
            return Err(DefinitionError::PersonalNotAllowed(self.owner_type));
        }
        if self.capabilities.unique && !self.field_type.may_be_unique() {
            return Err(DefinitionError::UniqueNotAllowed(self.field_type));
        }
        if self.capabilities.filterable && !self.field_type.may_be_filterable() {
            return Err(DefinitionError::FilterableNotAllowed(self.field_type));
        }
        if self.namespace == Namespace::Custom && self.access.admin != AdminAccess::ReadWrite {
            return Err(DefinitionError::AdminAccessNotAllowed);
        }
        Ok(())
    }

    fn subject(&self) -> Subject {
        Subject::new(CUSTOM_FIELD_DEFINITION, self.id.as_uuid())
    }

    /// The definition `change` would make, without touching this one: for commands that
    /// check a tightening against the stored values before applying it. Returns the
    /// changed one and the names of the fields that changed (empty: nothing to write).
    pub fn changed(
        &self,
        change: DefinitionChange,
    ) -> Result<(CustomFieldDefinition, Vec<&'static str>), DefinitionError> {
        let mut next = self.clone();
        next.events = Recorder::default();
        let mut fields = Vec::new();
        if let Some(name) = change.name {
            let name = name_of(&name)?;
            if name != next.name {
                next.name = name;
                fields.push("name");
            }
        }
        if let Some(description) = change.description {
            let description = description_of(description)?;
            if description != next.description {
                next.description = description;
                fields.push("description");
            }
        }
        if let Some(raw) = change.validations {
            let validations = Validations::new(next.field_type, raw)?;
            if validations != next.validations {
                next.validations = validations;
                fields.push("validations");
            }
        }
        if let Some(access) = change.access
            && access != next.access
        {
            next.access = access;
            fields.push("access");
        }
        if let Some(private) = change.private
            && private != next.private
        {
            next.private = private;
            fields.push("private");
        }
        if let Some(personal) = change.personal
            && personal != next.personal
        {
            next.personal = personal;
            fields.push("personal");
        }
        if let Some(capabilities) = change.capabilities
            && capabilities != next.capabilities
        {
            next.capabilities = capabilities;
            fields.push("capabilities");
        }
        next.check()?;
        Ok((next, fields))
    }

    /// Applies `change`, recording `custom_field_definition.updated` with the fields
    /// that changed; nothing changed records nothing. Returns those fields.
    pub fn change(
        &mut self,
        change: DefinitionChange,
    ) -> Result<Vec<&'static str>, DefinitionError> {
        let (next, fields) = self.changed(change)?;
        if fields.is_empty() {
            return Ok(fields);
        }
        let events = std::mem::take(&mut self.events);
        *self = next;
        self.events = events;
        self.record_updated(fields.clone());
        Ok(fields)
    }

    fn record_updated(&mut self, fields: Vec<&'static str>) {
        let event = CustomFieldDefinitionUpdated::of(self, fields);
        self.events.record(self.subject(), &event);
    }

    /// Whether going from this definition to `next` may reject values valid now (§2): a
    /// tighter validation, or `unique` turned on. Then the stored values are checked
    /// first.
    pub fn tightened_by(&self, next: &CustomFieldDefinition) -> bool {
        self.validations.tightened_by(&next.validations)
            || (next.capabilities.unique && !self.capabilities.unique)
    }

    /// Moves it on its owner type's form, recording the change.
    pub fn move_to(&mut self, position: u32) {
        if position != self.position {
            self.position = position;
            self.record_updated(vec!["position"]);
        }
    }

    /// Renumbers it after a sibling moved or went, recording nothing: the sibling's own
    /// event says what happened.
    pub fn shift_to(&mut self, position: u32) {
        self.position = position;
    }

    /// Records `custom_field_definition.deleted`, before the repo deletes it (with its
    /// values).
    pub fn mark_deleted(&mut self) {
        let event = CustomFieldDefinitionDeleted::of(self);
        self.events.record(self.subject(), &event);
    }

    pub fn id(&self) -> CustomFieldDefinitionId {
        self.id
    }

    pub fn owner_type(&self) -> OwnerType {
        self.owner_type
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn key(&self) -> &Key {
        &self.key
    }

    /// `namespace.key`.
    pub fn field_key(&self) -> FieldKey {
        FieldKey::new(self.namespace.clone(), self.key.clone())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn field_type(&self) -> FieldType {
        self.field_type
    }

    pub fn validations(&self) -> &Validations {
        &self.validations
    }

    pub fn access(&self) -> Access {
        self.access
    }

    pub fn private(&self) -> bool {
        self.private
    }

    pub fn personal(&self) -> bool {
        self.personal
    }

    pub fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    pub fn position(&self) -> u32 {
        self.position
    }

    /// Whether an app or a spore manages it (its namespace isn't `custom`): then it's
    /// read-only through the definitions API (`custom_field_managed`).
    pub fn is_managed(&self) -> bool {
        self.namespace.managed_by().is_some()
    }

    pub fn snapshot(&self) -> DefinitionSnapshot {
        DefinitionSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            owner_type: self.owner_type,
            namespace: self.namespace.clone(),
            key: self.key.as_str().to_string(),
            name: self.name.clone(),
            description: self.description.clone(),
            field_type: self.field_type,
            validations: self.validations.to_raw(),
            access: self.access,
            private: self.private,
            personal: self.personal,
            capabilities: self.capabilities,
            position: self.position,
        }
    }

    /// Rebuilds a definition from storage, with the same checks as `new`.
    pub fn restore(s: DefinitionSnapshot) -> Result<Self, DefinitionError> {
        let spec = DefinitionSpec {
            owner_type: s.owner_type,
            key: s.key,
            field_type: s.field_type,
            name: s.name,
            description: s.description,
            validations: s.validations,
            access: s.access,
            private: s.private,
            personal: s.personal,
            capabilities: s.capabilities,
        };
        let mut definition = Self::build(s.id, s.namespace, spec, s.position)?;
        definition.version = s.version;
        definition.stamps = s.stamps;
        Ok(definition)
    }
}

/// Plain-data form of a definition for storage adapters. `validations` is the JSON
/// form (`RawValidations`), stored as JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct DefinitionSnapshot {
    pub id: CustomFieldDefinitionId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub owner_type: OwnerType,
    pub namespace: Namespace,
    pub key: String,
    pub name: String,
    pub description: Option<String>,
    pub field_type: FieldType,
    pub validations: RawValidations,
    pub access: Access,
    pub private: bool,
    pub personal: bool,
    pub capabilities: Capabilities,
    pub position: u32,
}

impl Recorded for CustomFieldDefinition {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for CustomFieldDefinition {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for CustomFieldDefinition {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::AppKey;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::{
        namespace::KeyError,
        types::{BaseType, ValidationError},
    };

    fn spec(owner_type: OwnerType, field_type: &str) -> DefinitionSpec {
        DefinitionSpec {
            owner_type,
            key: "material".to_string(),
            field_type: FieldType::parse(field_type).unwrap(),
            name: "Material".to_string(),
            description: None,
            validations: RawValidations::default(),
            access: Access::default(),
            private: false,
            personal: false,
            capabilities: Capabilities::default(),
        }
    }

    fn id() -> CustomFieldDefinitionId {
        CustomFieldDefinitionId::from_uuid(Uuid::from_u128(1))
    }

    fn app() -> Namespace {
        Namespace::of_app(AppKey::new("app_erp").unwrap())
    }

    fn new(
        namespace: Namespace,
        spec: DefinitionSpec,
    ) -> Result<CustomFieldDefinition, DefinitionError> {
        CustomFieldDefinition::new(id(), namespace, spec, 0)
    }

    #[test]
    fn a_new_definition_records_its_creation() {
        let mut d = new(Namespace::Custom, spec(OwnerType::Product, "text")).unwrap();
        let events = d.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "custom_field_definition.created");
        assert_eq!(
            events[0].data,
            json!({"definition": id().to_string(), "owner_type": "product", "namespace": "custom", "key": "material"})
        );
    }

    #[test]
    fn the_key_is_checked() {
        let mut s = spec(OwnerType::Product, "text");
        s.key = "Material".to_string();
        assert_eq!(
            new(Namespace::Custom, s),
            Err(DefinitionError::Key(KeyError::Invalid(
                "Material".to_string()
            )))
        );
    }

    #[test]
    fn personal_only_on_customers_and_orders() {
        let mut s = spec(OwnerType::Product, "text");
        s.personal = true;
        assert_eq!(
            new(Namespace::Custom, s.clone()),
            Err(DefinitionError::PersonalNotAllowed(OwnerType::Product))
        );
        s.owner_type = OwnerType::Customer;
        assert!(new(Namespace::Custom, s).is_ok());
    }

    #[test]
    fn private_only_in_app_namespaces() {
        let mut s = spec(OwnerType::Product, "text");
        s.private = true;
        assert_eq!(
            new(Namespace::Custom, s.clone()),
            Err(DefinitionError::PrivateNotAllowed)
        );
        assert!(new(app(), s).is_ok());
    }

    #[test]
    fn unique_only_on_text_integer_and_url() {
        let mut s = spec(OwnerType::Product, "decimal");
        s.capabilities.unique = true;
        assert_eq!(
            new(Namespace::Custom, s.clone()),
            Err(DefinitionError::UniqueNotAllowed(FieldType::single(
                BaseType::Decimal
            )))
        );
        s.field_type = FieldType::parse("list.text").unwrap();
        assert!(matches!(
            new(Namespace::Custom, s.clone()),
            Err(DefinitionError::UniqueNotAllowed(_))
        ));
        s.field_type = FieldType::parse("integer").unwrap();
        assert!(new(Namespace::Custom, s).is_ok());
    }

    #[test]
    fn long_text_is_not_filterable() {
        let mut s = spec(OwnerType::Product, "rich_text");
        s.capabilities.filterable = true;
        assert_eq!(
            new(Namespace::Custom, s),
            Err(DefinitionError::FilterableNotAllowed(FieldType::single(
                BaseType::RichText
            )))
        );
    }

    #[test]
    fn custom_fields_are_writable_by_staff() {
        let mut s = spec(OwnerType::Product, "text");
        s.access.admin = AdminAccess::Read;
        assert_eq!(
            new(Namespace::Custom, s.clone()),
            Err(DefinitionError::AdminAccessNotAllowed)
        );
        assert!(new(app(), s).is_ok());
    }

    #[test]
    fn validations_must_fit_the_type() {
        let mut s = spec(OwnerType::Product, "integer");
        s.validations.pattern = Some("x".to_string());
        assert_eq!(
            new(Namespace::Custom, s),
            Err(DefinitionError::Validation(ValidationError::NotAllowed {
                validation: "pattern",
                field_type: FieldType::single(BaseType::Integer),
            }))
        );
    }

    #[test]
    fn a_choice_field_needs_its_choices() {
        assert_eq!(
            new(Namespace::Custom, spec(OwnerType::Customer, "choice")),
            Err(DefinitionError::Validation(ValidationError::Required {
                validation: "choices",
                field_type: FieldType::single(BaseType::Choice),
            }))
        );
    }

    #[test]
    fn a_change_records_the_fields_it_changed() {
        let mut d = new(Namespace::Custom, spec(OwnerType::Product, "text")).unwrap();
        d.take_events();
        let fields = d
            .change(DefinitionChange {
                name: Some("Fabric".to_string()),
                description: Some(Some("What it's made of".to_string())),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(fields, ["name", "description"]);
        let events = d.take_events();
        assert_eq!(events[0].data["fields"], json!(["name", "description"]));
        assert!(
            d.change(DefinitionChange {
                name: Some("Fabric".to_string()),
                ..Default::default()
            })
            .unwrap()
            .is_empty()
        );
        assert!(d.take_events().is_empty());
    }

    #[test]
    fn a_lower_max_tightens_but_a_higher_one_does_not() {
        let mut s = spec(OwnerType::Product, "integer");
        s.validations.max = Some(json!(10));
        let d = new(Namespace::Custom, s).unwrap();
        let with_max = |max: i64| {
            d.changed(DefinitionChange {
                validations: Some(RawValidations {
                    max: Some(json!(max)),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap()
            .0
        };
        assert!(d.tightened_by(&with_max(9)));
        assert!(!d.tightened_by(&with_max(11)));
        let without = d
            .changed(DefinitionChange {
                validations: Some(RawValidations::default()),
                ..Default::default()
            })
            .unwrap()
            .0;
        assert!(!d.tightened_by(&without));
    }

    #[test]
    fn fewer_choices_and_a_new_pattern_tighten() {
        let mut s = spec(OwnerType::Product, "choice");
        s.validations.choices = Some(vec![
            crate::types::Choice {
                key: "a".into(),
                label: "A".into(),
            },
            crate::types::Choice {
                key: "b".into(),
                label: "B".into(),
            },
        ]);
        let d = new(Namespace::Custom, s).unwrap();
        let relabelled = d
            .changed(DefinitionChange {
                validations: Some(RawValidations {
                    choices: Some(vec![
                        crate::types::Choice {
                            key: "a".into(),
                            label: "Aa".into(),
                        },
                        crate::types::Choice {
                            key: "b".into(),
                            label: "B".into(),
                        },
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap()
            .0;
        assert!(!d.tightened_by(&relabelled));
        let fewer = d
            .changed(DefinitionChange {
                validations: Some(RawValidations {
                    choices: Some(vec![crate::types::Choice {
                        key: "a".into(),
                        label: "A".into(),
                    }]),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap()
            .0;
        assert!(d.tightened_by(&fewer));
        let text = new(Namespace::Custom, spec(OwnerType::Product, "text")).unwrap();
        let patterned = text
            .changed(DefinitionChange {
                validations: Some(RawValidations {
                    pattern: Some("[a-z]+".into()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap()
            .0;
        assert!(text.tightened_by(&patterned));
    }

    #[test]
    fn turning_unique_on_tightens() {
        let d = new(Namespace::Custom, spec(OwnerType::Product, "text")).unwrap();
        let unique = d
            .changed(DefinitionChange {
                capabilities: Some(Capabilities {
                    unique: true,
                    filterable: false,
                }),
                ..Default::default()
            })
            .unwrap()
            .0;
        assert!(d.tightened_by(&unique));
        assert!(!unique.tightened_by(&d));
    }

    #[test]
    fn restore_round_trips() {
        let mut s = spec(OwnerType::Product, "measurement");
        s.validations.dimension = Some("weight".to_string());
        s.validations.max = Some(json!({"value": "2", "unit": "kg"}));
        let mut d = new(Namespace::Custom, s).unwrap();
        d.take_events();
        d.set_version(Version::new(3));
        assert_eq!(CustomFieldDefinition::restore(d.snapshot()), Ok(d));
    }
}
