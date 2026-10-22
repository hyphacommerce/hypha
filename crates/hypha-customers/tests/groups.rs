//! Customer groups and their members over a store in memory: what each write records.

use hypha_core::{Actor, AuthError, Ctx, CustomerGroupId, CustomerId, Handle, Scope, UserId};
use hypha_customers::{
    CommandError, CreateCustomer, CreateCustomerGroup, UpdateCustomerGroup, add_group_members,
    create_customer, create_group, delete_group, remove_group_members, set_customer_groups,
    update_group,
};
use hypha_testkit::{InMemoryPorts, SequentialIds};
use pollster::block_on;
use serde_json::{Value, json};
use uuid::Uuid;

struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    ctx: Ctx,
}

impl World {
    fn new() -> Self {
        Self {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
            ctx: Ctx::system(),
        }
    }

    fn customer(&self, mail: &str) -> CustomerId {
        let input = CreateCustomer {
            email: mail.to_string(),
            first_name: None,
            last_name: None,
            phone: None,
        };
        block_on(create_customer(
            &self.ctx,
            &self.ports.customers,
            &self.ids,
            input,
        ))
        .unwrap()
        .id()
    }

    fn group(&self, name: &str, handle: &str) -> CustomerGroupId {
        let input = CreateCustomerGroup {
            name: name.to_string(),
            handle: handle.to_string(),
            description: None,
        };
        block_on(create_group(&self.ctx, &self.ports, &self.ids, input))
            .unwrap()
            .id()
    }

    fn add(&self, group: CustomerGroupId, customers: &[CustomerId]) {
        block_on(add_group_members(
            &self.ctx,
            &self.ports,
            group,
            customers.to_vec(),
        ))
        .unwrap();
    }

    fn remove(&self, group: CustomerGroupId, customers: &[CustomerId]) {
        block_on(remove_group_members(
            &self.ctx,
            &self.ports,
            group,
            customers.to_vec(),
        ))
        .unwrap();
    }

    fn set_groups(&self, customer: CustomerId, groups: &[CustomerGroupId]) {
        block_on(set_customer_groups(
            &self.ctx,
            &self.ports,
            customer,
            groups.to_vec(),
        ))
        .unwrap();
    }

    /// The events of `event_type` recorded, as their data.
    fn events(&self, event_type: &str) -> Vec<Value> {
        self.ports
            .outbox
            .events()
            .into_iter()
            .filter(|e| e.event_type == event_type)
            .map(|e| e.data)
            .collect()
    }

    fn event_count(&self) -> usize {
        self.ports.outbox.events().len()
    }
}

fn members(group: CustomerGroupId, customers: &[CustomerId]) -> Value {
    json!({ "group": group, "customers": customers })
}

#[test]
fn adding_members_records_only_the_new_ones() {
    let w = World::new();
    let (ana, ivo) = (w.customer("ana@example.hr"), w.customer("ivo@example.hr"));
    let vip = w.group("VIP", "vip");
    w.add(vip, &[ana]);

    w.add(vip, &[ana, ivo]);

    assert_eq!(
        w.events("customer_group.members_added"),
        vec![members(vip, &[ana]), members(vip, &[ivo])]
    );
}

#[test]
fn adding_only_existing_members_writes_nothing() {
    let w = World::new();
    let ana = w.customer("ana@example.hr");
    let vip = w.group("VIP", "vip");
    w.add(vip, &[ana]);
    let before = w.event_count();

    w.add(vip, &[ana]);

    assert_eq!(w.event_count(), before);
}

#[test]
fn removing_members_records_only_those_that_were_in() {
    let w = World::new();
    let (ana, ivo) = (w.customer("ana@example.hr"), w.customer("ivo@example.hr"));
    let vip = w.group("VIP", "vip");
    w.add(vip, &[ana]);

    w.remove(vip, &[ana, ivo]);

    assert_eq!(
        w.events("customer_group.members_removed"),
        vec![members(vip, &[ana])]
    );
}

#[test]
fn removing_non_members_writes_nothing() {
    let w = World::new();
    let ana = w.customer("ana@example.hr");
    let vip = w.group("VIP", "vip");
    let before = w.event_count();

    w.remove(vip, &[ana]);

    assert_eq!(w.event_count(), before);
}

