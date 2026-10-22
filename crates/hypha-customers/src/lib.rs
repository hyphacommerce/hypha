//! Customers and their address books.
//!
//! A leaf domain: it depends on core only. One customer per email; accounts and login
//! come later.

pub mod commands;
pub mod consent;
pub mod customer;
pub mod error;
pub mod events;
pub mod group;
pub mod groups;
pub mod list;
pub mod repo;

pub use commands::{
    CommandError, CreateCustomer, SaveAddress, UpdateCustomer, add_address, create_customer,
    find_or_create_customer, get_customer, list_customers, remove_address, set_marketing_consent,
    update_address, update_customer,
};
pub use consent::{
    Channel, ConsentChange, ConsentSource, ConsentState, MarketingConsent, OptInLevel,
};
pub use customer::{Customer, CustomerSnapshot, SavedAddress};
pub use error::{CustomerError, RestoreError};
pub use events::{
    CustomerCreated, CustomerDataRequested, CustomerDeleted, CustomerErasureCancelled,
    CustomerErasureRequested, CustomerRedacted, CustomerUpdated, MarketingConsentUpdated,
    customer_subject,
};
pub use group::{
    CUSTOMER_GROUP, CustomerGroup, CustomerGroupCreated, CustomerGroupDeleted,
    CustomerGroupMembersAdded, CustomerGroupMembersRemoved, CustomerGroupSnapshot,
    CustomerGroupUpdated, MAX_GROUP_DESCRIPTION_CHARS, group_subject,
};
pub use groups::{
    CreateCustomerGroup, GroupPorts, UpdateCustomerGroup, add_group_members, create_group,
    delete_group, get_group, groups_of_customers, list_group_members, list_groups,
    remove_group_members, set_customer_groups, update_group,
};
pub use list::{CustomerFilter, CustomerSort, CustomerSummary};
pub use repo::{
    CustomerGroupRepo, CustomerRepo, HasCustomerGroups, HasCustomers, StageCustomerGroups,
    StageCustomers,
};
