//! Field types and their validations. A definition's type is immutable; its validations may
//! change, and a change that tightens them is checked against the stored values first.

use std::{collections::BTreeSet, fmt, str::FromStr};

use hypha_core::{AssetId, CategoryId, Currency, CustomerId, ProductId, VariantId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    decimal::Decimal,
    limits::{MAX_CHOICES, MAX_LIST_ITEMS, MAX_LONG_BYTES, MAX_PATTERN_CHARS, MAX_TEXT_CHARS},
};

/// The type of one value: every field type is one of these, alone or as a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseType {
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

impl BaseType {
    pub const ALL: [BaseType; 14] = [
        BaseType::Text,
        BaseType::MultilineText,
        BaseType::RichText,
        BaseType::Integer,
        BaseType::Decimal,
        BaseType::Boolean,
        BaseType::Date,
        BaseType::DateTime,
        BaseType::Url,
        BaseType::Money,
        BaseType::Measurement,
        BaseType::Choice,
        BaseType::Reference,
        BaseType::Json,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            BaseType::Text => "text",
            BaseType::MultilineText => "multiline_text",
            BaseType::RichText => "rich_text",
            BaseType::Integer => "integer",
            BaseType::Decimal => "decimal",
            BaseType::Boolean => "boolean",
            BaseType::Date => "date",
            BaseType::DateTime => "date_time",
            BaseType::Url => "url",
            BaseType::Money => "money",
            BaseType::Measurement => "measurement",
            BaseType::Choice => "choice",
            BaseType::Reference => "reference",
            BaseType::Json => "json",
        }
    }

    /// Whether `list.<t>` exists for it: every type but `boolean`, `rich_text`,
    /// `multiline_text` and `json`.
    pub fn listable(self) -> bool {
        !matches!(
            self,
            BaseType::Boolean | BaseType::RichText | BaseType::MultilineText | BaseType::Json
        )
    }

    /// Which validations it takes (lists add `min_items`, `max_items`, `unique_items`).
    fn allows(self, validation: &str) -> bool {
        let allowed: &[&str] = match self {
            BaseType::Text => &["min_length", "max_length", "pattern"],
            BaseType::MultilineText => &["min_length", "max_length"],
            BaseType::RichText => &["max_length"],
            BaseType::Integer | BaseType::Date | BaseType::DateTime => &["min", "max"],
            BaseType::Decimal => &["min", "max", "max_scale"],
            BaseType::Boolean | BaseType::Json => &[],
            BaseType::Url => &["schemes", "domains"],
            BaseType::Money => &["currencies", "min", "max"],
            BaseType::Measurement => &["dimension", "min", "max"],
            BaseType::Choice => &["choices"],
            BaseType::Reference => &["kind"],
        };
        allowed.contains(&validation)
    }
}

/// A field's type: one value, or a list of them (`list.text`). Serialised as its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FieldType {
    base: BaseType,
    list: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FieldTypeError {
    #[error("{0:?} is not a field type")]
    Unknown(String),
    #[error("{0} values can't be listed")]
    NotListable(BaseType),
}

impl hypha_core::ErrorCode for FieldTypeError {
    fn code(&self) -> &'static str {
        match self {
            FieldTypeError::Unknown(_) | FieldTypeError::NotListable(_) => "invalid_field_type",
        }
    }
}

impl fmt::Display for BaseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FieldType {
    pub fn single(base: BaseType) -> Self {
        Self { base, list: false }
    }

    pub fn list_of(base: BaseType) -> Result<Self, FieldTypeError> {
        if !base.listable() {
            return Err(FieldTypeError::NotListable(base));
        }
        Ok(Self { base, list: true })
    }

    pub fn parse(text: &str) -> Result<Self, FieldTypeError> {
        let (list, name) = match text.strip_prefix("list.") {
            Some(name) => (true, name),
            None => (false, text),
        };
        let base = BaseType::ALL
            .into_iter()
            .find(|b| b.as_str() == name)
            .ok_or_else(|| FieldTypeError::Unknown(text.to_string()))?;
        if list {
            Self::list_of(base)
        } else {
            Ok(Self::single(base))
        }
    }