#[test]
fn setting_a_customers_groups_records_one_event_per_group_changed() {
    let w = World::new();
    let ana = w.customer("ana@example.hr");
    let (vip, staff, press) = (
        w.group("VIP", "vip"),
        w.group("Staff", "staff"),
        w.group("Press", "press"),
    );
    w.set_groups(ana, &[vip, staff]);

    w.set_groups(ana, &[staff, press]);

    assert_eq!(
        w.events("customer_group.members_added"),
        vec![
            members(vip, &[ana]),
            members(staff, &[ana]),
            members(press, &[ana])
        ]
    );
    assert_eq!(
        w.events("customer_group.members_removed"),
        vec![members(vip, &[ana])]
    );
}

#[test]
fn setting_the_same_groups_writes_nothing() {
    let w = World::new();
    let ana = w.customer("ana@example.hr");
    let vip = w.group("VIP", "vip");
    w.set_groups(ana, &[vip]);
    let before = w.event_count();

    w.set_groups(ana, &[vip]);

    assert_eq!(w.event_count(), before);
}

#[test]
fn deleting_a_group_records_the_members_it_had() {
    let w = World::new();
    let (ana, ivo) = (w.customer("ana@example.hr"), w.customer("ivo@example.hr"));
    let vip = w.group("VIP", "vip");
    w.add(vip, &[ana, ivo]);

    block_on(delete_group(&w.ctx, &w.ports, vip)).unwrap();

    let mut expected = vec![ana, ivo];
    expected.sort();
    assert_eq!(
        w.events("customer_group.deleted"),
        vec![members(vip, &expected)]
    );
}

#[test]
fn a_group_may_keep_its_own_handle_and_name() {
    let w = World::new();
    let vip = w.group("VIP", "vip");
    let input = UpdateCustomerGroup {
        name: Some("VIP".to_string()),
        handle: Some("vip".to_string()),
        description: None,
    };
    assert!(block_on(update_group(&w.ctx, &w.ports, vip, input)).is_ok());
}

#[test]
fn another_groups_handle_is_taken() {
    let w = World::new();
    w.group("VIP", "vip");
    let staff = w.group("Staff", "staff");
    let input = UpdateCustomerGroup {
        handle: Some("vip".to_string()),
        ..UpdateCustomerGroup::default()
    };
    assert_eq!(
        block_on(update_group(&w.ctx, &w.ports, staff, input)).map(|g| g.id()),
        Err(CommandError::GroupHandleTaken(Handle::new("vip").unwrap()))
    );
}

#[test]
fn another_groups_name_is_taken() {
    let w = World::new();
    w.group("VIP", "vip");
    let staff = w.group("Staff", "staff");
    let input = UpdateCustomerGroup {
        name: Some("vip".to_string()),
        ..UpdateCustomerGroup::default()
    };
    assert_eq!(
        block_on(update_group(&w.ctx, &w.ports, staff, input)).map(|g| g.id()),
        Err(CommandError::GroupNameTaken("vip".to_string()))
    );
}

#[test]
fn group_writes_need_customers_write() {
    let w = World::new();
    let ana = w.customer("ana@example.hr");
    let vip = w.group("VIP", "vip");
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::CustomersRead],
    );
    let create = CreateCustomerGroup {
        name: "Staff".to_string(),
        handle: "staff".to_string(),
        description: None,
    };
    let results = [
        block_on(create_group(&reader, &w.ports, &w.ids, create)).map(|_| ()),
        block_on(update_group(
            &reader,
            &w.ports,
            vip,
            UpdateCustomerGroup::default(),
        ))
        .map(|_| ()),
        block_on(delete_group(&reader, &w.ports, vip)),
        block_on(add_group_members(&reader, &w.ports, vip, vec![ana])),
        block_on(remove_group_members(&reader, &w.ports, vip, vec![ana])),
        block_on(set_customer_groups(&reader, &w.ports, ana, vec![vip])).map(|_| ()),
    ];
    for result in results {
        assert_eq!(
            result,
            Err(CommandError::Auth(AuthError::MissingScope(
                Scope::CustomersWrite
            )))
        );
    }
}
