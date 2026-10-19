//! A spore's settings (design note 0030, §8): what the merchant sets for a spore built
//! into the API (a Stripe secret key, a webhook signing secret), one record per spore,
//! saved through core rather than a store each spore builds.
//!
//! A spore describes its form with a JSON Schema (`SettingsSchema`: the subset every
//! admin can render, checked when the host starts), marks fields that hold secrets
//! `"x-hypha-secret": true`, and checks values its own way too (`check` passed to
//! `save_spore_settings`). Secret fields are stored sealed with the host's `SecretBox`
//! and never answered: an answer shows them as `{"set": true}`. The spore reads them
//! unsealed (`read_spore_settings`).

use std::fmt;

use hypha_core::{
    AppKey, AppKind, Ctx, Random, RepoError, Scope, SealError, Sealed, SecretBox, Stamped, Stamps,
    Version, Versioned,
};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::{AppRepo, CommandError, HasApps};

/// The marker of a secret field in a schema: `"x-hypha-secret": true` on a string
/// property. A contract with spores and admins.
pub const SECRET_MARKER: &str = "x-hypha-secret";

/// The key a sealed value is stored under: `{"sealed": "<hex>"}`.
const SEALED: &str = "sealed";

/// One thing wrong with settings: the field (`""` for the whole) and what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingsProblem {
    pub field: String,
    pub message: String,
}

impl SettingsProblem {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for SettingsProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.field.as_str() {
            "" => f.write_str(&self.message),
            field => write!(f, "{field}: {}", self.message),
        }
    }
}

/// A field's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    String,
    Integer,
    Number,
    Boolean,
}

/// One property of a settings schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub kind: FieldType,
    pub required: bool,
    pub secret: bool,
    pub choices: Option<Vec<Value>>,
    pub min_length: Option<u64>,
    pub max_length: Option<u64>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

/// A spore's settings form: a JSON Schema object of scalar properties, the subset every
/// admin renders. Keywords allowed on the object: `type` (`"object"`), `title`,
/// `description`, `properties`, `required`, `additionalProperties` (`false` only:
/// unknown fields are always refused). On a property: `type` (`string`, `integer`,
/// `number`, `boolean`), `title`, `description`, `default`, `examples`, `format`,
/// `enum`, `minLength`, `maxLength`, `minimum`, `maximum`, and `x-hypha-secret` on
/// strings. Anything else is refused when the host starts, so an admin never meets a
/// form it can't draw.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsSchema {
    json: Value,
    fields: Vec<Field>,
}

const OBJECT_KEYWORDS: &[&str] = &[
    "$schema",
    "type",
    "title",
    "description",
    "properties",
    "required",
    "additionalProperties",
];
const FIELD_KEYWORDS: &[&str] = &[
    "type",
    "title",
    "description",
    "default",
    "examples",
    "format",
    "enum",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    SECRET_MARKER,
];

impl SettingsSchema {
    /// Checks `json` is in the subset; every problem, by field.
    pub fn parse(json: Value) -> Result<Self, Vec<SettingsProblem>> {
        let mut problems = Vec::new();
        let Some(object) = json.as_object() else {
            return Err(vec![SettingsProblem::new("", "is a JSON object")]);
        };
        for key in object.keys() {
            if !OBJECT_KEYWORDS.contains(&key.as_str()) {
                problems.push(SettingsProblem::new("", format!("{key:?} isn't supported")));
            }
        }
        if object.get("type").and_then(Value::as_str) != Some("object") {
            problems.push(SettingsProblem::new("", "\"type\" is \"object\""));
        }
        if object
            .get("additionalProperties")
            .is_some_and(|a| a != &Value::Bool(false))
        {
            problems.push(SettingsProblem::new(
                "",
                "\"additionalProperties\" can only be false",
            ));
        }
        let required: Vec<&str> = match object.get("required") {
            None => Vec::new(),
            Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
            Some(_) => {
                problems.push(SettingsProblem::new("", "\"required\" is a list of names"));
                Vec::new()
            }
        };
        let empty = Map::new();
        let properties = match object.get("properties") {
            None => &empty,
            Some(Value::Object(p)) => p,
            Some(_) => {
                problems.push(SettingsProblem::new("", "\"properties\" is an object"));
                &empty
            }
        };
        for name in &required {
            if !properties.contains_key(*name) {
                problems.push(SettingsProblem::new(
                    *name,
                    "is required but not a property",
                ));
            }
        }
        let mut fields = Vec::new();
        for (name, property) in properties {
            match field(name, property, required.contains(&name.as_str())) {
                Ok(f) => fields.push(f),
                Err(mut more) => problems.append(&mut more),
            }
        }
        match problems.is_empty() {
            true => Ok(Self { json, fields }),
            false => Err(problems),
        }
    }