    pub fn base(self) -> BaseType {
        self.base
    }

    pub fn is_list(self) -> bool {
        self.list
    }

    /// Whether a definition of this type may be `unique` (text, integer, url).
    pub fn may_be_unique(self) -> bool {
        !self.list
            && matches!(
                self.base,
                BaseType::Text | BaseType::Integer | BaseType::Url
            )
    }

    /// Whether a definition of this type may be `filterable`: values with an index form,
    /// single or listed. Long text, rich text and JSON aren't.
    pub fn may_be_filterable(self) -> bool {
        !matches!(
            self.base,
            BaseType::MultilineText | BaseType::RichText | BaseType::Json
        )
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.list {
            write!(f, "list.{}", self.base)
        } else {
            write!(f, "{}", self.base)
        }
    }
}

impl FromStr for FieldType {
    type Err = FieldTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<String> for FieldType {
    type Error = FieldTypeError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl From<FieldType> for String {
    fn from(t: FieldType) -> String {
        t.to_string()
    }
}

/// What a measurement measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    Weight,
    Length,
    Volume,
}

impl Dimension {
    pub const ALL: [Dimension; 3] = [Dimension::Weight, Dimension::Length, Dimension::Volume];

    pub fn as_str(self) -> &'static str {
        match self {
            Dimension::Weight => "weight",
            Dimension::Length => "length",
            Dimension::Volume => "volume",
        }
    }

    pub fn parse(s: &str) -> Option<Dimension> {
        Dimension::ALL.into_iter().find(|d| d.as_str() == s)
    }

    /// The unit the index stores this dimension in.
    pub fn base_unit(self) -> Unit {
        match self {
            Dimension::Weight => Unit::G,
            Dimension::Length => Unit::Mm,
            Dimension::Volume => Unit::Ml,
        }
    }
}

/// The fixed set of measurement units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    G,
    Kg,
    Oz,
    Lb,
    Mm,
    Cm,
    M,
    In,
    Ft,
    Ml,
    L,
    /// The US fluid ounce.
    FlOz,
}

impl Unit {
    pub const ALL: [Unit; 12] = [
        Unit::G,
        Unit::Kg,
        Unit::Oz,
        Unit::Lb,
        Unit::Mm,
        Unit::Cm,
        Unit::M,
        Unit::In,
        Unit::Ft,
        Unit::Ml,
        Unit::L,
        Unit::FlOz,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Unit::G => "g",
            Unit::Kg => "kg",
            Unit::Oz => "oz",
            Unit::Lb => "lb",
            Unit::Mm => "mm",
            Unit::Cm => "cm",
            Unit::M => "m",
            Unit::In => "in",
            Unit::Ft => "ft",
            Unit::Ml => "ml",
            Unit::L => "l",
            Unit::FlOz => "fl_oz",
        }
    }

    pub fn parse(s: &str) -> Option<Unit> {
        Unit::ALL.into_iter().find(|u| u.as_str() == s)
    }

    pub fn dimension(self) -> Dimension {
        match self {
            Unit::G | Unit::Kg | Unit::Oz | Unit::Lb => Dimension::Weight,
            Unit::Mm | Unit::Cm | Unit::M | Unit::In | Unit::Ft => Dimension::Length,
            Unit::Ml | Unit::L | Unit::FlOz => Dimension::Volume,
        }
    }

    /// How many of the dimension's base unit one of this is: exact, by definition of
    /// the international pound and yard and the US gallon.
    pub fn in_base(self) -> Decimal {
        let ratio = match self {
            Unit::G | Unit::Mm | Unit::Ml => "1",
            Unit::Kg | Unit::M | Unit::L => "1000",
            Unit::Oz => "28.349523125",
            Unit::Lb => "453.59237",
            Unit::Cm => "10",
            Unit::In => "25.4",
            Unit::Ft => "304.8",
            Unit::FlOz => "29.5735295625",
        };
        // The literals above are valid decimals.
        Decimal::parse(ratio).unwrap_or(Decimal::ZERO)
    }
}

