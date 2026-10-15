//! `reserve_stock` and `release_stock`: all or nothing, idempotent, and safe under
//! concurrent checkouts (design note 0004, section 3).

use std::cell::Cell;

use hypha_core::{
    Actor, AuthError, Ctx, InventoryItemId, LocationId, MAX_ATTEMPTS, MarketId, Quantity,
    QuantityError, RepoError, Scope, UserId, VariantId, Versioned,
};
use hypha_inventory::{
    CommandError, InventoryError, InventoryItem, InventoryItemRepo, Location, LocationRepo,
    ReleaseLine, ReleaseStock, ReservationRef, ReserveLine, ReserveStock, Shortage, release_stock,
    reserve_stock,
};
use hypha_testkit::{InMemoryInventoryItemRepo, InMemoryLocationRepo};
use pollster::block_on;
use uuid::Uuid;

fn market() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(1))
}

fn other_market() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(2))
}

fn loc(n: u128) -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(100 + n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(200 + n))
}

fn item_id(n: u128) -> InventoryItemId {
    InventoryItemId::from_uuid(Uuid::from_u128(300 + n))
}

fn reference(s: &str) -> ReservationRef {
    ReservationRef::new(s).unwrap()
}

fn line(reference: &str, variant: u128, quantity: u32) -> ReserveLine {
    ReserveLine {
        reference: reference.to_string(),
        variant: vid(variant),
        quantity,
    }
}

/// Wraps the item repo to play a rival checkout: before each of the next `races`
/// `update_many` calls, the rival reserves `rival_quantity` of the first item being
/// written (or just saves it again, if 0), so the command's write is stale.
#[derive(Default)]
struct Racing {
    inner: InMemoryInventoryItemRepo,
    races: Cell<u32>,
    rival_quantity: u32,
    serving: Vec<Location>,
    writes: Cell<u32>,
}

impl InventoryItemRepo for Racing {
    async fn get(&self, id: InventoryItemId) -> Result<Option<InventoryItem>, RepoError> {
        self.inner.get(id).await
    }
    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<InventoryItem>, RepoError> {
        self.inner.get_by_variant(variant).await
    }
    async fn for_variants(&self, variants: &[VariantId]) -> Result<Vec<InventoryItem>, RepoError> {
        self.inner.for_variants(variants).await
    }
    async fn insert(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        self.inner.insert(item).await
    }
    async fn update(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        self.inner.update(item).await
    }
    async fn delete(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        self.inner.delete(item).await
    }
    async fn update_many(&self, items: &mut [InventoryItem]) -> Result<(), RepoError> {
        self.writes.set(self.writes.get() + 1);
        if self.races.get() > 0 {
            self.races.set(self.races.get() - 1);
            let mut rival = self.inner.get(items[0].id()).await?.unwrap();
            if self.rival_quantity > 0 {
                let r = reference(&format!("rival-{}", self.writes.get()));
                rival
                    .reserve(
                        &r,
                        Quantity::new(self.rival_quantity).unwrap(),
                        &self.serving,
                    )
                    .unwrap();
            }
            self.inner.update(&mut rival).await?;
        }
        self.inner.update_many(items).await
    }
}

struct World {
    items: Racing,
    locations: InMemoryLocationRepo,
}

impl World {
    /// Two locations serve the market: Zagreb (priority 0) and Split (priority 1).
    /// Rijeka holds stock but sells to another market.
    fn new() -> Self {
        Self::racing(0, 0)
    }

    fn racing(races: u32, rival_quantity: u32) -> Self {
        let locations = InMemoryLocationRepo::default();
        let mut serving = Vec::new();
        for (n, name, m, priority) in [
            (1, "Zagreb", market(), 0),
            (2, "Split", market(), 1),
            (3, "Rijeka", other_market(), 0),
        ] {
            let mut l = Location::new(loc(n), name.to_string(), vec![m], priority).unwrap();
            block_on(locations.insert(&mut l)).unwrap();
            if m == market() {
                serving.push(l);
            }
        }
        Self {
            items: Racing {
                races: Cell::new(races),
                rival_quantity,
                serving,
                ..Racing::default()
            },
            locations,
        }
    }

    /// A tracked item for variant `n` with `stock` as (location, on hand).
    fn stock(&self, n: u128, stock: &[(u128, i64)]) {
        let mut item = InventoryItem::new(item_id(n), vid(n), true, false);
        for (location, on_hand) in stock {
            item.set_on_hand(loc(*location), *on_hand, None).unwrap();
        }
        block_on(self.items.inner.insert(&mut item)).unwrap();
    }

    fn reserve(&self, lines: Vec<ReserveLine>) -> Result<(), CommandError> {
        self.reserve_as(&Ctx::system(), lines)
    }

