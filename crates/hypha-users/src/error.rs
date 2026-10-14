use hypha_core::{ErrorCode, TextError};
use thiserror::Error;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RoleError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error("a role needs at least one scope")]
    NoScopes,
    #[error("the owner role can't be changed")]
    OwnerRoleFixed,
}

impl ErrorCode for RoleError {
    fn code(&self) -> &'static str {
        match self {
            RoleError::Text(e) => e.code(),
            RoleError::NoScopes => "role_scopes_required",
            RoleError::OwnerRoleFixed => "owner_role_fixed",
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum UserError {
    #[error(transparent)]
    Text(#[from] TextError),
}

impl ErrorCode for UserError {
    fn code(&self) -> &'static str {
        match self {
            UserError::Text(e) => e.code(),
        }
    }
}
