use hypha_core::{
    Actor, AuthError, Ctx, InventoryItemId, LocationId, MarketId, Scope, UserId, VariantId,
};
use hypha_inventory::{
    AdjustOnHand, CommandError, CreateInventoryItem, CreateLocation, InventoryError, InventoryItem,
    InventoryItemRepo, Location, LocationRepo, SetOnHand, UpdateInventoryItem, UpdateLocation,
    adjust_on_hand, create_inventory_item, create_location, get_inventory_item,
    get_variant_inventory, set_on_hand, update_inventory_item, update_location,
};
use hypha_testkit::{InMemoryInventoryItemRepo, InMemoryLocationRepo, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn market(n: u128) -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(1000 + n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(2000 + n))
}

fn reader() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::InventoryRead],
    )
}

struct World {
    items: InMemoryInventoryItemRepo,
    locations: InMemoryLocationRepo,
    ids: SequentialIds,
}

impl World {
    fn new() -> Self {
        Self {
            items: InMemoryInventoryItemRepo::default(),
            locations: InMemoryLocationRepo::default(),
            ids: SequentialIds::default(),
        }
    }

    fn create_location(
        &self,
        name: &str,
        markets: Vec<MarketId>,
    ) -> Result<Location, CommandError> {
        let input = CreateLocation {
            name: name.to_string(),
            markets,
            priority: 0,
        };
        block_on(create_location(
            &Ctx::system(),
            &self.locations,
            &self.ids,
            input,
        ))
    }

    fn location(&self) -> LocationId {
        self.create_location("Zagreb", vec![market(1)])
            .unwrap()
            .id()
    }

    fn create_item(&self, variant: VariantId) -> Result<InventoryItem, CommandError> {
        let input = CreateInventoryItem {
            variant,
            tracked: true,
            continue_selling: false,
        };
        block_on(create_inventory_item(
            &Ctx::system(),
            &self.items,
            &self.ids,
            input,
        ))
    }

    fn item(&self) -> InventoryItemId {
        self.create_item(vid(1)).unwrap().id()
    }

    fn set(
        &self,
        item: InventoryItemId,
        location: LocationId,
        quantity: i64,
        expected: Option<i64>,
    ) -> Result<InventoryItem, CommandError> {
        let input = SetOnHand {
            item,
            location,
            quantity,
            expected,
        };
        block_on(set_on_hand(
            &Ctx::system(),
            &self.items,
            &self.locations,
            input,
        ))
    }

    fn adjust(
        &self,
        item: InventoryItemId,
        location: LocationId,
        delta: i64,
    ) -> Result<InventoryItem, CommandError> {
        let input = AdjustOnHand {
            item,
            location,
            delta,
        };
        block_on(adjust_on_hand(
            &Ctx::system(),
            &self.items,
            &self.locations,
            input,
        ))
    }

    fn stored(&self, item: InventoryItemId) -> InventoryItem {
        block_on(self.items.get(item)).unwrap().unwrap()
    }
}

// ---- locations ----

#[test]
fn create_location_stores_it() {
    let world = World::new();
    let location = world
        .create_location("Zagreb", vec![market(1), market(2)])
        .unwrap();
    assert_eq!(location.markets(), &[market(1), market(2)]);
    assert_eq!(
        block_on(world.locations.get(location.id())),
        Ok(Some(location))
    );
}

#[test]
fn create_location_rejects_a_market_listed_twice() {
    assert_eq!(
        World::new().create_location("Zagreb", vec![market(1), market(1)]),
        Err(CommandError::Inventory(InventoryError::DuplicateMarket(
            market(1)
        )))
    );
}

#[test]
fn create_location_requires_write_scope() {
    let world = World::new();
    let input = CreateLocation {
        name: "Zagreb".to_string(),
        markets: vec![],
        priority: 0,
    };
    assert_eq!(
        block_on(create_location(
            &reader(),
            &world.locations,
            &world.ids,
            input
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::InventoryWrite
        )))
    );
}

#[test]
fn update_location_changes_only_what_is_given() {
    let world = World::new();
    let id = world.location();
    let input = UpdateLocation {
        priority: Some(7),
        markets: Some(vec![market(2)]),
        ..UpdateLocation::default()
    };
    let updated = block_on(update_location(&Ctx::system(), &world.locations, id, input)).unwrap();

    assert_eq!(updated.name(), "Zagreb");
    assert_eq!(updated.priority(), 7);
    assert_eq!(
        block_on(world.locations.serving(market(2))),
        Ok(vec![updated])
    );
}

#[test]
fn update_location_rejects_unknown_location() {
    let world = World::new();
    let unknown = LocationId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(update_location(
            &Ctx::system(),
            &world.locations,
            unknown,
            UpdateLocation::default()
        )),
        Err(CommandError::LocationNotFound(unknown))
    );
}

// ---- items ----

#[test]
fn create_item_stores_it_without_stock() {
    let world = World::new();
    let item = world.create_item(vid(1)).unwrap();
    assert!(item.tracked());
    assert!(item.levels().is_empty());
    assert_eq!(world.stored(item.id()), item);
}

