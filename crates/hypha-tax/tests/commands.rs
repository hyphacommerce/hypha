use std::collections::HashMap;

use hypha_core::{
    Actor, AuthError, CountryCode, CountryError, Ctx, MoneyError, ProductId, Scope, TaxClassId,
    TaxRate, TaxRegionId, UserId,
};
use hypha_tax::{
    AssignTaxClass, ClassRate, CommandError, CreateTaxClass, CreateTaxRegion, TaxClassRepo,
    TaxError, TaxRegion, TaxRegionRepo, assign_tax_class, create_tax_class, create_tax_region,
    get_tax_region, list_tax_classes, list_tax_regions, remove_class_rate, set_class_rate,
    set_default_rate,
};
use hypha_testkit::{InMemoryPorts, InMemoryTaxRegionRepo, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn rate(ppm: u32) -> TaxRate {
    TaxRate::new(ppm).unwrap()
}

fn reader() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::TaxesRead],
    )
}

struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
}

impl World {
    fn new() -> Self {
        Self {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
        }
    }

    fn class(&self, name: &str) -> TaxClassId {
        let input = CreateTaxClass {
            name: name.to_string(),
        };
        block_on(create_tax_class(
            &Ctx::system(),
            &self.ports.tax_classes,
            &self.ids,
            input,
        ))
        .unwrap()
        .id()
    }

    fn create_region(&self, country: &str, default_rate: u32) -> Result<TaxRegion, CommandError> {
        let input = CreateTaxRegion {
            country: country.to_string(),
            default_rate,
        };
        block_on(create_tax_region(
            &Ctx::system(),
            &self.ports.tax_regions,
            &self.ids,
            input,
        ))
    }

    fn region(&self, country: &str) -> TaxRegionId {
        self.create_region(country, 250_000).unwrap().id()
    }

    fn set_class_rate(
        &self,
        region: TaxRegionId,
        class: TaxClassId,
        rate: u32,
    ) -> Result<TaxRegion, CommandError> {
        block_on(set_class_rate(
            &Ctx::system(),
            &self.ports.tax_regions,
            &self.ports.tax_classes,
            region,
            class,
            rate,
        ))
    }

    fn stored(&self, region: TaxRegionId) -> TaxRegion {
        block_on(self.ports.tax_regions.get(region))
            .unwrap()
            .unwrap()
    }
}

// ---------- classes ----------

#[test]
fn create_tax_class_stores_it() {
    let world = World::new();
    let id = world.class("Books");
    assert_eq!(
        block_on(world.ports.tax_classes.get(id))
            .unwrap()
            .map(|c| c.name().to_string()),
        Some("Books".to_string())
    );
}

#[test]
fn create_tax_class_requires_taxes_write() {
    let world = World::new();
    let input = CreateTaxClass {
        name: "Books".to_string(),
    };
    assert_eq!(
        block_on(create_tax_class(
            &reader(),
            &world.ports.tax_classes,
            &world.ids,
            input
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::TaxesWrite
        )))
    );
}

#[test]
fn assign_tax_class_assigns_and_unassigns() {
    let world = World::new();
    let books = world.class("Books");
    let ctx = Ctx::system();

    block_on(assign_tax_class(
        &ctx,
        &world.ports,
        AssignTaxClass {
            products: vec![pid(1), pid(2)],
            class: Some(books),
        },
    ))
    .unwrap();
    block_on(assign_tax_class(
        &ctx,
        &world.ports,
        AssignTaxClass {
            products: vec![pid(2)],
            class: None,
        },
    ))
    .unwrap();

    assert_eq!(
        block_on(world.ports.tax_classes.classes_for(&[pid(1), pid(2)])),
        Ok(HashMap::from([(pid(1), books)]))
    );
    let events = world.ports.outbox.events();
    let set: Vec<(&serde_json::Value, &serde_json::Value)> = events
        .iter()
        .filter(|e| e.event_type == "product.tax_class_set")
        .map(|e| (&e.data["product"], &e.data["tax_class"]))
        .collect();
    let (one, two, books) = (
        serde_json::json!(pid(1)),
        serde_json::json!(pid(2)),
        serde_json::json!(books),
    );
    let none = serde_json::Value::Null;
    assert_eq!(set, [(&one, &books), (&two, &books), (&two, &none)]);
}

#[test]
fn assign_tax_class_rejects_unknown_class() {
    let world = World::new();
    let unknown = TaxClassId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(assign_tax_class(
            &Ctx::system(),
            &world.ports,
            AssignTaxClass {
                products: vec![pid(1)],
                class: Some(unknown),
            },
        )),
        Err(CommandError::TaxClassNotFound(unknown))
    );
    assert_eq!(
        block_on(world.ports.tax_classes.classes_for(&[pid(1)])),
        Ok(HashMap::new())
    );
}

// ---------- regions ----------

#[test]
fn create_tax_region_stores_it() {
    let world = World::new();
    let region = world.create_region("HR", 250_000).unwrap();
    assert_eq!(region.country(), CountryCode::new("HR").unwrap());
    assert_eq!(region.default_rate(), rate(250_000));
    assert_eq!(world.stored(region.id()), region);
}