    /// The schema as the spore wrote it, for admins to render.
    pub fn json(&self) -> &Value {
        &self.json
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The secret fields' names.
    pub fn secrets(&self) -> impl Iterator<Item = &str> {
        self.fields
            .iter()
            .filter(|f| f.secret)
            .map(|f| f.name.as_str())
    }

    fn get(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Every way `values` breaks the schema: unknown fields, wrong types, values out of
    /// range or not among the choices, required ones missing.
    pub fn check(&self, values: &Map<String, Value>) -> Vec<SettingsProblem> {
        let mut problems = Vec::new();
        for (name, value) in values {
            let Some(field) = self.get(name) else {
                problems.push(SettingsProblem::new(name, "is not a setting"));
                continue;
            };
            if let Some(problem) = field.check(value) {
                problems.push(SettingsProblem::new(name, problem));
            }
        }
        for field in &self.fields {
            if field.required && !values.contains_key(&field.name) {
                problems.push(SettingsProblem::new(&field.name, "is required"));
            }
        }
        problems
    }
}

/// One property read from the schema.
fn field(name: &str, property: &Value, required: bool) -> Result<Field, Vec<SettingsProblem>> {
    let mut problems = Vec::new();
    let Some(property) = property.as_object() else {
        return Err(vec![SettingsProblem::new(name, "is a JSON object")]);
    };
    for key in property.keys() {
        if !FIELD_KEYWORDS.contains(&key.as_str()) {
            problems.push(SettingsProblem::new(
                name,
                format!("{key:?} isn't supported"),
            ));
        }
    }
    let kind = match property.get("type").and_then(Value::as_str) {
        Some("string") => FieldType::String,
        Some("integer") => FieldType::Integer,
        Some("number") => FieldType::Number,
        Some("boolean") => FieldType::Boolean,
        _ => {
            problems.push(SettingsProblem::new(
                name,
                "\"type\" is string, integer, number or boolean",
            ));
            FieldType::String
        }
    };
    let secret = match property.get(SECRET_MARKER) {
        None => false,
        Some(Value::Bool(secret)) => *secret,
        Some(_) => {
            problems.push(SettingsProblem::new(
                name,
                "\"x-hypha-secret\" is true or false",
            ));
            false
        }
    };
    if secret && kind != FieldType::String {
        problems.push(SettingsProblem::new(name, "only a string can be secret"));
    }
    let choices = match property.get("enum") {
        None => None,
        Some(Value::Array(choices)) if !choices.is_empty() => Some(choices.clone()),
        Some(_) => {
            problems.push(SettingsProblem::new(name, "\"enum\" is a list of values"));
            None
        }
    };
    let number = |key: &str, problems: &mut Vec<SettingsProblem>| match property.get(key) {
        None => None,
        Some(v) => match v.as_f64() {
            Some(n) => Some(n),
            None => {
                problems.push(SettingsProblem::new(name, format!("\"{key}\" is a number")));
                None
            }
        },
    };
    let length = |key: &str, problems: &mut Vec<SettingsProblem>| match property.get(key) {
        None => None,
        Some(v) => match v.as_u64() {
            Some(n) => Some(n),
            None => {
                problems.push(SettingsProblem::new(
                    name,
                    format!("\"{key}\" is a whole number"),
                ));
                None
            }
        },
    };
    let min_length = length("minLength", &mut problems);
    let max_length = length("maxLength", &mut problems);
    let minimum = number("minimum", &mut problems);
    let maximum = number("maximum", &mut problems);
    match problems.is_empty() {
        true => Ok(Field {
            name: name.to_string(),
            kind,
            required,
            secret,
            choices,
            min_length,
            max_length,
            minimum,
            maximum,
        }),
        false => Err(problems),
    }
}

impl Field {
    /// What's wrong with `value` for this field, if anything.
    fn check(&self, value: &Value) -> Option<String> {
        let typed = match self.kind {
            FieldType::String => value.is_string(),
            FieldType::Integer => value.is_i64() || value.is_u64(),
            FieldType::Number => value.is_number(),
            FieldType::Boolean => value.is_boolean(),
        };
        if !typed {
            let kind = match self.kind {
                FieldType::String => "a string",
                FieldType::Integer => "a whole number",
                FieldType::Number => "a number",
                FieldType::Boolean => "true or false",
            };
            return Some(format!("is {kind}"));
        }
        if let Some(choices) = &self.choices
            && !choices.contains(value)
        {
            let list: Vec<String> = choices.iter().map(Value::to_string).collect();
            return Some(format!("is one of {}", list.join(", ")));
        }
        if let Some(text) = value.as_str() {
            let chars = text.chars().count() as u64;
            if self.min_length.is_some_and(|min| chars < min) {
                return Some(format!(
                    "is at least {} characters",
                    self.min_length.unwrap_or_default()
                ));
            }
            if self.max_length.is_some_and(|max| chars > max) {
                return Some(format!(
                    "is at most {} characters",
                    self.max_length.unwrap_or_default()
                ));
            }
        }
        if let Some(n) = value.as_f64() {
            if let Some(min) = self.minimum.filter(|min| n < *min) {
                return Some(format!("is at least {min}"));
            }
            if let Some(max) = self.maximum.filter(|max| n > *max) {
                return Some(format!("is at most {max}"));
            }
        }
        None
    }
}

/// A spore's stored settings: plain values as JSON, secret ones sealed
/// (`{"sealed": "<hex>"}`, sealed for this spore and field).
#[derive(Debug, Clone, PartialEq)]
pub struct SporeSettings {
    key: AppKey,
    values: Map<String, Value>,
    version: Version,
    stamps: Option<Stamps>,
}

impl SporeSettings {
    /// Settings as storage holds them.
    pub fn restore(
        key: AppKey,
        values: Map<String, Value>,
        version: Version,
        stamps: Option<Stamps>,
    ) -> Self {
        Self {
            key,
            values,
            version,
            stamps,
        }
    }

    pub fn key(&self) -> &AppKey {
        &self.key
    }

    /// As stored: secret fields sealed.
    pub fn values(&self) -> &Map<String, Value> {
        &self.values
    }
}

impl Versioned for SporeSettings {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for SporeSettings {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

/// Uniqueness rules, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    /// One record per spore.
    pub const SPORE_SETTINGS: &str = "spore_settings_key";
}

/// Spores' settings, by key. Records nothing in the outbox.
#[allow(async_fn_in_trait)]
pub trait SporeSettingsRepo {
    async fn get(&self, key: &AppKey) -> Result<Option<SporeSettings>, RepoError>;
    /// Stores a spore's first settings (version 1); `Duplicate(SPORE_SETTINGS)` when it
    /// has some.
    async fn insert(&self, settings: &mut SporeSettings) -> Result<(), RepoError>;
    /// `StaleVersion` unless the stored settings have `settings`' version.
    async fn update(&self, settings: &mut SporeSettings) -> Result<(), RepoError>;
}

/// Whatever holds the `SporeSettingsRepo`: a host's per-request ports, a test world.
/// What a spore reads its settings through (`read_spore_settings`).
pub trait HasSporeSettings {
    type SporeSettings: SporeSettingsRepo;
    fn spore_settings(&self) -> &Self::SporeSettings;
}

impl<T: HasSporeSettings + ?Sized> HasSporeSettings for &T {
    type SporeSettings = T::SporeSettings;
    fn spore_settings(&self) -> &Self::SporeSettings {
        (**self).spore_settings()
    }
}

/// What a caller sees of a spore's settings: plain values as set, secret ones as
/// `{"set": true}` (or `false`), never their values.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsView {
    pub values: Map<String, Value>,
    /// The stored record's version and times; `None` before the first save.
    pub version: Option<Version>,
    pub stamps: Option<Stamps>,
}

fn view(schema: &SettingsSchema, stored: Option<&SporeSettings>) -> SettingsView {
    let mut values = Map::new();
    if let Some(stored) = stored {
        for (name, value) in stored.values() {
            if schema.get(name).is_some_and(|f| !f.secret) {
                values.insert(name.clone(), value.clone());
            }
        }
    }
    for secret in schema.secrets() {
        let set = stored.is_some_and(|s| s.values().contains_key(secret));
        values.insert(secret.to_string(), serde_json::json!({ "set": set }));
    }
    SettingsView {
        values,
        version: stored.map(|s| s.version()),
        stamps: stored.and_then(|s| s.stamps()),
    }
}

/// The spore `key` and its schema: a spore this host has (`AppNotFound` otherwise:
/// only spores keep their settings here, an app keeps its own) and that takes settings
/// (`SettingsNotFound`).
async fn spore_with_settings<'s, R: AppRepo>(
    apps: &R,
    key: &str,
    schema: Option<&'s SettingsSchema>,
) -> Result<(AppKey, &'s SettingsSchema), CommandError> {
    let not_found = || CommandError::AppNotFound(key.to_string());
    let parsed = AppKey::new(key).map_err(|_| not_found())?;
    if parsed.kind() != AppKind::Spore || apps.get(&parsed).await?.is_none() {
        return Err(not_found());
    }
    let schema = schema.ok_or_else(|| CommandError::SettingsNotFound(parsed.clone()))?;
    Ok((parsed, schema))
}

/// A spore's settings as staff see them (`apps:read`). `schema` is the spore's, `None`
/// when it takes none (or `key` is no spore of this host).
pub async fn get_spore_settings<S: HasApps + HasSporeSettings>(
    ctx: &Ctx,
    ports: &S,
    key: &str,
    schema: Option<&SettingsSchema>,
) -> Result<SettingsView, CommandError> {
    ctx.require(Scope::AppsRead)?;
    let (key, schema) = spore_with_settings(ports.apps(), key, schema).await?;
    let stored = ports.spore_settings().get(&key).await?;
    Ok(view(schema, stored.as_ref()))
}

/// The context a secret field is sealed with: its spore and its name, so a sealed value
/// copied to another field or spore doesn't open.
fn seal_context(key: &AppKey, field: &str) -> Vec<u8> {
    format!("{key}/{field}").into_bytes()
}

fn sealed_value(sealed: &Sealed) -> Value {
    let hex: String = sealed
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    serde_json::json!({ SEALED: hex })
}

fn sealed_from(value: &Value) -> Option<Sealed> {
    let hex = value.get(SEALED)?.as_str()?;
    let bytes: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| {
            hex.get(i..i + 2)
                .and_then(|b| u8::from_str_radix(b, 16).ok())
        })
        .collect();
    bytes.map(Sealed::from_bytes)
}

