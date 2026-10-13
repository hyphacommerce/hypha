use std::cell::Cell;

use hypha_core::{
    Actor, AddressError, AddressId, AddressParts, AuthError, CountryCode, Ctx, CustomerId, Email,
    EmailError, RepoError, Scope, UserId,
};
use hypha_customers::{
    CommandError, CreateCustomer, Customer, CustomerError, CustomerRepo, SaveAddress,
    UpdateCustomer, add_address, create_customer, find_or_create_customer, get_customer,
    remove_address, update_address, update_customer,
};
use hypha_testkit::{InMemoryCustomerRepo, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn email(s: &str) -> Email {
    Email::new(s).unwrap()
}

fn parts(city: &str) -> AddressParts {
    AddressParts {
        first_name: Some("Ana".to_string()),
        last_name: "Horvat".to_string(),
        company: None,
        line1: "Ilica 1".to_string(),
        line2: None,
        city: city.to_string(),
        province: None,
        postal_code: Some("10000".to_string()),
        country: CountryCode::new("HR").unwrap(),
        phone: None,
    }
}

fn save(city: &str, default_shipping: bool, default_billing: bool) -> SaveAddress {
    SaveAddress {
        address: parts(city),
        default_shipping,
        default_billing,
    }
}

fn reader() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::CustomersRead],
    )
}

struct World {
    customers: InMemoryCustomerRepo,
    ids: SequentialIds,
}

impl World {
    fn new() -> Self {
        Self {
            customers: InMemoryCustomerRepo::default(),
            ids: SequentialIds::default(),
        }
    }

    fn create(&self, mail: &str) -> Result<Customer, CommandError> {
        let input = CreateCustomer {
            email: mail.to_string(),
            first_name: Some("Ana".to_string()),
            last_name: None,
            phone: None,
        };
        block_on(create_customer(
            &Ctx::system(),
            &self.customers,
            &self.ids,
            input,
        ))
    }

    fn update(&self, id: CustomerId, input: UpdateCustomer) -> Result<Customer, CommandError> {
        block_on(update_customer(&Ctx::system(), &self.customers, id, input))
    }

    fn add(&self, id: CustomerId, input: SaveAddress) -> Result<Customer, CommandError> {
        block_on(add_address(
            &Ctx::system(),
            &self.customers,
            &self.ids,
            id,
            input,
        ))
    }

    fn stored(&self, id: CustomerId) -> Customer {
        block_on(self.customers.get(id)).unwrap().unwrap()
    }
}

// ---- create ----

#[test]
fn create_stores_the_customer_with_a_normalized_email() {
    let world = World::new();
    let c = world.create(" Ana@Example.HR ").unwrap();
    assert_eq!(c.email(), &email("ana@example.hr"));
    assert_eq!(c.first_name(), Some("Ana"));
    assert_eq!(world.stored(c.id()), c);
}

#[test]
fn an_email_belongs_to_one_customer_in_any_case() {
    let world = World::new();
    let first = world.create("ana@example.hr").unwrap();
    assert_eq!(
        world.create("ANA@example.hr"),
        Err(CommandError::CustomerExists {
            email: email("ana@example.hr"),
            customer: first.id()
        })
    );
}

#[test]
fn create_rejects_an_invalid_email() {
    assert_eq!(
        World::new().create("ana"),
        Err(CommandError::Email(EmailError::Invalid("ana".to_string())))
    );
}

#[test]
fn create_requires_write_scope() {
    let world = World::new();
    let input = CreateCustomer {
        email: "ana@example.hr".to_string(),
        first_name: None,
        last_name: None,
        phone: None,
    };
    assert_eq!(
        block_on(create_customer(
            &reader(),
            &world.customers,
            &world.ids,
            input
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::CustomersWrite
        )))
    );
}

