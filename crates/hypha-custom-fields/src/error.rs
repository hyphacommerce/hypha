use hypha_core::{ErrorCode, TextError};
use thiserror::Error;

use crate::{
    namespace::{KeyError, NamespaceError},
    owner::OwnerType,
    types::{FieldType, FieldTypeError, ValidationError},
};

/// A definition breaking its own rules.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DefinitionError {
    #[error(transparent)]
    Key(#[from] KeyError),
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    #[error(transparent)]
    FieldType(#[from] FieldTypeError),
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error(transparent)]
    Text(#[from] TextError),
    #[error("description is longer than {max} characters")]
    DescriptionTooLong { max: usize },
    #[error("{0} fields can't be personal: only customer and order fields can")]
    PersonalNotAllowed(OwnerType),
    #[error("only an app's or a spore's fields can be private")]
    PrivateNotAllowed,
    #[error("{0} fields can't be unique: only text, integer and url fields can")]
    UniqueNotAllowed(FieldType),
    #[error("{0} fields can't be filterable")]
    FilterableNotAllowed(FieldType),
    #[error("the merchant's own fields are always read_write in the admin")]
    AdminAccessNotAllowed,
}

impl ErrorCode for DefinitionError {
    fn code(&self) -> &'static str {
        match self {
            DefinitionError::Key(e) => e.code(),
            DefinitionError::Namespace(e) => e.code(),
            DefinitionError::FieldType(e) => e.code(),
            DefinitionError::Validation(e) => e.code(),
            DefinitionError::Text(e) => e.code(),
            DefinitionError::DescriptionTooLong { .. } => "description_too_long",
            DefinitionError::PersonalNotAllowed(_) => "personal_not_allowed",
            DefinitionError::PrivateNotAllowed => "private_not_allowed",
            DefinitionError::UniqueNotAllowed(_) => "unique_not_allowed",
            DefinitionError::FilterableNotAllowed(_) => "filterable_not_allowed",
            DefinitionError::AdminAccessNotAllowed => "admin_access_not_allowed",
        }
    }
}
