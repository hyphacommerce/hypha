use hypha_core::{CustomerId, Email, RepoError};
use hypha_customers::{Customer, CustomerRepo, repo::keys};

use crate::table::Table;

#[derive(Default)]
pub struct InMemoryCustomerRepo {
    customers: Table<CustomerId, Customer>,
}

/// One customer per email.
fn unique(new: &Customer, other: &Customer) -> Option<&'static str> {
    (new.email() == other.email()).then_some(keys::CUSTOMER_EMAIL)
}

impl CustomerRepo for InMemoryCustomerRepo {
    async fn get(&self, id: CustomerId) -> Result<Option<Customer>, RepoError> {
        Ok(self.customers.get(&id))
    }

    async fn get_by_email(&self, email: &Email) -> Result<Option<Customer>, RepoError> {
        Ok(self.customers.find(|c| c.email() == email))
    }

    async fn insert(&self, customer: &mut Customer) -> Result<(), RepoError> {
        self.customers
            .insert(customer.id(), customer, keys::CUSTOMER_ID, &unique)
    }

    async fn update(&self, customer: &mut Customer) -> Result<(), RepoError> {
        self.customers.update(customer.id(), customer, &unique)
    }
}
