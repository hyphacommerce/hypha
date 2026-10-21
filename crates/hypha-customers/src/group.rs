//! Customer groups (design note 0032, §6, CD5): named, static sets of customers with a
//! stable ID other domains point at (prices, payment methods, later). A plain aggregate,
//! not a hierarchy; who's in it is its membership, kept apart like a category's
//! products (`CustomerGroupRepo::add_members`).

use std::collections::BTreeSet;

use hypha_core::{
    CustomerGroupId, CustomerId, Event, Handle, PendingEvent, Recorded, Stamped, Stamps, Subject,
    Version, Versioned, optional_text, required_text,
};
use serde::Serialize;

use crate::CustomerError;

pub const CUSTOMER_GROUP: &str = "customer_group";

/// `customer_group.created`.
#[derive(Debug, Serialize)]
pub struct CustomerGroupCreated {
    pub group: CustomerGroupId,
}

impl Event for CustomerGroupCreated {
    const TYPE: &'static str = "customer_group.created";
    const VERSION: u16 = 1;
}

/// `customer_group.updated`: which of `name`, `handle` and `description` changed.
#[derive(Debug, Serialize)]
pub struct CustomerGroupUpdated {
    pub group: CustomerGroupId,
    pub fields: Vec<&'static str>,
}

impl Event for CustomerGroupUpdated {
    const TYPE: &'static str = "customer_group.updated";
    const VERSION: u16 = 1;
}

/// `customer_group.deleted`, with the customers that were in it (their memberships went
/// with it).
#[derive(Debug, Serialize)]
pub struct CustomerGroupDeleted {
    pub group: CustomerGroupId,
    pub customers: Vec<CustomerId>,
}

impl Event for CustomerGroupDeleted {
    const TYPE: &'static str = "customer_group.deleted";
    const VERSION: u16 = 1;
}

/// `customer_group.members_added`: these customers are in the group now.
#[derive(Debug, Serialize)]
pub struct CustomerGroupMembersAdded {
    pub group: CustomerGroupId,
    pub customers: Vec<CustomerId>,
}

impl Event for CustomerGroupMembersAdded {
    const TYPE: &'static str = "customer_group.members_added";
    const VERSION: u16 = 1;
}

/// `customer_group.members_removed`: these customers aren't in the group any more.
#[derive(Debug, Serialize)]
pub struct CustomerGroupMembersRemoved {
    pub group: CustomerGroupId,
    pub customers: Vec<CustomerId>,
}

impl Event for CustomerGroupMembersRemoved {
    const TYPE: &'static str = "customer_group.members_removed";
    const VERSION: u16 = 1;
}

pub fn group_subject(group: CustomerGroupId) -> Subject {
    Subject::new(CUSTOMER_GROUP, group.as_uuid())
}

/// What changed since the group was loaded, coalesced into one event per write.
/// Compares as always equal, so it never makes two groups unequal.
#[derive(Debug, Clone, Default)]
struct Changes {
    created: bool,
    fields: BTreeSet<&'static str>,
}

impl PartialEq for Changes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changes {}

/// The description's limit, in characters.
pub const MAX_GROUP_DESCRIPTION_CHARS: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerGroup {
    id: CustomerGroupId,
    version: Version,
    stamps: Option<Stamps>,
    name: String,
    handle: Handle,
    description: Option<String>,
    changes: Changes,
}

impl Recorded for CustomerGroup {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let changes = std::mem::take(&mut self.changes);
        let subject = group_subject(self.id);
        if changes.created {
            return vec![PendingEvent::new(
                subject,
                &CustomerGroupCreated { group: self.id },
            )];
        }
        if changes.fields.is_empty() {
            return Vec::new();
        }
        vec![PendingEvent::new(
            subject,
            &CustomerGroupUpdated {
                group: self.id,
                fields: changes.fields.into_iter().collect(),
            },
        )]
    }
}