#[test]
fn a_variant_has_at_most_one_item() {
    let world = World::new();
    let first = world.item();
    assert_eq!(
        world.create_item(vid(1)),
        Err(CommandError::InventoryItemExists {
            variant: vid(1),
            item: first
        })
    );
}

#[test]
fn losing_the_race_for_a_variant_reports_the_existing_item() {
    let repo = InMemoryInventoryItemRepo::default();
    let other = InventoryItemId::from_uuid(Uuid::from_u128(77));
    block_on(repo.insert(&mut InventoryItem::new(other, vid(1), true, false))).unwrap();

    let input = CreateInventoryItem {
        variant: vid(1),
        tracked: true,
        continue_selling: false,
    };
    // The pre-check misses it, as if it was inserted right after.
    repo.hide_next_lookups(1);
    assert_eq!(
        block_on(create_inventory_item(
            &Ctx::system(),
            &repo,
            &SequentialIds::default(),
            input
        )),
        Err(CommandError::InventoryItemExists {
            variant: vid(1),
            item: other
        })
    );
}

#[test]
fn get_item_needs_only_read_scope() {
    let world = World::new();
    let id = world.item();
    assert_eq!(
        block_on(get_inventory_item(&reader(), &world.items, id)).map(|i| i.id()),
        Ok(id)
    );
}

#[test]
fn get_unknown_item_fails() {
    let world = World::new();
    let unknown = InventoryItemId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(get_inventory_item(&reader(), &world.items, unknown)),
        Err(CommandError::InventoryItemNotFound(unknown))
    );
}

#[test]
fn a_variant_without_an_item_has_no_inventory() {
    let world = World::new();
    world.item();
    assert_eq!(
        block_on(get_variant_inventory(&reader(), &world.items, vid(2))),
        Ok(None)
    );
    assert!(
        block_on(get_variant_inventory(&reader(), &world.items, vid(1)))
            .unwrap()
            .is_some()
    );
}

#[test]
fn update_item_turns_tracking_off() {
    let world = World::new();
    let id = world.item();
    let input = UpdateInventoryItem {
        tracked: Some(false),
        ..UpdateInventoryItem::default()
    };
    let updated = block_on(update_inventory_item(
        &Ctx::system(),
        &world.items,
        id,
        input,
    ))
    .unwrap();
    assert!(!updated.tracked());
    assert!(!updated.continue_selling());
    assert_eq!(world.stored(id), updated);
}

// ---- stock counts ----

#[test]
fn set_on_hand_stores_the_count() {
    let world = World::new();
    let (item, location) = (world.item(), world.location());
    world.set(item, location, 5, None).unwrap();
    assert_eq!(world.stored(item).on_hand(location), 5);
}

#[test]
fn a_count_records_what_is_available_now() {
    let world = World::new();
    let (item, location) = (world.item(), world.location());
    world.set(item, location, 5, None).unwrap();
    let events = world.items.outbox().events();
    let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
    // Created, then counted.
    assert_eq!(types, ["inventory_item.changed", "inventory_item.changed"]);
    assert_eq!(
        events[1].data["available"],
        serde_json::json!([{ "location": location.to_string(), "available": 5 }])
    );
    // Counting the same again changes nothing, and records nothing.
    world.set(item, location, 5, None).unwrap();
    assert_eq!(world.items.outbox().events().len(), 2);
}

#[test]
fn set_on_hand_refuses_a_moved_count() {
    let world = World::new();
    let (item, location) = (world.item(), world.location());
    world.set(item, location, 5, None).unwrap();
    world.adjust(item, location, -1).unwrap();

    assert_eq!(
        world.set(item, location, 10, Some(5)),
        Err(CommandError::Inventory(InventoryError::OnHandChanged {
            expected: 5,
            actual: 4
        }))
    );
    assert_eq!(world.stored(item).on_hand(location), 4);
}

#[test]
fn set_on_hand_rejects_unknown_location() {
    let world = World::new();
    let item = world.item();
    let unknown = LocationId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        world.set(item, unknown, 5, None),
        Err(CommandError::LocationNotFound(unknown))
    );
}

#[test]
fn set_on_hand_rejects_unknown_item() {
    let world = World::new();
    let location = world.location();
    let unknown = InventoryItemId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        world.set(unknown, location, 5, None),
        Err(CommandError::InventoryItemNotFound(unknown))
    );
}

#[test]
fn adjust_on_hand_adds_to_the_count() {
    let world = World::new();
    let (item, location) = (world.item(), world.location());
    world.adjust(item, location, 3).unwrap();
    world.adjust(item, location, 4).unwrap();
    assert_eq!(world.stored(item).on_hand(location), 7);
}

#[test]
fn stock_writes_require_write_scope() {
    let world = World::new();
    let (item, location) = (world.item(), world.location());
    let input = AdjustOnHand {
        item,
        location,
        delta: 1,
    };
    assert_eq!(
        block_on(adjust_on_hand(
            &reader(),
            &world.items,
            &world.locations,
            input
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::InventoryWrite
        )))
    );
}
