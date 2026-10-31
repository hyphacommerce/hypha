#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]

use hypha_core::{
    Actor, AddressError, AddressId, AddressParts, AuthError, Ctx, CustomerId, Email, EmailError,
    ErrorCode, Scope, UserId,
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
        country: "HR".to_string(),
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

#[test]
fn losing_the_race_for_an_email_reports_the_existing_customer() {
    let repo = InMemoryCustomerRepo::default();
    let other = CustomerId::from_uuid(Uuid::from_u128(77));
    block_on(repo.insert(&mut Customer::new(other, email("ana@example.hr")))).unwrap();
    let input = CreateCustomer {
        email: "ana@example.hr".to_string(),
        first_name: None,
        last_name: None,
        phone: None,
    };
    // The pre-check misses it, as if it was inserted right after.
    repo.hide_next_lookups(1);
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
    let repo = InMemoryCustomerRepo::default();
    let other = CustomerId::from_uuid(Uuid::from_u128(77));
    let mut winner = Customer::new(other, email("ana@example.hr"));
    block_on(repo.insert(&mut winner)).unwrap();
    // The pre-check misses it, as if it was inserted right after.
    repo.hide_next_lookups(1);
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
fn update_changes_given_fields_and_none_or_blank_clears() {
    let world = World::new();
    let c = world.create("ana@example.hr").unwrap();
    world
        .update(
            c.id(),
            UpdateCustomer {
                first_name: Some(Some("Ana".to_string())),
                phone: Some(Some("+385 1 234".to_string())),
                ..UpdateCustomer::default()
            },
        )
        .unwrap();
    let updated = world
        .update(
            c.id(),
            UpdateCustomer {
                first_name: Some(None),
                last_name: Some(Some("Horvat".to_string())),
                phone: Some(Some(" ".to_string())),
                ..UpdateCustomer::default()
            },
        )
        .unwrap();
    assert_eq!(updated.first_name(), None);
    assert_eq!(updated.last_name(), Some("Horvat"));
    assert_eq!(updated.phone(), None);
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
    let mut lowercase = save("Zagreb", false, false);
    lowercase.address.country = "hr".to_string();
    assert_eq!(
        world.add(c.id(), lowercase).map_err(|e| e.code()),
        Err("invalid_country_code")
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

mod consent {
    use hypha_core::{AppId, Clock, Timestamp};
    use hypha_customers::{
        Channel, ConsentChange, ConsentSource, ConsentState, OptInLevel, set_marketing_consent,
    };
    use hypha_testkit::FixedClock;

    use super::*;

    fn admin() -> Ctx {
        Ctx::new(
            Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
            vec![Scope::CustomersWrite],
        )
    }

    fn subscribe(at: Option<Timestamp>) -> ConsentChange {
        ConsentChange {
            state: ConsentState::Subscribed,
            opt_in_level: OptInLevel::Single,
            consented_at: at,
        }
    }

    #[test]
    fn the_source_is_the_caller_and_the_newest_consent_wins() {
        let repo = InMemoryCustomerRepo::default();
        let clock = FixedClock::default();
        let ids = SequentialIds::default();
        let ana = block_on(create_customer(
            &admin(),
            &repo,
            &ids,
            CreateCustomer {
                email: "ana@example.hr".to_string(),
                first_name: None,
                last_name: None,
                phone: None,
            },
        ))
        .unwrap();

        // An app writes a confirmed subscription dated an hour ago.
        let app = AppId::from_uuid(Uuid::from_u128(77));
        let app_ctx = Ctx::new(Actor::App(app), vec![Scope::CustomersWrite]);
        let earlier = clock.now().minus(std::time::Duration::from_secs(3600));
        let (c, applied) = block_on(set_marketing_consent(
            &app_ctx,
            &repo,
            &clock,
            ana.id(),
            Channel::Email,
            ConsentChange {
                opt_in_level: OptInLevel::Confirmed,
                ..subscribe(Some(earlier))
            },
        ))
        .unwrap();
        assert!(applied);
        assert_eq!(
            c.marketing_consent(Channel::Email).source,
            Some(ConsentSource::App(app))
        );

        // Staff unsubscribe now: newer, so it wins; the repeat changes nothing.
        let unsubscribe = ConsentChange {
            state: ConsentState::Unsubscribed,
            opt_in_level: OptInLevel::Unknown,
            consented_at: None,
        };
        let (c, applied) = block_on(set_marketing_consent(
            &admin(),
            &repo,
            &clock,
            ana.id(),
            Channel::Email,
            unsubscribe,
        ))
        .unwrap();
        assert!(applied);
        assert_eq!(
            c.marketing_consent(Channel::Email).source,
            Some(ConsentSource::Admin)
        );
        let (_, applied) = block_on(set_marketing_consent(
            &admin(),
            &repo,
            &clock,
            ana.id(),
            Channel::Email,
            unsubscribe,
        ))
        .unwrap();
        assert!(!applied);

        // The app replays its old subscribe: too old.
        let (c, applied) = block_on(set_marketing_consent(
            &app_ctx,
            &repo,
            &clock,
            ana.id(),
            Channel::Email,
            subscribe(Some(earlier)),
        ))
        .unwrap();
        assert!(!applied);
        assert_eq!(
            c.marketing_consent(Channel::Email).state,
            ConsentState::Unsubscribed
        );
        let stored = block_on(repo.get(ana.id())).unwrap().unwrap();
        assert_eq!(
            stored.marketing_consent(Channel::Email).state,
            ConsentState::Unsubscribed
        );

        let reader = Ctx::new(
            Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
            vec![Scope::CustomersRead],
        );
        assert_eq!(
            block_on(set_marketing_consent(
                &reader,
                &repo,
                &clock,
                ana.id(),
                Channel::Email,
                subscribe(None),
            )),
            Err(CommandError::Auth(AuthError::MissingScope(
                Scope::CustomersWrite
            )))
        );
    }
}