impl Versioned for CustomerGroup {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for CustomerGroup {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

fn description_of(description: Option<String>) -> Result<Option<String>, CustomerError> {
    let text = optional_text("description", description)?;
    if let Some(text) = &text
        && text.chars().count() > MAX_GROUP_DESCRIPTION_CHARS
    {
        return Err(hypha_core::TextError::TooLong("description").into());
    }
    Ok(text)
}

impl CustomerGroup {
    /// A new group; the name and handle must be unique among groups (the repository's
    /// keys, checked by the commands too).
    pub fn new(
        id: CustomerGroupId,
        name: &str,
        handle: Handle,
        description: Option<String>,
    ) -> Result<Self, CustomerError> {
        Ok(Self {
            id,
            version: Version::NEW,
            stamps: None,
            name: required_text("name", name)?,
            handle,
            description: description_of(description)?,
            changes: Changes {
                created: true,
                fields: BTreeSet::new(),
            },
        })
    }

    /// The shared text rule; the same name records nothing.
    pub fn rename(&mut self, name: &str) -> Result<(), CustomerError> {
        let name = required_text("name", name)?;
        if name != self.name {
            self.name = name;
            self.changes.fields.insert("name");
        }
        Ok(())
    }

    pub fn set_handle(&mut self, handle: Handle) {
        if handle != self.handle {
            self.handle = handle;
            self.changes.fields.insert("handle");
        }
    }

    /// Plain text up to 1,000 characters; blank clears it.
    pub fn set_description(&mut self, description: Option<String>) -> Result<(), CustomerError> {
        let description = description_of(description)?;
        if description != self.description {
            self.description = description;
            self.changes.fields.insert("description");
        }
        Ok(())
    }

    pub fn id(&self) -> CustomerGroupId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

/// Plain-data form of a `CustomerGroup` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerGroupSnapshot {
    pub id: CustomerGroupId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
    pub handle: String,
    pub description: Option<String>,
}

impl CustomerGroup {
    pub fn snapshot(&self) -> CustomerGroupSnapshot {
        CustomerGroupSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            name: self.name.clone(),
            handle: self.handle.as_str().to_string(),
            description: self.description.clone(),
        }
    }

    /// Rebuilds a group from storage, with the same rules as `new`.
    pub fn restore(s: CustomerGroupSnapshot) -> Result<Self, CustomerError> {
        let handle = Handle::new(&s.handle)?;
        let mut group = Self::new(s.id, &s.name, handle, s.description)?;
        group.version = s.version;
        group.stamps = s.stamps;
        group.changes = Changes::default();
        Ok(group)
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    fn group() -> CustomerGroup {
        CustomerGroup::new(
            CustomerGroupId::from_uuid(Uuid::from_u128(1)),
            "Wholesale",
            Handle::new("wholesale").unwrap(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn a_new_group_records_created_then_changes_as_updated() {
        let mut g = group();
        let created = g.take_events();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].event_type, "customer_group.created");
        g.rename("Wholesale ").unwrap();
        assert!(g.take_events().is_empty(), "trimmed to the same name");
        g.rename("Trade").unwrap();
        g.set_description(Some("Buys in bulk".to_string())).unwrap();
        let updated = g.take_events();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].data["fields"],
            serde_json::json!(["description", "name"])
        );
    }

    #[test]
    fn names_and_descriptions_follow_the_text_rules() {
        let mut g = group();
        assert_eq!(
            g.rename("  "),
            Err(CustomerError::Text(TextError::Missing("name")))
        );
        let long = "x".repeat(MAX_GROUP_DESCRIPTION_CHARS + 1);
        assert_eq!(
            g.set_description(Some(long)),
            Err(CustomerError::Text(TextError::TooLong("description")))
        );
        g.set_description(Some("  ".to_string())).unwrap();
        assert_eq!(g.description(), None);
    }

    #[test]
    fn restore_round_trips() {
        let mut g = group();
        g.set_description(Some("Buys in bulk".to_string())).unwrap();
        g.set_version(Version::new(3));
        assert_eq!(CustomerGroup::restore(g.snapshot()), Ok(g));
    }
}
