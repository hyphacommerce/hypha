//! Custom fields (design note 0033): typed fields on core resources (products, variants,
//! categories, customers, orders), defined at runtime as data, in the merchant's
//! `custom` namespace or an app's or spore's own. A leaf crate on core: it never sees the
//! owners, which routes and workflows check exist before handing them over (§6.1).
//!
//! - Definitions (`CustomFieldDefinition`): owner type, namespace, key and type are
//!   immutable (CF3); validations, access and capabilities may change, and a change that
//!   tightens validations is checked against the stored values first (§2).
//! - Values (`FieldValue`): one row per (definition, owner), typed JSON with derived
//!   index columns (`ValueRow`), written through a batch with one
//!   `<owner>.custom_fields_updated` event per owner (§4, §6.4).
//! - Access (`access`): who reads and writes what, per namespace, surface and
//!   personal data (§5).

pub mod access;
pub mod commands;
pub mod decimal;
pub mod definition;
pub mod error;
pub mod events;
pub mod handler;
pub mod limits;
pub mod namespace;
pub mod owner;
pub mod repo;
pub mod types;
pub mod value;

pub use access::{CallerKind, FieldCaller, ReadAccess, WriteDenied, read_access, write_access};
pub use commands::*;
pub use handler::{CustomFieldCleanup, clear_references};

pub use decimal::{Decimal, DecimalError};
pub use definition::{
    Access, AdminAccess, Capabilities, CustomFieldDefinition, DefinitionChange, DefinitionSnapshot,
    DefinitionSpec, StorefrontAccess,
};
pub use error::DefinitionError;
pub use events::{
    CategoryCustomFieldsUpdated, ChangeData, CustomFieldDefinitionCreated,
    CustomFieldDefinitionDeleted, CustomFieldDefinitionUpdated, CustomerCustomFieldsUpdated,
    FieldChange, OrderCustomFieldsUpdated, ProductCustomFieldsUpdated, fields_updated,
};

pub use namespace::{FieldKey, Key, KeyError, Namespace, NamespaceError};
pub use owner::{Owner, OwnerType};
pub use repo::{
    DefinitionRepo, HasCustomFieldDefinitions, HasCustomFieldValues, ItemsApplied, ItemsChange,
    ItemsProblem, ItemsResult, OwnerItems, ReferenceTargets, StageCustomFieldDefinitions,
    StageCustomFieldValues, StoredValue, ValueCount, ValueRepo, ValueRow, item_key, keys,
};
pub use types::{
    BaseType, Bound, Choice, Dimension, FieldType, FieldTypeError, Measurement, Pattern,
    RawValidations, Reference, ReferenceKind, Unit, ValidationError, Validations,
};
pub use value::{FieldValue, ValueProblem, codes};
