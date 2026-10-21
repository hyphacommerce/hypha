use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

use hypha_core::{
    CustomerGroupId, CustomerId, Page, PageRequest, Recorded, RepoError, Versioned, stamp_insert,
    stamp_update,
};
use hypha_customers::{CustomerGroup, CustomerGroupRepo, repo::keys};

use crate::{
    InMemoryOutbox,
    table::{Table, missing, page_of},
};

/// Membership: each group's customers. Shared with the customer repo, whose list filters
/// by group (design note 0032, §6), as a database joins the tables.
pub(crate) type GroupMembers = HashMap<CustomerGroupId, HashSet<CustomerId>>;

pub(crate) type SharedGroupMembers = Arc<Mutex<GroupMembers>>;

#[derive(Default)]
pub struct InMemoryCustomerGroupRepo {
    pub(crate) groups: Table<CustomerGroupId, CustomerGroup>,
    pub(crate) members: SharedGroupMembers,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryCustomerGroupRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// Membership, locked, for a batch (`InMemoryPorts::commit`).
    pub(crate) fn lock_members(&self) -> MutexGuard<'_, GroupMembers> {
        self.members.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn shared_members(&self) -> SharedGroupMembers {
        self.members.clone()
    }
}

/// Handles are unique among groups, and so are names ignoring case.
pub(crate) fn unique(new: &CustomerGroup, other: &CustomerGroup) -> Option<&'static str> {
    if new.handle() == other.handle() {
        return Some(keys::CUSTOMER_GROUP_HANDLE);
    }
    new.name()
        .eq_ignore_ascii_case(other.name())
        .then_some(keys::CUSTOMER_GROUP_NAME)
}

impl CustomerGroupRepo for InMemoryCustomerGroupRepo {
    async fn get(&self, id: CustomerGroupId) -> Result<Option<CustomerGroup>, RepoError> {
        Ok(self.groups.get(&id))
    }

    async fn get_by_handle(&self, handle: &str) -> Result<Option<CustomerGroup>, RepoError> {
        Ok(self.groups.find(|g| g.handle().as_str() == handle))
    }

    async fn list_all(&self) -> Result<Vec<CustomerGroup>, RepoError> {
        let mut all = self.groups.all();
        all.sort_by(|a, b| {
            a.name()
                .to_lowercase()
                .cmp(&b.name().to_lowercase())
                .then(a.id().cmp(&b.id()))
        });
        Ok(all)
    }

    async fn insert(&self, group: &mut CustomerGroup) -> Result<(), RepoError> {
        stamp_insert(group, self.outbox.now());
        let events = group.take_events();
        self.groups
            .insert(group.id(), group, keys::CUSTOMER_GROUP_ID, &unique)?;
        self.outbox.write(events, Some(group.version()));
        Ok(())
    }

    async fn update(&self, group: &mut CustomerGroup) -> Result<(), RepoError> {
        stamp_update(group, self.outbox.now());
        let events = group.take_events();
        self.groups.update(group.id(), group, &unique)?;
        self.outbox.write(events, Some(group.version()));
        Ok(())
    }

    async fn add_members(
        &self,
        group: CustomerGroupId,
        customers: &[CustomerId],
    ) -> Result<(), RepoError> {
        if self.groups.get(&group).is_none() {
            return Err(missing("customer group", group));
        }
        self.lock_members()
            .entry(group)
            .or_default()
            .extend(customers.iter().copied());
        Ok(())
    }

    async fn member_page(
        &self,
        group: CustomerGroupId,
        page: PageRequest<CustomerId>,
    ) -> Result<Page<CustomerId>, RepoError> {
        let members = self.lock_members();
        let found = page_of(
            members.get(&group).into_iter().flatten().map(|c| (*c, ())),
            page,
        );
        Ok(Page {
            items: found.items.into_iter().map(|(c, ())| c).collect(),
            has_more: found.has_more,
        })
    }

    async fn members_in(&self, group: CustomerGroupId) -> Result<Vec<CustomerId>, RepoError> {
        let members = self.lock_members();
        let mut found: Vec<CustomerId> =
            members.get(&group).into_iter().flatten().copied().collect();
        found.sort();
        Ok(found)
    }

    async fn groups_of(
        &self,
        customers: &[CustomerId],
    ) -> Result<HashMap<CustomerId, Vec<CustomerGroupId>>, RepoError> {
        let mut found: HashMap<CustomerId, Vec<CustomerGroupId>> = HashMap::new();
        for (group, members) in self.lock_members().iter() {
            for customer in customers.iter().filter(|c| members.contains(c)) {
                found.entry(*customer).or_default().push(*group);
            }
        }
        for groups in found.values_mut() {
            groups.sort();
            groups.dedup();
        }
        Ok(found)
    }
}
