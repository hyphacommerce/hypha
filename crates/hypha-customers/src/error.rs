use hypha_core::{AddressId, ErrorCode, HandleError, TextError};
use thiserror::Error;

use crate::consent::ConsentState;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CustomerError {
    #[error(transparent)]
    Text(#[from] TextError),
    /// A group's handle.
    #[error(transparent)]
    Handle(#[from] HandleError),
    #[error("address {0} not found")]
    AddressNotFound(AddressId),
    #[error("address {0} is in the address book more than once")]
    DuplicateAddress(AddressId),
    /// `not_subscribed` is the starting state and `redacted` is erasure's: neither can be
    /// asked for.
    #[error("marketing consent can't be set to {0}")]
    ConsentStateNotSettable(ConsentState),
    /// SMS consent needs somewhere to send.
    #[error("SMS marketing consent needs a phone number")]
    ConsentNeedsPhone,
    /// An erased customer is read-only.
    #[error("the customer was erased")]
    Erased,
    #[error("the customer's erasure is requested already")]
    ErasureAlreadyRequested,
    #[error("no erasure is requested for the customer")]
    ErasureNotRequested,
    /// A list filter naming more IDs than a list takes.
    #[error("`{field}` names more than {max} IDs")]
    TooManyFilterIds { field: &'static str, max: usize },
}

/// Why stored data can't be rebuilt into a customer. Only `Customer::restore` returns
/// it and adapters report it as a backend failure, so it has no API code.
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
            CustomerError::Handle(e) => e.code(),
            CustomerError::AddressNotFound(_) => "address_not_found",
            CustomerError::DuplicateAddress(_) => "duplicate_address",
            CustomerError::ConsentStateNotSettable(_) => "consent_state_not_settable",
            CustomerError::ConsentNeedsPhone => "consent_needs_phone",
            CustomerError::Erased => "customer_erased",
            CustomerError::ErasureAlreadyRequested => "erasure_already_requested",
            CustomerError::ErasureNotRequested => "erasure_not_requested",
            CustomerError::TooManyFilterIds { .. } => "too_many_filter_ids",
        }
    }
}