/// Hides an existing customer from the command's first email lookup, as if another
/// request created them between the command's check and its write.
#[derive(Default)]
struct RacingInsert {
    inner: InMemoryCustomerRepo,
    looked_up: Cell<bool>,
}

impl CustomerRepo for RacingInsert {
    async fn get(&self, id: CustomerId) -> Result<Option<Customer>, RepoError> {
        self.inner.get(id).await
    }
    async fn get_by_email(&self, email: &Email) -> Result<Option<Customer>, RepoError> {
        if !self.looked_up.replace(true) {
            return Ok(None);
        }
        self.inner.get_by_email(email).await
    }
    async fn insert(&self, customer: &mut Customer) -> Result<(), RepoError> {
        self.inner.insert(customer).await
    }
    async fn update(&self, customer: &mut Customer) -> Result<(), RepoError> {
        self.inner.update(customer).await
    }
}

#[test]
fn losing_the_race_for_an_email_reports_the_existing_customer() {
    let repo = RacingInsert::default();
    let other = CustomerId::from_uuid(Uuid::from_u128(77));
    block_on(
        repo.inner
            .insert(&mut Customer::new(other, email("ana@example.hr"))),
    )
    .unwrap();
    let input = CreateCustomer {
        email: "ana@example.hr".to_string(),
        first_name: None,
        last_name: None,
        phone: None,
    };
    assert_eq!(
        block_on(create_customer(
            &Ctx::system(),
            &repo,
            &SequentialIds::default(),
            input
        )),
        Err(CommandError::CustomerExists {
            email: email("ana@example.hr"),
            customer: other
        })
    );
}

// ---- find or create ----

#[test]
fn find_or_create_creates_a_guest_once() {
    let world = World::new();
    let ana = email("ana@example.hr");
    let created = block_on(find_or_create_customer(
        &Ctx::system(),
        &world.customers,
        &world.ids,
        &ana,
    ))
    .unwrap();
    let found = block_on(find_or_create_customer(
        &Ctx::system(),
        &world.customers,
        &world.ids,
        &ana,
    ))
    .unwrap();
    assert_eq!(found, created);
    assert_eq!(created.first_name(), None);
}

#[test]
fn find_or_create_finds_an_existing_customer() {
    let world = World::new();
    let existing = world.create("ana@example.hr").unwrap();
    assert_eq!(
        block_on(find_or_create_customer(
            &Ctx::system(),
            &world.customers,
            &world.ids,
            &email("ana@example.hr")
        )),
        Ok(existing)
    );
}

#[test]
fn find_or_create_losing_a_race_returns_the_winner() {
    let repo = RacingInsert::default();
    let other = CustomerId::from_uuid(Uuid::from_u128(77));
    let mut winner = Customer::new(other, email("ana@example.hr"));
    block_on(repo.inner.insert(&mut winner)).unwrap();
    assert_eq!(
        block_on(find_or_create_customer(
            &Ctx::system(),
            &repo,
            &SequentialIds::default(),
            &email("ana@example.hr")
        )),
        Ok(winner)
    );
}

// ---- read and update ----

#[test]
fn get_needs_only_read_scope() {
    let world = World::new();
    let c = world.create("ana@example.hr").unwrap();
    assert_eq!(
        block_on(get_customer(&reader(), &world.customers, c.id())),
        Ok(c)
    );
}

#[test]
fn get_unknown_customer_fails() {
    let unknown = CustomerId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(get_customer(&reader(), &World::new().customers, unknown)),
        Err(CommandError::CustomerNotFound(unknown))
    );
}

#[test]
fn update_changes_given_fields_and_empty_clears() {
    let world = World::new();
    let c = world.create("ana@example.hr").unwrap();
    let updated = world
        .update(
            c.id(),
            UpdateCustomer {
                first_name: Some(String::new()),
                last_name: Some("Horvat".to_string()),
                ..UpdateCustomer::default()
            },
        )
        .unwrap();
    assert_eq!(updated.first_name(), None);
    assert_eq!(updated.last_name(), Some("Horvat"));
    assert_eq!(updated.email(), &email("ana@example.hr"));
    assert_eq!(world.stored(c.id()), updated);
}

