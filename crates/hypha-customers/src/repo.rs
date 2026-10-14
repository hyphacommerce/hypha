use hypha_core::{CustomerId, Email, RepoError};

use crate::Customer;

pub mod keys {
    pub const CUSTOMER_ID: &str = "customer_id";
    /// One customer per email (design note 0005, C2).
    pub const CUSTOMER_EMAIL: &str = "customer_email";
}

#[allow(async_fn_in_trait)]
pub trait CustomerRepo {
    async fn get(&self, id: CustomerId) -> Result<Option<Customer>, RepoError>;
    async fn get_by_email(&self, email: &Email) -> Result<Option<Customer>, RepoError>;
    async fn insert(&self, customer: &mut Customer) -> Result<(), RepoError>;
    async fn update(&self, customer: &mut Customer) -> Result<(), RepoError>;
}

/// Stages customer writes in a `hypha_core::HasBatch` batch; the adapter's batch type
/// implements it.
pub trait StageCustomers {
    /// Stages what `CustomerRepo::insert` would write (checkout creating a guest's
    /// customer); `customer` is at version 1 once the batch commits. Enforces
    /// `CUSTOMER_EMAIL` on commit.
    fn insert_customer(&mut self, customer: &mut Customer);
}

/// Whatever holds the `CustomerRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use (review plan D1); `hypha_cart::CartPorts` is
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
