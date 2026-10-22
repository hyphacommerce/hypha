use hypha_core::{
    CustomerId, Email, Keyset, Page, PageRequest, Recorded, RepoError, Timestamp, Versioned,
    stamp_insert, stamp_update,
};
use hypha_customers::{
    Customer, CustomerFilter, CustomerRepo, CustomerSort, CustomerSummary, repo::keys,
};

use crate::{
    InMemoryOutbox,
    custom_fields::{SharedValues, fields_match, values_for},
    customer_groups::SharedGroupMembers,
    table::Table,
};

#[derive(Default)]
pub struct InMemoryCustomerRepo {
    pub(crate) customers: Table<CustomerId, Customer>,
    pub(crate) outbox: InMemoryOutbox,
    field_values: SharedValues,
    group_members: SharedGroupMembers,
}

impl InMemoryCustomerRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The same repo, filtering its list by the custom field values `fields` holds, as a
    /// database joins them.
    pub fn joining_fields(self, fields: &crate::InMemoryCustomFields) -> Self {
        Self {
            field_values: fields.shared_values(),
            ..self
        }
    }

    /// The same repo, filtering its list by `groups`' membership.
    pub fn joining_groups(self, groups: &crate::InMemoryCustomerGroupRepo) -> Self {
        Self {
            group_members: groups.shared_members(),
            ..self
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
    async fn list(
        &self,
        filter: &CustomerFilter,
        sort: CustomerSort,
        page: PageRequest<Keyset<CustomerId>>,
    ) -> Result<Page<CustomerSummary>, RepoError> {
        if let Some(after) = &page.after
            && !sort.fits(&after.key)
        {
            return Err(RepoError::Backend(format!(
                "a {:?} position for a {sort:?} list",
                after.key
            )));
        }
        let values = values_for(&self.field_values, filter.fields());
        let in_group: Option<std::collections::HashSet<CustomerId>> = filter.group().map(|g| {
            self.group_members
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&g)
                .cloned()
                .unwrap_or_default()
        });
        Ok(self.customers.page_sorted(
            |c| {
                filter.matches(c)
                    && fields_match(values.as_ref(), c.id().as_uuid(), filter.fields())
                    && in_group
                        .as_ref()
                        .is_none_or(|members| members.contains(&c.id()))
            },
            &page,
            sort.direction(),
            Customer::summary,
            |s: &CustomerSummary| sort.key(s),
        ))
    }

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

    async fn due_for_erasure(
        &self,
        now: Timestamp,
        limit: u32,
    ) -> Result<Vec<CustomerId>, RepoError> {
        let mut due: Vec<(Timestamp, CustomerId)> = self
            .customers
            .all()
            .into_iter()
            .filter(|c| !c.is_erased())
            .filter_map(|c| {
                c.erasure_due_at()
                    .filter(|at| *at <= now)
                    .map(|at| (at, c.id()))
            })
            .collect();
        due.sort();
        Ok(due
            .into_iter()
            .take(limit as usize)
            .map(|(_, id)| id)
            .collect())
    }
}
