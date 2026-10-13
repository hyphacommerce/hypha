use hypha_core::{AddressId, ErrorCode, TextError};
use thiserror::Error;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CustomerError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error("address {0} not found")]
    AddressNotFound(AddressId),
    #[error("address {0} is in the address book more than once")]
    DuplicateAddress(AddressId),
}

/// Why stored data can't be rebuilt into a customer. Only `Customer::restore` returns
/// it and adapters report it as a backend failure, so it has no API code (review plan D4).
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RestoreError {
    #[error(transparent)]
    Customer(#[from] CustomerError),
    #[error("default address {0} is not in the address book")]
    UnknownDefaultAddress(AddressId),
}

impl ErrorCode for CustomerError {
    fn code(&self) -> &'static str {
        match self {
            CustomerError::Text(e) => e.code(),
            CustomerError::AddressNotFound(_) => "address_not_found",
            CustomerError::DuplicateAddress(_) => "duplicate_address",
        }
    }
}
