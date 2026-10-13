//! Customers and their address books (design note 0005).
//!
//! A leaf domain: it depends on core only. One customer per email; accounts and login
//! come later.

pub mod commands;
pub mod customer;
pub mod error;
pub mod repo;

pub use commands::{
    CommandError, CreateCustomer, SaveAddress, UpdateCustomer, add_address, create_customer,
    find_or_create_customer, get_customer, remove_address, update_address, update_customer,
};
pub use customer::{Customer, CustomerSnapshot, SavedAddress};
pub use error::CustomerError;
pub use repo::CustomerRepo;