/// A measurement value: `{"value": "1.5", "unit": "kg"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measurement {
    pub value: Decimal,
    pub unit: Unit,
}

impl Measurement {
    /// The value in its dimension's base unit, exact (`None` only if it doesn't fit,
    /// which values within the decimal limits never reach).
    pub fn in_base(self) -> Option<Decimal> {
        self.value.checked_mul(self.unit.in_base())
    }
}

/// What a `reference` field points at. More kinds later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReferenceKind {
    Product,
    Variant,
    Category,
    Customer,
    Asset,
}

impl ReferenceKind {
    pub const ALL: [ReferenceKind; 5] = [
        ReferenceKind::Product,
        ReferenceKind::Variant,
        ReferenceKind::Category,
        ReferenceKind::Customer,
        ReferenceKind::Asset,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ReferenceKind::Product => "product",
            ReferenceKind::Variant => "variant",
            ReferenceKind::Category => "category",
            ReferenceKind::Customer => "customer",
            ReferenceKind::Asset => "asset",
        }
    }

    pub fn parse(s: &str) -> Option<ReferenceKind> {
        ReferenceKind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The ID inside `text` if it's this kind's prefixed ID (`prod_…`).
    pub fn parse_id(self, text: &str) -> Option<Uuid> {
        let id = match self {
            ReferenceKind::Product => text.parse::<ProductId>().ok()?.as_uuid(),
            ReferenceKind::Variant => text.parse::<VariantId>().ok()?.as_uuid(),
            ReferenceKind::Category => text.parse::<CategoryId>().ok()?.as_uuid(),
            ReferenceKind::Customer => text.parse::<CustomerId>().ok()?.as_uuid(),
            ReferenceKind::Asset => text.parse::<AssetId>().ok()?.as_uuid(),
        };
        Some(id)
    }

    /// `id` as this kind's prefixed ID.
    pub fn format_id(self, id: Uuid) -> String {
        match self {
            ReferenceKind::Product => ProductId::from_uuid(id).to_string(),
            ReferenceKind::Variant => VariantId::from_uuid(id).to_string(),
            ReferenceKind::Category => CategoryId::from_uuid(id).to_string(),
            ReferenceKind::Customer => CustomerId::from_uuid(id).to_string(),
            ReferenceKind::Asset => AssetId::from_uuid(id).to_string(),
        }
    }
}

/// A reference value: the target's kind and ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reference {
    pub kind: ReferenceKind,
    pub id: Uuid,
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.kind.format_id(self.id))
    }
}

/// One choice of a `choice` field: a stable key (what's stored, filtered and faceted)
/// and a label for people (editable, translatable later).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub key: String,
    pub label: String,
}

/// A `pattern` validation: compiled once, compared by its source.
#[derive(Debug, Clone)]
pub struct Pattern {
    source: String,
    regex: regex::Regex,
}

