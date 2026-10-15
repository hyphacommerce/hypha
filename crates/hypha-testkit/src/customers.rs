use hypha_core::{CustomerId, Email, Recorded, RepoError, Versioned, stamp_insert, stamp_update};
use hypha_customers::{Customer, CustomerRepo, repo::keys};

use crate::{InMemoryOutbox, table::Table};

#[derive(Default)]
pub struct InMemoryCustomerRepo {
    pub(crate) customers: Table<CustomerId, Customer>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryCustomerRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The events this repo wrote (into its outbox, shared or not).
    pub fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }
}

/// One customer per email.
pub(crate) fn unique(new: &Customer, other: &Customer) -> Option<&'static str> {
    (new.email() == other.email()).then_some(keys::CUSTOMER_EMAIL)
}

impl InMemoryCustomerRepo {
    /// The next `n` lookups by a unique key (and `list_all`, where there is one) find
    /// nothing, as if another request inserted the row right after: for testing a
    /// command that loses the race between its pre-check and its write.
    pub fn hide_next_lookups(&self, n: usize) {
        self.customers.hide_next_lookups(n);
    }
}

impl CustomerRepo for InMemoryCustomerRepo {
    async fn get(&self, id: CustomerId) -> Result<Option<Customer>, RepoError> {
        Ok(self.customers.get(&id))
    }

    async fn get_by_email(&self, email: &Email) -> Result<Option<Customer>, RepoError> {
        Ok(self.customers.find(|c| c.email() == email))
    }

    async fn insert(&self, customer: &mut Customer) -> Result<(), RepoError> {
        stamp_insert(customer, self.outbox.now());
        let events = customer.take_events();
        self.customers
            .insert(customer.id(), customer, keys::CUSTOMER_ID, &unique)?;
        self.outbox.write(events, Some(customer.version()));
        Ok(())
    }

    async fn update(&self, customer: &mut Customer) -> Result<(), RepoError> {
        stamp_update(customer, self.outbox.now());
        let events = customer.take_events();
        self.customers.update(customer.id(), customer, &unique)?;
        self.outbox.write(events, Some(customer.version()));
        Ok(())
    }
}