/// Opens a stored secret field.
fn open(
    secrets: &impl SecretBox,
    key: &AppKey,
    field: &str,
    value: &Value,
) -> Result<String, SealError> {
    let sealed = sealed_from(value).ok_or(SealError("a secret setting isn't sealed"))?;
    let plain = secrets.open(&sealed, &seal_context(key, field))?;
    String::from_utf8(plain).map_err(|_| SealError("a secret setting isn't UTF-8"))
}

/// Saves a spore's settings (`apps:write`): `input` is the whole form (an object). A
/// secret field left out keeps what's stored, `null` clears it, a string replaces it.
/// The values are checked against `schema`, then by the spore (`check`, which sees
/// secrets in plain text), then the secret ones are sealed with `secrets` and the record
/// written. Answers as `get_spore_settings`.
pub async fn save_spore_settings<S: HasApps + HasSporeSettings>(
    ctx: &Ctx,
    ports: &S,
    secrets: &(impl SecretBox + Random),
    key: &str,
    schema: Option<&SettingsSchema>,
    input: Value,
    check: impl FnOnce(&Map<String, Value>) -> Result<(), Vec<SettingsProblem>>,
) -> Result<SettingsView, CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let (key, schema) = spore_with_settings(ports.apps(), key, schema).await?;
    let Value::Object(mut input) = input else {
        return Err(CommandError::SettingsInvalid(vec![SettingsProblem::new(
            "",
            "the settings are a JSON object",
        )]));
    };
    let stored = ports.spore_settings().get(&key).await?;
    // The values in plain text (to check), and as they'll be stored.
    let mut plain = Map::new();
    let mut kept = Map::new();
    for field in schema.secrets() {
        match input.remove(field) {
            Some(Value::Null) => {}
            Some(Value::String(secret)) => {
                let mut nonce = [0u8; 24];
                secrets.fill(&mut nonce);
                let sealed = secrets.seal(secret.as_bytes(), &seal_context(&key, field), nonce);
                kept.insert(field.to_string(), sealed_value(&sealed));
                plain.insert(field.to_string(), Value::String(secret));
            }
            Some(other) => {
                plain.insert(field.to_string(), other);
            }
            None => {
                if let Some(value) = stored.as_ref().and_then(|s| s.values().get(field)) {
                    let secret = open(secrets, &key, field, value).map_err(CommandError::Seal)?;
                    kept.insert(field.to_string(), value.clone());
                    plain.insert(field.to_string(), Value::String(secret));
                }
            }
        }
    }
    for (name, value) in input {
        if !value.is_null() {
            plain.insert(name.clone(), value.clone());
            kept.insert(name, value);
        }
    }
    let problems = schema.check(&plain);
    if !problems.is_empty() {
        return Err(CommandError::SettingsInvalid(problems));
    }
    check(&plain).map_err(CommandError::SettingsInvalid)?;
    let repo = ports.spore_settings();
    let saved = match stored {
        Some(mut settings) => {
            settings.values = kept;
            repo.update(&mut settings).await?;
            settings
        }
        None => {
            let mut settings = SporeSettings::restore(key, kept, Version::NEW, None);
            repo.insert(&mut settings).await?;
            settings
        }
    };
    Ok(view(schema, Some(&saved)))
}