impl Pattern {
    /// The whole value must match (the pattern is anchored at both ends).
    pub fn new(source: &str) -> Result<Pattern, String> {
        if source.chars().count() > MAX_PATTERN_CHARS {
            return Err(format!("is longer than {MAX_PATTERN_CHARS} characters"));
        }
        let regex = regex::RegexBuilder::new(&format!("^(?:{source})$"))
            .size_limit(256 * 1024)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Pattern {
            source: source.to_string(),
            regex,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.source
    }

    pub fn matches(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for Pattern {}

/// A `min` or `max`, in the field's own terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// An integer, or money in minor units.
    Integer(i64),
    Decimal(Decimal),
    Date(jiff::civil::Date),
    /// Microseconds since the epoch.
    DateTime(hypha_core::Timestamp),
    /// A measurement, kept in the unit it was given in and compared in the dimension's
    /// base unit.
    Measurement(Measurement),
}

impl Bound {
    /// What bounds compare by: the number, or the moment, in one unit.
    fn order_key(&self) -> Decimal {
        match self {
            Bound::Integer(n) => Decimal::from_i64(*n),
            Bound::Decimal(d) => *d,
            Bound::Date(d) => Decimal::from_i64(crate::value::date_micros(*d)),
            Bound::DateTime(t) => Decimal::from_i64(t.unix_micros()),
            Bound::Measurement(m) => m.in_base().unwrap_or(m.value),
        }
    }

    /// How two bounds of one field compare.
    pub fn cmp_value(&self, other: &Bound) -> std::cmp::Ordering {
        self.order_key().cmp(&other.order_key())
    }
}

/// Validations as the API and storage spell them: every key optional, checked against
/// the field's type by `Validations::new`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawValidations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_scale: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schemes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currencies: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<Choice>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unique_items: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("`{validation}` doesn't apply to {field_type} fields")]
    NotAllowed {
        validation: &'static str,
        field_type: FieldType,
    },
    #[error("{field_type} fields need `{validation}`")]
    Required {
        validation: &'static str,
        field_type: FieldType,
    },
    #[error("`{validation}` {message}")]
    Invalid {
        validation: &'static str,
        message: String,
    },
}

impl hypha_core::ErrorCode for ValidationError {
    fn code(&self) -> &'static str {
        match self {
            ValidationError::NotAllowed { .. } => "validation_not_allowed",
            ValidationError::Required { .. } => "validation_required",
            ValidationError::Invalid { .. } => "invalid_validation",
        }
    }
}

/// The URL schemes a `url` value may have.
pub const URL_SCHEMES: [&str; 4] = ["https", "http", "mailto", "tel"];

/// A field's validations, checked against its type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Validations {
    pub(crate) min_length: Option<u32>,
    pub(crate) max_length: Option<u32>,
    pub(crate) pattern: Option<Pattern>,
    pub(crate) min: Option<Bound>,
    pub(crate) max: Option<Bound>,
    pub(crate) max_scale: Option<u32>,
    pub(crate) schemes: Option<BTreeSet<String>>,
    pub(crate) domains: Option<BTreeSet<String>>,
    /// Currency codes.
    pub(crate) currencies: Option<BTreeSet<&'static str>>,
    pub(crate) dimension: Option<Dimension>,
    pub(crate) choices: Vec<Choice>,
    pub(crate) kind: Option<ReferenceKind>,
    pub(crate) min_items: Option<u32>,
    pub(crate) max_items: Option<u32>,
    pub(crate) unique_items: bool,
}

fn invalid(validation: &'static str, message: impl Into<String>) -> ValidationError {
    ValidationError::Invalid {
        validation,
        message: message.into(),
    }
}