#[test]
fn update_can_change_the_email_to_a_free_one() {
    let world = World::new();
    let c = world.create("ana@example.hr").unwrap();
    let updated = world
        .update(
            c.id(),
            UpdateCustomer {
                email: Some("ana.horvat@example.hr".to_string()),
                ..UpdateCustomer::default()
            },
        )
        .unwrap();
    assert_eq!(updated.email(), &email("ana.horvat@example.hr"));
}

#[test]
fn update_keeping_the_same_email_is_not_a_conflict() {
    let world = World::new();
    let c = world.create("ana@example.hr").unwrap();
    assert!(
        world
            .update(
                c.id(),
                UpdateCustomer {
                    email: Some("ANA@example.hr".to_string()),
                    ..UpdateCustomer::default()
                },
            )
            .is_ok()
    );
}

#[test]
fn update_refuses_another_customers_email() {
    let world = World::new();
    let ana = world.create("ana@example.hr").unwrap();
    let ivo = world.create("ivo@example.hr").unwrap();
    assert_eq!(
        world.update(
            ivo.id(),
            UpdateCustomer {
                email: Some("ana@example.hr".to_string()),
                ..UpdateCustomer::default()
            },
        ),
        Err(CommandError::CustomerExists {
            email: email("ana@example.hr"),
            customer: ana.id()
        })
    );
}

// ---- addresses ----

#[test]
fn add_address_stores_it_and_its_defaults() {
    let world = World::new();
    let c = world.create("ana@example.hr").unwrap();
    let updated = world.add(c.id(), save("Zagreb", true, true)).unwrap();

    let saved = &updated.addresses()[0];
    assert_eq!(saved.address.parts().city, "Zagreb");
    assert_eq!(updated.default_shipping(), Some(saved.id));
    assert_eq!(updated.default_billing(), Some(saved.id));
    assert_eq!(world.stored(c.id()), updated);
}

#[test]
fn add_address_rejects_an_invalid_address() {
    let world = World::new();
    let c = world.create("ana@example.hr").unwrap();
    assert_eq!(
        world.add(c.id(), save(" ", false, false)),
        Err(CommandError::Address(AddressError::Missing("city")))
    );
}

#[test]
fn update_address_replaces_it() {
    let world = World::new();
    let c = world.create("ana@example.hr").unwrap();
    let id = world
        .add(c.id(), save("Zagreb", true, false))
        .unwrap()
        .addresses()[0]
        .id;

    let updated = block_on(update_address(
        &Ctx::system(),
        &world.customers,
        c.id(),
        id,
        save("Split", false, true),
    ))
    .unwrap();

    assert_eq!(updated.address(id).unwrap().address.parts().city, "Split");
    assert_eq!(updated.default_shipping(), None);
    assert_eq!(updated.default_billing(), Some(id));
}

#[test]
fn remove_address_removes_it_and_its_defaults() {
    let world = World::new();
    let c = world.create("ana@example.hr").unwrap();
    let id = world
        .add(c.id(), save("Zagreb", true, true))
        .unwrap()
        .addresses()[0]
        .id;

    let updated = block_on(remove_address(&Ctx::system(), &world.customers, c.id(), id)).unwrap();

    assert!(updated.addresses().is_empty());
    assert_eq!(updated.default_shipping(), None);
}

#[test]
fn address_commands_reject_unknown_addresses() {
    let world = World::new();
    let c = world.create("ana@example.hr").unwrap();
    let unknown = AddressId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(remove_address(
            &Ctx::system(),
            &world.customers,
            c.id(),
            unknown
        )),
        Err(CommandError::Customer(CustomerError::AddressNotFound(
            unknown
        )))
    );
}