/// Why a spore couldn't read its settings.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsReadError {
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Seal(#[from] SealError),
}

/// A spore's settings in plain text, secrets opened, for the spore itself (as core
/// reads: no scope, the spore is trusted code, SP2). `None` before the merchant first
/// saves them. A spore deserializes the map into its own type.
pub async fn read_spore_settings<S: HasSporeSettings + ?Sized>(
    ports: &S,
    secrets: &impl SecretBox,
    key: &AppKey,
) -> Result<Option<Map<String, Value>>, SettingsReadError> {
    let Some(stored) = ports.spore_settings().get(key).await? else {
        return Ok(None);
    };
    let mut values = Map::new();
    for (name, value) in stored.values() {
        let plain = match sealed_from(value) {
            Some(_) => Value::String(open(secrets, key, name, value)?),
            None => value.clone(),
        };
        values.insert(name.clone(), plain);
    }
    Ok(Some(values))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> SettingsSchema {
        SettingsSchema::parse(json!({
            "type": "object",
            "properties": {
                "secret_key": { "type": "string", "title": "Secret key", "x-hypha-secret": true },
                "mode": { "type": "string", "enum": ["test", "live"] },
                "retries": { "type": "integer", "minimum": 0, "maximum": 5 },
                "label": { "type": "string", "maxLength": 3 },
                "capture": { "type": "boolean" }
            },
            "required": ["secret_key", "mode"],
            "additionalProperties": false
        }))
        .unwrap()
    }

    #[test]
    fn a_schema_in_the_subset_parses_with_its_secrets() {
        let schema = schema();
        assert_eq!(schema.secrets().collect::<Vec<_>>(), ["secret_key"]);
        assert_eq!(schema.fields().len(), 5);
    }

    #[test]
    fn what_an_admin_cant_draw_is_refused() {
        let problems = SettingsSchema::parse(json!({
            "type": "object",
            "properties": {
                "nested": { "type": "object" },
                "flag": { "type": "boolean", "x-hypha-secret": true },
                "pattern": { "type": "string", "pattern": "^sk_" }
            },
            "required": ["missing"],
            "oneOf": []
        }))
        .unwrap_err();
        let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
        assert_eq!(
            lines,
            [
                "\"oneOf\" isn't supported",
                "missing: is required but not a property",
                "flag: only a string can be secret",
                "nested: \"type\" is string, integer, number or boolean",
                "pattern: \"pattern\" isn't supported",
            ]
        );
    }

    #[test]
    fn values_are_checked_against_every_rule() {
        let values = json!({
            "mode": "staging",
            "retries": 9,
            "label": "four",
            "capture": "yes",
            "extra": 1
        });
        let problems = schema().check(values.as_object().unwrap());
        let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
        assert_eq!(
            lines,
            [
                "capture: is true or false",
                "extra: is not a setting",
                "label: is at most 3 characters",
                "mode: is one of \"test\", \"live\"",
                "retries: is at most 5",
                "secret_key: is required",
            ]
        );
        let good = json!({ "secret_key": "sk_x", "mode": "test", "retries": 0 });
        assert!(schema().check(good.as_object().unwrap()).is_empty());
    }

    #[test]
    fn a_sealed_value_round_trips_through_its_hex() {
        let sealed = Sealed::from_bytes(vec![0, 1, 254, 255]);
        let value = sealed_value(&sealed);
        assert_eq!(value, json!({ "sealed": "0001feff" }));
        assert_eq!(sealed_from(&value), Some(sealed));
        assert_eq!(sealed_from(&json!("sk_plain")), None);
    }
}
