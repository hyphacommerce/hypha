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