    fn reserve_as(&self, ctx: &Ctx, lines: Vec<ReserveLine>) -> Result<(), CommandError> {
        let input = ReserveStock {
            market: market(),
            lines,
        };
        block_on(reserve_stock(ctx, &self.items, &self.locations, input))
    }

    fn release(&self, lines: &[(&str, u128)]) -> Result<(), CommandError> {
        let input = ReleaseStock {
            lines: lines
                .iter()
                .map(|(r, v)| ReleaseLine {
                    reference: r.to_string(),
                    variant: vid(*v),
                })
                .collect(),
        };
        block_on(release_stock(&Ctx::system(), &self.items, input))
    }

    fn stored(&self, n: u128) -> InventoryItem {
        block_on(self.items.inner.get(item_id(n))).unwrap().unwrap()
    }

    /// (reference, location, quantity) of every reservation on item `n`.
    fn held(&self, n: u128) -> Vec<(String, LocationId, u32)> {
        self.stored(n)
            .reservations()
            .iter()
            .map(|r| (r.reference.to_string(), r.location, r.quantity.value()))
            .collect()
    }
}

// ---- reserving ----

#[test]
fn reserves_every_line_from_the_market_locations() {
    let world = World::new();
    world.stock(1, &[(1, 5)]);
    world.stock(2, &[(1, 1), (2, 2)]);

    world
        .reserve(vec![line("ol-1", 1, 2), line("ol-2", 2, 3)])
        .unwrap();

    assert_eq!(world.held(1), vec![("ol-1".to_string(), loc(1), 2)]);
    assert_eq!(
        world.held(2),
        vec![
            ("ol-2".to_string(), loc(1), 1),
            ("ol-2".to_string(), loc(2), 2)
        ]
    );
    assert_eq!(world.items.writes.get(), 1, "one write for the whole order");
}

#[test]
fn stock_in_another_market_does_not_count() {
    let world = World::new();
    world.stock(1, &[(1, 1), (3, 50)]);
    assert_eq!(
        world.reserve(vec![line("ol-1", 1, 2)]),
        Err(CommandError::OutOfStock {
            lines: vec![Shortage {
                variant: vid(1),
                requested: 2,
                available: 1
            }]
        })
    );
}

#[test]
fn one_short_line_reserves_nothing_and_every_short_line_is_reported() {
    let world = World::new();
    world.stock(1, &[(1, 5)]);
    world.stock(2, &[(1, 1)]);
    world.stock(3, &[]);

    assert_eq!(
        world.reserve(vec![
            line("ol-1", 1, 2),
            line("ol-2", 2, 3),
            line("ol-3", 3, 1)
        ]),
        Err(CommandError::OutOfStock {
            lines: vec![
                Shortage {
                    variant: vid(2),
                    requested: 3,
                    available: 1
                },
                Shortage {
                    variant: vid(3),
                    requested: 1,
                    available: 0
                }
            ]
        })
    );
    assert!(
        world.held(1).is_empty(),
        "the line with stock wasn't reserved either"
    );
    assert_eq!(world.items.writes.get(), 0);
}

#[test]
fn two_lines_of_one_variant_share_its_stock() {
    let world = World::new();
    world.stock(1, &[(1, 3)]);
    assert_eq!(
        world.reserve(vec![line("ol-1", 1, 2), line("ol-2", 1, 2)]),
        Err(CommandError::OutOfStock {
            lines: vec![Shortage {
                variant: vid(1),
                requested: 2,
                available: 1
            }]
        })
    );
}

#[test]
fn variants_without_tracked_stock_need_nothing() {
    let world = World::new();
    world.stock(1, &[]);
    let mut untracked = world.stored(1);
    untracked.set_tracked(false);
    block_on(world.items.inner.update(&mut untracked)).unwrap();

    // Variant 1 is untracked, variant 9 has no item at all.
    world
        .reserve(vec![line("ol-1", 1, 100), line("ol-2", 9, 100)])
        .unwrap();
    assert!(world.held(1).is_empty());
    assert_eq!(world.items.writes.get(), 0, "nothing to write");
}

#[test]
fn reserving_again_is_a_no_op() {
    let world = World::new();
    world.stock(1, &[(1, 2)]);
    world.reserve(vec![line("ol-1", 1, 2)]).unwrap();
    let version = world.stored(1).version();

    // A retried checkout: same reference, same quantity.
    assert_eq!(world.reserve(vec![line("ol-1", 1, 2)]), Ok(()));
    assert_eq!(world.stored(1).version(), version);
    assert_eq!(world.held(1).len(), 1);
}

#[test]
fn a_reference_cannot_be_reused_for_another_quantity() {
    let world = World::new();
    world.stock(1, &[(1, 5)]);
    world.reserve(vec![line("ol-1", 1, 2)]).unwrap();
    assert_eq!(
        world.reserve(vec![line("ol-1", 1, 3)]),
        Err(CommandError::Inventory(InventoryError::ReferenceInUse(
            reference("ol-1")
        )))
    );
}

