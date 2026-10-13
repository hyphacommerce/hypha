use hypha_core::{AddressId, ErrorCode};
use thiserror::Error;

use crate::Customer;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CustomerError {
    #[error("{0} is longer than {max} characters", max = Customer::MAX_FIELD_LEN)]
    FieldTooLong(&'static str),
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
            CustomerError::FieldTooLong(_) => "field_too_long",
            CustomerError::AddressNotFound(_) => "address_not_found",
            CustomerError::DuplicateAddress(_) => "duplicate_address",
        }
    }
}