/// A choice key: `[a-z0-9_-]{1,64}`.
fn valid_choice_key(key: &str) -> bool {
    (1..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// A lowercase host name: labels of letters, digits and hyphens.
fn valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

impl Validations {
    /// Checks `raw` against `field_type`: only the validations its type takes, each
    /// within its limits, required ones present.
    pub fn new(field_type: FieldType, raw: RawValidations) -> Result<Validations, ValidationError> {
        let base = field_type.base();
        let allowed = |name: &'static str, present: bool| {
            let fits = base.allows(name)
                || (field_type.is_list()
                    && ["min_items", "max_items", "unique_items"].contains(&name));
            if present && !fits {
                return Err(ValidationError::NotAllowed {
                    validation: name,
                    field_type,
                });
            }
            Ok(())
        };
        allowed("min_length", raw.min_length.is_some())?;
        allowed("max_length", raw.max_length.is_some())?;
        allowed("pattern", raw.pattern.is_some())?;
        allowed("min", raw.min.is_some())?;
        allowed("max", raw.max.is_some())?;
        allowed("max_scale", raw.max_scale.is_some())?;
        allowed("schemes", raw.schemes.is_some())?;
        allowed("domains", raw.domains.is_some())?;
        allowed("currencies", raw.currencies.is_some())?;
        allowed("dimension", raw.dimension.is_some())?;
        allowed("choices", raw.choices.is_some())?;
        allowed("kind", raw.kind.is_some())?;
        allowed("min_items", raw.min_items.is_some())?;
        allowed("max_items", raw.max_items.is_some())?;
        allowed("unique_items", raw.unique_items.is_some())?;
        let required = |name: &'static str| ValidationError::Required {
            validation: name,
            field_type,
        };

        let mut v = Validations {
            min_length: raw.min_length,
            max_length: raw.max_length,
            max_scale: raw.max_scale,
            min_items: raw.min_items,
            max_items: raw.max_items,
            unique_items: raw.unique_items.unwrap_or(false),
            ..Validations::default()
        };

        let length_cap = match base {
            BaseType::Text => MAX_TEXT_CHARS,
            _ => MAX_LONG_BYTES,
        } as u32;
        if v.max_length.is_some_and(|m| m == 0 || m > length_cap) {
            return Err(invalid("max_length", format!("is 1 to {length_cap}")));
        }
        if v.min_length.is_some_and(|m| m == 0 || m > length_cap) {
            return Err(invalid("min_length", format!("is 1 to {length_cap}")));
        }
        if let (Some(min), Some(max)) = (v.min_length, v.max_length)
            && min > max
        {
            return Err(invalid("min_length", "is more than `max_length`"));
        }
        if let Some(pattern) = &raw.pattern {
            v.pattern = Some(Pattern::new(pattern).map_err(|e| invalid("pattern", e))?);
        }
        if v.max_scale
            .is_some_and(|s| s > crate::limits::MAX_DECIMAL_SCALE)
        {
            return Err(invalid(
                "max_scale",
                format!("is 0 to {}", crate::limits::MAX_DECIMAL_SCALE),
            ));
        }
        if let Some(schemes) = raw.schemes {
            if schemes.is_empty() {
                return Err(invalid("schemes", "lists at least one scheme"));
            }
            if let Some(bad) = schemes.iter().find(|s| !URL_SCHEMES.contains(&s.as_str())) {
                return Err(invalid(
                    "schemes",
                    format!("has {bad:?}: schemes are https, http, mailto and tel"),
                ));
            }
            v.schemes = Some(schemes.into_iter().collect());
        }
        if let Some(domains) = raw.domains {
            if domains.is_empty() {
                return Err(invalid("domains", "lists at least one domain"));
            }
            if let Some(bad) = domains.iter().find(|d| !valid_domain(d)) {
                return Err(invalid(
                    "domains",
                    format!("has {bad:?}: domains are lowercase host names"),
                ));
            }
            v.domains = Some(domains.into_iter().collect());
        }
        if let Some(currencies) = raw.currencies {
            if currencies.is_empty() {
                return Err(invalid("currencies", "lists at least one currency"));
            }
            let mut set = BTreeSet::new();
            for c in &currencies {
                let currency = Currency::new(c)
                    .map_err(|_| invalid("currencies", format!("has {c:?}, not a currency")))?;
                set.insert(currency.as_str());
            }
            v.currencies = Some(set);
        }
        if base == BaseType::Measurement {
            let name = raw
                .dimension
                .as_deref()
                .ok_or_else(|| required("dimension"))?;
            v.dimension = Some(Dimension::parse(name).ok_or_else(|| {
                invalid(
                    "dimension",
                    format!("is {name:?}: weight, length or volume"),
                )
            })?);
        }
        if base == BaseType::Choice {
            let choices = raw.choices.ok_or_else(|| required("choices"))?;
            if choices.is_empty() || choices.len() > MAX_CHOICES {
                return Err(invalid(
                    "choices",
                    format!("lists 1 to {MAX_CHOICES} choices"),
                ));
            }
            let mut keys = BTreeSet::new();
            let mut checked = Vec::with_capacity(choices.len());
            for choice in choices {
                if !valid_choice_key(&choice.key) {
                    return Err(invalid(
                        "choices",
                        format!(
                            "has the key {:?}: keys are 1 to 64 lowercase letters, digits, `_` or `-`",
                            choice.key
                        ),
                    ));
                }
                if !keys.insert(choice.key.clone()) {
                    return Err(invalid(
                        "choices",
                        format!("has the key {:?} twice", choice.key),
                    ));
                }
                let label = hypha_core::required_text("label", &choice.label)
                    .map_err(|e| invalid("choices", format!("has a label that {e}")))?;
                checked.push(Choice {
                    key: choice.key,
                    label,
                });
            }
            v.choices = checked;
        }
        if base == BaseType::Reference {
            let name = raw.kind.as_deref().ok_or_else(|| required("kind"))?;
            v.kind = Some(ReferenceKind::parse(name).ok_or_else(|| {
                invalid(
                    "kind",
                    format!("is {name:?}: product, variant, category, customer or asset"),
                )
            })?);
        }
        let cap = MAX_LIST_ITEMS as u32;
        if v.max_items.is_some_and(|m| m == 0 || m > cap) {
            return Err(invalid("max_items", format!("is 1 to {cap}")));
        }
        if v.min_items.is_some_and(|m| m == 0 || m > cap) {
            return Err(invalid("min_items", format!("is 1 to {cap}")));
        }
        if let (Some(min), Some(max)) = (v.min_items, v.max_items)
            && min > max
        {
            return Err(invalid("min_items", "is more than `max_items`"));
        }
        if let Some(min) = &raw.min {
            v.min = Some(v.bound(base, "min", min)?);
        }
        if let Some(max) = &raw.max {
            v.max = Some(v.bound(base, "max", max)?);
        }
        if let (Some(min), Some(max)) = (v.min, v.max)
            && min.cmp_value(&max).is_gt()
        {
            return Err(invalid("min", "is more than `max`"));
        }
        Ok(v)
    }

    /// No validations: what stored values are loaded with (a value is parsed only by its
    /// type when loaded). For types with required validations, use the definition's.
    pub fn none() -> Validations {
        Validations::default()
    }

    /// A `min` or `max` in the field's own terms.
    fn bound(
        &self,
        base: BaseType,
        name: &'static str,
        json: &serde_json::Value,
    ) -> Result<Bound, ValidationError> {
        let bad = |what: &str| invalid(name, format!("must be {what}"));
        match base {
            BaseType::Integer | BaseType::Money => json
                .as_i64()
                .filter(|n| n.unsigned_abs() <= hypha_core::Money::MAX_AMOUNT as u64)
                .map(Bound::Integer)
                .ok_or_else(|| bad("a whole number within ±(2^53 − 1)")),
            BaseType::Decimal => json
                .as_str()
                .and_then(|s| Decimal::parse(s).ok())
                .map(Bound::Decimal)
                .ok_or_else(|| bad("a decimal string like \"12.50\"")),
            BaseType::Date => json
                .as_str()
                .and_then(crate::value::parse_date)
                .map(Bound::Date)
                .ok_or_else(|| bad("a date like \"2026-10-20\"")),
            BaseType::DateTime => json
                .as_str()
                .and_then(crate::value::parse_date_time)
                .map(Bound::DateTime)
                .ok_or_else(|| bad("an RFC 3339 date and time")),
            BaseType::Measurement => {
                let dimension = self.dimension.ok_or_else(|| bad("a measurement"))?;
                let measurement = crate::value::parse_measurement(json).ok_or_else(|| {
                    bad("a measurement like {\"value\": \"1.5\", \"unit\": \"kg\"}")
                })?;
                if measurement.unit.dimension() != dimension {
                    return Err(bad(&format!("in a unit of {}", dimension.as_str())));
                }
                Ok(Bound::Measurement(measurement))
            }
            _ => Err(ValidationError::NotAllowed {
                validation: name,
                field_type: FieldType::single(base),
            }),
        }
    }

    /// The validations as the API and storage spell them.
    pub fn to_raw(&self) -> RawValidations {
        let bound = |b: &Option<Bound>| -> Option<serde_json::Value> {
            b.as_ref().map(|b| match b {
                Bound::Integer(n) => serde_json::json!(n),
                Bound::Decimal(d) => serde_json::json!(d.to_string()),
                Bound::Date(d) => serde_json::json!(d.to_string()),
                Bound::DateTime(t) => serde_json::json!(crate::value::format_date_time(*t)),
                Bound::Measurement(m) => crate::value::measurement_json(*m),
            })
        };
        RawValidations {
            min_length: self.min_length,
            max_length: self.max_length,
            pattern: self.pattern.as_ref().map(|p| p.as_str().to_string()),
            min: bound(&self.min),
            max: bound(&self.max),
            max_scale: self.max_scale,
            schemes: self.schemes.as_ref().map(|s| s.iter().cloned().collect()),
            domains: self.domains.as_ref().map(|s| s.iter().cloned().collect()),
            currencies: self
                .currencies
                .as_ref()
                .map(|s| s.iter().map(|c| c.to_string()).collect()),
            dimension: self.dimension.map(|d| d.as_str().to_string()),
            choices: (!self.choices.is_empty()).then(|| self.choices.clone()),
            kind: self.kind.map(|k| k.as_str().to_string()),
            min_items: self.min_items,
            max_items: self.max_items,
            unique_items: self.unique_items.then_some(true),
        }
    }

    pub fn choices(&self) -> &[Choice] {
        &self.choices
    }

    pub fn kind(&self) -> Option<ReferenceKind> {
        self.kind
    }

    pub fn dimension(&self) -> Option<Dimension> {
        self.dimension
    }

    pub fn max_items(&self) -> Option<u32> {
        self.max_items
    }

    pub fn min_items(&self) -> Option<u32> {
        self.min_items
    }

    pub fn unique_items(&self) -> bool {
        self.unique_items
    }

    /// Whether `new` accepts less than `self`: a value valid under `self` may fail `new`.
    /// Then the stored values are checked before the change is applied. Removing or
    /// widening a validation never tightens.
    pub fn tightened_by(&self, new: &Validations) -> bool {
        // A lower bound that's new or higher; an upper bound that's new or lower.
        fn raised_bound(old: &Option<Bound>, new: &Option<Bound>) -> bool {
            match (old, new) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(o), Some(n)) => n.cmp_value(o).is_gt(),
            }
        }
        fn lowered_bound(old: &Option<Bound>, new: &Option<Bound>) -> bool {
            match (old, new) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(o), Some(n)) => n.cmp_value(o).is_lt(),
            }
        }
        fn raised<T: Ord>(old: &Option<T>, new: &Option<T>) -> bool {
            match (old, new) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(o), Some(n)) => n > o,
            }
        }
        fn lowered<T: Ord>(old: &Option<T>, new: &Option<T>) -> bool {
            match (old, new) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(o), Some(n)) => n < o,
            }
        }
        // A set of allowed things (`None`: anything) that lost something.
        fn narrowed<T: Ord>(old: &Option<BTreeSet<T>>, new: &Option<BTreeSet<T>>) -> bool {
            match (old, new) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(o), Some(n)) => !n.is_superset(o),
            }
        }
        let pattern_changed = match (&self.pattern, &new.pattern) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(o), Some(n)) => o != n,
        };
        let choices_lost = self
            .choices
            .iter()
            .any(|c| !new.choices.iter().any(|n| n.key == c.key));
        raised(&self.min_length, &new.min_length)
            || lowered(&self.max_length, &new.max_length)
            || pattern_changed
            || raised_bound(&self.min, &new.min)
            || lowered_bound(&self.max, &new.max)
            || lowered(&self.max_scale, &new.max_scale)
            || narrowed(&self.schemes, &new.schemes)
            || narrowed(&self.domains, &new.domains)
            || narrowed(&self.currencies, &new.currencies)
            || self.dimension != new.dimension
            || choices_lost
            || self.kind != new.kind
            || raised(&self.min_items, &new.min_items)
            || lowered(&self.max_items, &new.max_items)
            || (new.unique_items && !self.unique_items)
    }
}
