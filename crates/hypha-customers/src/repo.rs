use std::collections::HashMap;

use hypha_core::{
    CustomerGroupId, CustomerId, Email, Keyset, Page, PageRequest, RepoError, Timestamp,
};

use crate::{Customer, CustomerFilter, CustomerGroup, CustomerSort, CustomerSummary};

pub mod keys {
    pub const CUSTOMER_ID: &str = "customer_id";
    /// One customer per email.
    pub const CUSTOMER_EMAIL: &str = "customer_email";
    pub const CUSTOMER_GROUP_ID: &str = "customer_group_id";
    /// A group's handle is unique among groups.
    pub const CUSTOMER_GROUP_HANDLE: &str = "customer_group_handle";
    /// So is its name, ignoring case.
    pub const CUSTOMER_GROUP_NAME: &str = "customer_group_name";
}

#[allow(async_fn_in_trait)]
pub trait CustomerRepo {
    async fn get(&self, id: CustomerId) -> Result<Option<Customer>, RepoError>;
    async fn get_by_email(&self, email: &Email) -> Result<Option<Customer>, RepoError>;
    /// A page of customer summaries in `sort`'s order: those after `page.after` by
    /// `CustomerSort::key` and then ID, only those `filter` matches. Each equals
    /// `Customer::summary` of the stored customer. A position whose key doesn't fit the
    /// sort (`CustomerSort::fits`) is a `Backend` error.
    async fn list(
        &self,
        filter: &CustomerFilter,
        sort: CustomerSort,
        page: PageRequest<Keyset<CustomerId>>,
    ) -> Result<Page<CustomerSummary>, RepoError>;
    async fn insert(&self, customer: &mut Customer) -> Result<(), RepoError>;
    async fn update(&self, customer: &mut Customer) -> Result<(), RepoError>;
    /// Up to `limit` customers whose requested erasure is due at `now` (`erasure_due_at <=
    /// now`, not erased yet), oldest due first: the erasure job's batch.
    async fn due_for_erasure(
        &self,
        now: Timestamp,
        limit: u32,
    ) -> Result<Vec<CustomerId>, RepoError>;
}

/// Stages customer writes in a `hypha_core::HasBatch` batch; the adapter's batch type
/// implements it.
pub trait StageCustomers {
    /// Stages what `CustomerRepo::insert` would write (checkout creating a guest's
    /// customer); `customer` is at version 1 once the batch commits. Enforces
    /// `CUSTOMER_EMAIL` on commit.
    fn insert_customer(&mut self, customer: &mut Customer);
    /// Stages what `CustomerRepo::update` would write (checkout recording a consent,
    /// erasure), and moves `customer` to the version it has once the batch commits
    /// (stale then: `StaleVersion`, nothing written).
    fn update_customer(&mut self, customer: &mut Customer);
    /// Removes the customer at the version it was loaded at, with its address book and
    /// memberships, and writes its events (`Customer::delete` records
    /// `customer.deleted`). The commit fails with `StaleVersion` if the stored customer
    /// has another version or is gone.
    fn delete_customer(&mut self, customer: &mut Customer);
}

/// Customer groups and who's in each. Membership is read and written in sets, like a
/// category's products.
#[allow(async_fn_in_trait)]
pub trait CustomerGroupRepo {
    async fn get(&self, id: CustomerGroupId) -> Result<Option<CustomerGroup>, RepoError>;
    async fn get_by_handle(&self, handle: &str) -> Result<Option<CustomerGroup>, RepoError>;
    /// Every group, by name ignoring case then ID. A store has few groups, so they come
    /// in one list.
    async fn list_all(&self) -> Result<Vec<CustomerGroup>, RepoError>;
    /// Stores a new group (version 1). Enforces `CUSTOMER_GROUP_ID`,
    /// `CUSTOMER_GROUP_HANDLE` and `CUSTOMER_GROUP_NAME`.
    async fn insert(&self, group: &mut CustomerGroup) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, group: &mut CustomerGroup) -> Result<(), RepoError>;
    /// Adds membership, ignoring customers already in the group. Records no event:
    /// commands stage it (`StageCustomerGroups`) with `customer_group.members_added`;
    /// this is for setting up data.
    ///
    /// Preconditions: the group and the customers exist. Otherwise adapters fail rather
    /// than store orphaned membership.
    async fn add_members(
        &self,
        group: CustomerGroupId,
        customers: &[CustomerId],
    ) -> Result<(), RepoError>;
    /// A page of the group's customers, newest first.
    async fn member_page(
        &self,
        group: CustomerGroupId,
        page: PageRequest<CustomerId>,
    ) -> Result<Page<CustomerId>, RepoError>;
    /// Every customer in the group, sorted: what deleting it records.
    async fn members_in(&self, group: CustomerGroupId) -> Result<Vec<CustomerId>, RepoError>;
    /// Each of `customers`' groups, sorted; a customer in none is absent.
    async fn groups_of(
        &self,
        customers: &[CustomerId],
    ) -> Result<HashMap<CustomerId, Vec<CustomerGroupId>>, RepoError>;
}

/// Group writes as a batch stages them, with their events (`StageEvents` for
/// membership). Staging moves a group to the version it gets on commit.
pub trait StageCustomerGroups {
    /// A new group; the commit enforces `keys`.
    fn insert_group(&mut self, group: &mut CustomerGroup);
    /// A changed group; the commit fails with `StaleVersion` from an old copy.
    fn update_group(&mut self, group: &mut CustomerGroup);
    /// Removes the group and its membership (never its customers).
    fn delete_group(&mut self, group: CustomerGroupId);
    /// Same preconditions as `CustomerGroupRepo::add_members`.
    fn add_group_members(&mut self, group: CustomerGroupId, customers: &[CustomerId]);
    fn remove_group_members(&mut self, group: CustomerGroupId, customers: &[CustomerId]);
}

pub trait HasCustomerGroups {
    type CustomerGroups: CustomerGroupRepo;
    fn customer_groups(&self) -> &Self::CustomerGroups;
}

impl<T: HasCustomerGroups + ?Sized> HasCustomerGroups for &T {
    type CustomerGroups = T::CustomerGroups;
    fn customer_groups(&self) -> &Self::CustomerGroups {
        (**self).customer_groups()
    }
}

/// Whatever holds the `CustomerRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use; `hypha_cart::CartPorts` is
/// the union of all of them.
pub trait HasCustomers {
    type Customers: CustomerRepo;
    fn customers(&self) -> &Self::Customers;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasCustomers + ?Sized> HasCustomers for &T {
    type Customers = T::Customers;
    fn customers(&self) -> &Self::Customers {
        (**self).customers()
    }
}