#[test]
fn create_tax_region_rejects_invalid_input() {
    let world = World::new();
    assert_eq!(
        world.create_region("hr", 250_000),
        Err(CommandError::Country(CountryError::NotUppercase))
    );
    assert_eq!(
        world.create_region("HR", 1_000_001),
        Err(CommandError::Money(MoneyError::InvalidTaxRate(1_000_001)))
    );
}

#[test]
fn create_tax_region_rejects_second_region_for_country() {
    let world = World::new();
    let first = world.region("HR");
    assert_eq!(
        world.create_region("HR", 130_000),
        Err(CommandError::TaxRegionExists {
            country: CountryCode::new("HR").unwrap(),
            region: first,
        })
    );
}

#[test]
fn get_tax_region_needs_only_taxes_read() {
    let world = World::new();
    let id = world.region("HR");
    assert_eq!(
        block_on(get_tax_region(&reader(), &world.ports.tax_regions, id)),
        Ok(world.stored(id))
    );
}

#[test]
fn get_unknown_tax_region_is_not_found() {
    let world = World::new();
    let unknown = TaxRegionId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(get_tax_region(
            &Ctx::system(),
            &world.ports.tax_regions,
            unknown
        )),
        Err(CommandError::TaxRegionNotFound(unknown))
    );
}

#[test]
fn set_default_rate_stores_it() {
    let world = World::new();
    let id = world.region("HR");
    block_on(set_default_rate(
        &Ctx::system(),
        &world.ports.tax_regions,
        id,
        130_000,
    ))
    .unwrap();
    assert_eq!(world.stored(id).default_rate(), rate(130_000));
}

#[test]
fn set_class_rate_stores_it() {
    let world = World::new();
    let region = world.region("HR");
    let books = world.class("Books");

    world.set_class_rate(region, books, 50_000).unwrap();

    assert_eq!(
        world.stored(region).class_rates(),
        &[ClassRate {
            class: books,
            rate: rate(50_000),
        }]
    );
}

#[test]
fn set_class_rate_rejects_unknown_class_and_stores_nothing() {
    let world = World::new();
    let region = world.region("HR");
    let unknown = TaxClassId::from_uuid(Uuid::from_u128(99));

    assert_eq!(
        world.set_class_rate(region, unknown, 50_000),
        Err(CommandError::TaxClassNotFound(unknown))
    );
    assert!(world.stored(region).class_rates().is_empty());
}

#[test]
fn remove_class_rate_removes_it_once() {
    let world = World::new();
    let region = world.region("HR");
    let books = world.class("Books");
    world.set_class_rate(region, books, 50_000).unwrap();
    let ctx = Ctx::system();

    block_on(remove_class_rate(
        &ctx,
        &world.ports.tax_regions,
        region,
        books,
    ))
    .unwrap();

    assert!(world.stored(region).class_rates().is_empty());
    assert_eq!(
        block_on(remove_class_rate(
            &ctx,
            &world.ports.tax_regions,
            region,
            books
        )),
        Err(CommandError::Tax(TaxError::ClassRateNotFound(books)))
    );
}

// ---------- races: pre-check passes, the repo's unique key catches it ----------

#[test]
fn region_taken_by_a_concurrent_insert_names_the_owner() {
    let regions = InMemoryTaxRegionRepo::default();
    let ids = SequentialIds::default();
    let mut first = TaxRegion::new(
        TaxRegionId::from_uuid(Uuid::from_u128(500)),
        CountryCode::new("HR").unwrap(),
        rate(250_000),
    );
    block_on(regions.insert(&mut first)).unwrap();

    let input = CreateTaxRegion {
        country: "HR".to_string(),
        default_rate: 130_000,
    };
    // The pre-check misses it, as if it was inserted right after.
    regions.hide_next_lookups(1);
    assert_eq!(
        block_on(create_tax_region(&Ctx::system(), &regions, &ids, input)),
        Err(CommandError::TaxRegionExists {
            country: CountryCode::new("HR").unwrap(),
            region: first.id(),
        })
    );
}

#[test]
fn classes_and_regions_list_oldest_first_for_taxes_read() {
    let world = World::new();
    let standard = world.class("Standard");
    let reduced = world.class("Reduced");
    let hr = world.region("HR");
    let de = world.region("DE");

    let classes = block_on(list_tax_classes(&reader(), &world.ports.tax_classes)).unwrap();
    assert_eq!(
        classes.iter().map(|c| c.id()).collect::<Vec<_>>(),
        vec![standard, reduced]
    );
    let regions = block_on(list_tax_regions(&reader(), &world.ports.tax_regions)).unwrap();
    assert_eq!(
        regions.iter().map(|r| r.id()).collect::<Vec<_>>(),
        vec![hr, de]
    );

    let nobody = Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), vec![]);
    assert_eq!(
        block_on(list_tax_regions(&nobody, &world.ports.tax_regions)),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::TaxesRead
        )))
    );
}