#[test]
fn input_is_checked_before_anything_is_read() {
    let world = World::new();
    assert_eq!(
        world.reserve(vec![line("ol-1", 1, 0)]),
        Err(CommandError::Inventory(InventoryError::Quantity(
            QuantityError::Zero
        )))
    );
    assert_eq!(
        world.reserve(vec![line("", 1, 1)]),
        Err(CommandError::Inventory(InventoryError::InvalidReference))
    );
}

#[test]
fn reserving_requires_write_scope() {
    let world = World::new();
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::InventoryRead],
    );
    assert_eq!(
        world.reserve_as(&reader, vec![line("ol-1", 1, 1)]),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::InventoryWrite
        )))
    );
}

// ---- concurrent checkouts ----

#[test]
fn a_lost_race_is_retried_on_fresh_stock() {
    // A rival takes 1 of 5 between our read and our write.
    let world = World::racing(1, 1);
    world.stock(1, &[(1, 5)]);

    world.reserve(vec![line("ol-1", 1, 2)]).unwrap();

    assert_eq!(world.items.writes.get(), 2, "first write lost, second won");
    assert_eq!(
        world.stored(1).available(loc(1)),
        2,
        "5 - rival's 1 - our 2"
    );
}

#[test]
fn a_rival_taking_the_last_units_is_a_shortage_not_a_conflict() {
    // 2 left; the rival takes 1 first, so our 2 no longer fit.
    let world = World::racing(1, 1);
    world.stock(1, &[(1, 2)]);

    assert_eq!(
        world.reserve(vec![line("ol-1", 1, 2)]),
        Err(CommandError::OutOfStock {
            lines: vec![Shortage {
                variant: vid(1),
                requested: 2,
                available: 1
            }]
        })
    );
    assert_eq!(world.held(1).len(), 1, "only the rival's reservation");
}

#[test]
fn constant_conflicts_give_up_after_max_attempts() {
    let world = World::racing(u32::MAX, 0);
    world.stock(1, &[(1, 5)]);

    assert_eq!(
        world.reserve(vec![line("ol-1", 1, 1)]),
        Err(CommandError::Repo(RepoError::StaleVersion))
    );
    assert_eq!(world.items.writes.get(), MAX_ATTEMPTS);
    assert!(world.held(1).is_empty());
}

// ---- releasing ----

#[test]
fn release_gives_the_stock_back() {
    let world = World::new();
    world.stock(1, &[(1, 1), (2, 1)]);
    world.stock(2, &[(1, 3)]);
    world
        .reserve(vec![line("ol-1", 1, 2), line("ol-2", 2, 1)])
        .unwrap();

    world.release(&[("ol-1", 1)]).unwrap();

    assert!(world.held(1).is_empty());
    assert_eq!(world.held(2).len(), 1, "other references keep theirs");
}

#[test]
fn releasing_twice_or_unknown_lines_is_harmless() {
    let world = World::new();
    world.stock(1, &[(1, 1)]);
    world.reserve(vec![line("ol-1", 1, 1)]).unwrap();
    world.release(&[("ol-1", 1)]).unwrap();
    let version = world.stored(1).version();

    assert_eq!(world.release(&[("ol-1", 1), ("ol-9", 9)]), Ok(()));
    assert_eq!(world.stored(1).version(), version, "nothing written");
}

#[test]
fn release_retries_a_lost_race() {
    let world = World::racing(1, 0);
    world.stock(1, &[(1, 1)]);
    world.items.races.set(0);
    world.reserve(vec![line("ol-1", 1, 1)]).unwrap();
    world.items.races.set(1);

    world.release(&[("ol-1", 1)]).unwrap();

    assert!(world.held(1).is_empty());
}

/// B5: the same (reference, variant) twice in one request used to reserve once and
/// return `Ok` (the second line hit the idempotency path).
#[test]
fn the_same_line_twice_in_one_request_is_rejected() {
    let world = World::new();
    world.stock(1, &[(1, 5)]);
    let duplicate = Err(CommandError::Inventory(InventoryError::DuplicateLine {
        reference: reference("ol-1"),
        variant: vid(1),
    }));

    assert_eq!(
        world.reserve(vec![line("ol-1", 1, 2), line("ol-1", 1, 2)]),
        duplicate
    );
    assert!(world.held(1).is_empty(), "nothing reserved");
    assert_eq!(world.release(&[("ol-1", 1), ("ol-1", 1)]), duplicate);
}

#[test]
fn one_reference_for_two_variants_is_fine() {
    let world = World::new();
    world.stock(1, &[(1, 5)]);
    world.stock(2, &[(1, 5)]);
    assert_eq!(
        world.reserve(vec![line("ol-1", 1, 2), line("ol-1", 2, 1)]),
        Ok(())
    );
}
