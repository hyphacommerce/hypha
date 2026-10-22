//! Filters on custom fields: what a list route's `field[custom.material]=wool` becomes once
//! `hypha-custom-fields` has checked it against its definition (filterable, readable, the
//! operand parsed by the field's type). The products, categories, customers and orders list
//! filters carry them opaquely, and storage adapters turn each into a predicate on the
//! values' derived `scalar_text` / `scalar_num` columns or on the value itself (lists), so
//! those crates never depend on custom fields.

use crate::CustomFieldDefinitionId;

/// One filter on one field: owners whose value of `definition` matches `op`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldFilter {
    pub definition: CustomFieldDefinitionId,
    /// The field's key as the caller wrote it, `custom.material`: for error messages.
    pub key: String,
    pub op: FieldFilterOp,
}

/// How a value is compared. Operands are already in the field's index form: text
/// lowercased, numbers exact decimal strings in the field's base unit (see
/// `FilterScalar`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldFilterOp {
    /// A single value is one of these (`field[custom.material]=wool,cotton`).
    AnyOf(Vec<FilterScalar>),
    /// A list (`list.<t>`) holds any of these items, each as stored (canonical JSON)
    /// with strings lowercased (`hypha_custom_fields::list_item`): adapters match them
    /// against the value's derived `list_items` (`list_items @> '["vip"]'`), so text
    /// items match ignoring case, as single values do.
    ContainsAny(Vec<serde_json::Value>),
    Gt(FilterScalar),
    Gte(FilterScalar),
    Lt(FilterScalar),
    Lte(FilterScalar),
    /// The owner has a value (`true`) or has none (`false`).
    Exists(bool),
}

/// An operand: compared with a value's `scalar_text` (text, choice, url, reference,
/// boolean as `"true"`/`"false"`, integer digits) or `scalar_num` (integer, decimal,
/// money in minor units, measurement in its dimension's base unit, dates and times as
/// microseconds since the epoch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterScalar {
    /// Lowercased already.
    Text(String),
    /// An exact decimal string: `-12.5`, `1500`. Never a float.
    Number(String),
}
