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
    Actor, AuthError, Ctx, HasOutbox, IdempotencyRequest, InventoryItemId, LocationId, OutboxRepo,
    PageLimit, PageRequest, Scope, Timestamp, TransferId, UserId, VariantId,
};
use hypha_inventory::{
    CommandError, CreateTransfer, HasStockMovements, InTransit, InventoryError, InventoryItem,
    InventoryItemRepo, Location, LocationRepo, MovementFilter, StockMovementRepo, StockReason,
    StockSource, TransferArrival, TransferFilter, TransferLineDraft, TransferRepo, TransferStatus,
    UpdateTransfer, cancel_transfer, create_transfer, delete_transfer, get_transfer,
    list_transfers, receive_transfer, ship_transfer, update_transfer,
};
use hypha_testkit::{FixedClock, InMemoryPorts, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(2000 + n))
}

fn at(secs: i64) -> Timestamp {
    Timestamp::from_unix_micros(secs * 1_000_000)
}

fn staff(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), scopes)
}

fn writer() -> Ctx {
    staff(vec![Scope::InventoryWrite])
}

fn key(k: &str) -> IdempotencyRequest {
    IdempotencyRequest {
        key: k.to_string(),
        fingerprint: [7; 32],
    }
}

/// Two locations, a warehouse holding 10 of variant 1 and 4 of variant 2, and a shop.
struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    clock: FixedClock,
    warehouse: LocationId,
    shop: LocationId,
    items: Vec<InventoryItemId>,
}

impl World {
    fn new() -> Self {
        let ports = InMemoryPorts::default();
        let ids = SequentialIds::default();
        let location = |n: u128, name: &str| {
            let mut l = Location::new(
                LocationId::from_uuid(Uuid::from_u128(n)),
                name.to_string(),
                Vec::new(),
                0,
            )
            .unwrap();
            block_on(ports.locations.insert(&mut l)).unwrap();
            l.id()
        };
        let warehouse = location(1, "Warehouse");
        let shop = location(2, "Shop");
        let mut items = Vec::new();
        for (n, on_hand) in [(1, 10), (2, 4)] {
            let mut item = InventoryItem::new(
                InventoryItemId::from_uuid(Uuid::from_u128(500 + n)),
                vid(n),
                true,
                false,
            );
            item.set_on_hand(warehouse, on_hand, None, None).unwrap();
            block_on(ports.inventory_items.insert(&mut item)).unwrap();
            items.push(item.id());
        }
        Self {
            ports,
            ids,
            clock: FixedClock::at(at(100)),
            warehouse,
            shop,
            items,
        }
    }

    fn create(&self, lines: &[(u128, u32)]) -> Result<hypha_inventory::Transfer, CommandError> {
        block_on(create_transfer(
            &writer(),
            &self.ports,
            &self.ids,
            CreateTransfer {
                origin: self.warehouse,
                destination: self.shop,
                expected_at: Some(at(500)),
                reference: Some("Van 2".to_string()),
                note: None,
                lines: lines
                    .iter()
                    .map(|&(v, quantity)| TransferLineDraft {
                        variant: vid(v),
                        quantity,
                    })
                    .collect(),
            },
        ))
    }

    fn ship(&self, id: TransferId) -> Result<hypha_inventory::Transfer, CommandError> {
        block_on(ship_transfer(&writer(), &self.ports, &self.clock, id, None)).map(|r| r.value)
    }

    fn receive(
        &self,
        id: TransferId,
        arrivals: &[(u128, u32, u32)],
    ) -> Result<hypha_inventory::Transfer, CommandError> {
        let arrivals: Vec<TransferArrival> = arrivals
            .iter()
            .map(|&(v, accepted, rejected)| TransferArrival {
                variant: vid(v),
                accepted,
                rejected,
            })
            .collect();
        block_on(receive_transfer(
            &writer(),
            &self.ports,
            &self.clock,
            id,
            &arrivals,
            None,
        ))
        .map(|r| r.value)
    }

    fn on_hand(&self, n: usize, location: LocationId) -> i64 {
        block_on(self.ports.inventory_items.get(self.items[n]))
            .unwrap()
            .unwrap()
            .on_hand(location)
    }

    fn rows(&self, n: usize) -> Vec<(Option<LocationId>, i64, StockReason, StockSource)> {
        let page = block_on(self.ports.stock_movements().list(
            self.items[n],
            &MovementFilter::default(),
            PageRequest {
                after: None,
                limit: PageLimit::new(50).unwrap(),
            },
        ))
        .unwrap();
        let mut rows: Vec<_> = page
            .items
            .iter()
            .map(|r| (r.location, r.delta, r.reason, r.source))
            .collect();
        rows.reverse();
        rows
    }
}

#[test]
fn a_transfer_is_drafted_between_two_existing_locations() {
    let world = World::new();
    let t = world.create(&[(1, 3)]).unwrap();
    assert_eq!(t.status(), TransferStatus::Draft);
    assert_eq!(t.number().to_string(), "TR-1001");
    assert_eq!(t.reference(), Some("Van 2"));
    assert_eq!(
        block_on(get_transfer(&writer(), &world.ports, t.id())),
        Ok(t)
    );

    let nowhere = LocationId::from_uuid(Uuid::from_u128(99));
    let input = CreateTransfer {
        origin: world.warehouse,
        destination: nowhere,
        expected_at: None,
        reference: None,
        note: None,
        lines: Vec::new(),
    };
    assert_eq!(
        block_on(create_transfer(&writer(), &world.ports, &world.ids, input)),
        Err(CommandError::LocationNotFound(nowhere))
    );
}

#[test]
fn only_stocked_variants_are_transferred() {
    let world = World::new();
    assert_eq!(
        world.create(&[(1, 1), (7, 1)]),
        Err(CommandError::Inventory(InventoryError::VariantNotStocked(
            vid(7)
        )))
    );
}

#[test]
fn transfers_need_the_inventory_scopes() {
    let world = World::new();
    let reader = staff(vec![Scope::InventoryRead]);
    let input = CreateTransfer {
        origin: world.warehouse,
        destination: world.shop,
        expected_at: None,
        reference: None,
        note: None,
        lines: Vec::new(),
    };
    assert_eq!(
        block_on(create_transfer(&reader, &world.ports, &world.ids, input)),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::InventoryWrite
        )))
    );
    let nobody = staff(Vec::new());
    assert_eq!(
        block_on(list_transfers(
            &nobody,
            &world.ports,
            &TransferFilter::default(),
            PageRequest {
                after: None,
                limit: PageLimit::new(10).unwrap()
            }
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::InventoryRead
        )))
    );
}

#[test]
fn a_draft_changes_until_it_ships() {
    let world = World::new();
    let t = world.create(&[(1, 3)]).unwrap();
    let input = UpdateTransfer {
        lines: Some(vec![
            TransferLineDraft {
                variant: vid(1),
                quantity: 2,
            },
            TransferLineDraft {
                variant: vid(2),
                quantity: 1,
            },
        ]),
        note: Some(Some("Fragile".to_string())),
        reference: Some(None),
        ..UpdateTransfer::default()
    };
    let updated = block_on(update_transfer(&writer(), &world.ports, t.id(), input)).unwrap();
    assert_eq!(updated.lines().len(), 2);
    assert_eq!(
        (updated.note(), updated.reference()),
        (Some("Fragile"), None)
    );

    world.ship(t.id()).unwrap();
    let input = UpdateTransfer {
        // A change that would be refused for itself is refused for the state first.
        destination: Some(world.warehouse),
        ..UpdateTransfer::default()
    };
    assert_eq!(
        block_on(update_transfer(&writer(), &world.ports, t.id(), input)),
        Err(CommandError::Inventory(InventoryError::TransferNotDraft {
            status: TransferStatus::InTransit
        }))
    );
}

#[test]
fn shipping_takes_every_line_off_the_origin() {
    let world = World::new();
    let t = world.create(&[(1, 3), (2, 6)]).unwrap();
    let shipped = world.ship(t.id()).unwrap();
    assert_eq!(shipped.status(), TransferStatus::InTransit);
    assert_eq!(shipped.shipped_at(), Some(at(100)));
    assert_eq!(world.on_hand(0, world.warehouse), 7);
    // More than was there: below zero, as fulfilling can.
    assert_eq!(world.on_hand(1, world.warehouse), -2);
    let source = StockSource::Transfer(t.id());
    assert_eq!(
        world.rows(1).last(),
        Some(&(Some(world.warehouse), -6, StockReason::TransferOut, source))
    );
    assert_eq!(
        world.ship(t.id()),
        Err(CommandError::Inventory(InventoryError::TransferNotDraft {
            status: TransferStatus::InTransit
        }))
    );
}

#[test]
fn receiving_puts_accepted_units_at_the_destination_and_records_lost_ones() {
    let world = World::new();
    let t = world.create(&[(1, 5), (2, 2)]).unwrap();
    world.ship(t.id()).unwrap();

    let partly = world.receive(t.id(), &[(1, 3, 1)]).unwrap();
    assert_eq!(partly.status(), TransferStatus::InTransit);
    assert_eq!(world.on_hand(0, world.shop), 3);
    let source = StockSource::Transfer(t.id());
    assert_eq!(
        world.rows(0)[world.rows(0).len() - 2..],
        [
            (Some(world.shop), 3, StockReason::TransferIn, source),
            (Some(world.shop), 0, StockReason::Shrinkage, source),
        ]
    );

    assert_eq!(
        world.receive(t.id(), &[(1, 2, 0), (2, 1, 0)]),
        Err(CommandError::Inventory(InventoryError::OverReceived(vec![
            InTransit {
                variant: vid(1),
                in_transit: 1
            }
        ])))
    );
    let done = world.receive(t.id(), &[(1, 1, 0), (2, 2, 0)]).unwrap();
    assert_eq!(done.status(), TransferStatus::Received);
    assert_eq!(done.received_at(), Some(at(100)));
    assert_eq!(
        (world.on_hand(0, world.shop), world.on_hand(1, world.shop)),
        (4, 2)
    );
    // The deltas at each location still add up to what's there.
    for n in [0, 1] {
        for location in [world.warehouse, world.shop] {
            let sum: i64 = world
                .rows(n)
                .iter()
                .filter(|r| r.0 == Some(location))
                .map(|r| r.1)
                .sum();
            assert_eq!(sum, world.on_hand(n, location));
        }
    }
}

#[test]
fn a_keyed_ship_or_receive_sent_again_answers_the_first() {
    let world = World::new();
    let t = world.create(&[(1, 5)]).unwrap();
    let first = block_on(ship_transfer(
        &writer(),
        &world.ports,
        &world.clock,
        t.id(),
        Some(key("s1")),
    ))
    .unwrap();
    let again = block_on(ship_transfer(
        &writer(),
        &world.ports,
        &world.clock,
        t.id(),
        Some(key("s1")),
    ))
    .unwrap();
    assert!(!first.replayed && again.replayed);
    assert_eq!(again.value, first.value);
    assert_eq!(world.on_hand(0, world.warehouse), 5, "shipped once");

    let arrival = [TransferArrival {
        variant: vid(1),
        accepted: 2,
        rejected: 0,
    }];
    let receive = |k| {
        block_on(receive_transfer(
            &writer(),
            &world.ports,
            &world.clock,
            t.id(),
            &arrival,
            Some(key(k)),
        ))
        .unwrap()
    };
    receive("r1");
    assert!(receive("r1").replayed);
    assert_eq!(world.on_hand(0, world.shop), 2, "received once");
    assert!(!receive("r2").replayed, "another key is another arrival");
    assert_eq!(world.on_hand(0, world.shop), 4);
}

#[test]
fn a_draft_is_cancelled_or_deleted_but_a_shipped_one_is_not() {
    let world = World::new();
    let cancelled = world.create(&[(1, 1)]).unwrap();
    let t = block_on(cancel_transfer(
        &writer(),
        &world.ports,
        &world.clock,
        cancelled.id(),
    ))
    .unwrap();
    assert_eq!(t.status(), TransferStatus::Cancelled);

    let deleted = world.create(&[(1, 1)]).unwrap();
    block_on(delete_transfer(&writer(), &world.ports, deleted.id())).unwrap();
    assert_eq!(
        block_on(get_transfer(&writer(), &world.ports, deleted.id())),
        Err(CommandError::TransferNotFound(deleted.id()))
    );

    let shipped = world.create(&[(1, 1)]).unwrap();
    world.ship(shipped.id()).unwrap();
    let in_transit = Err(CommandError::Inventory(InventoryError::TransferNotDraft {
        status: TransferStatus::InTransit,
    }));
    assert_eq!(
        block_on(delete_transfer(&writer(), &world.ports, shipped.id())),
        in_transit.clone().map(|_: ()| ())
    );
    assert_eq!(
        block_on(cancel_transfer(
            &writer(),
            &world.ports,
            &world.clock,
            shipped.id()
        ))
        .map(|_| ()),
        in_transit
    );
}

#[test]
fn transfers_are_listed_newest_first_and_record_their_events() {
    let world = World::new();
    let first = world.create(&[(1, 1)]).unwrap();
    let second = world.create(&[(2, 1)]).unwrap();
    world.ship(second.id()).unwrap();
    let page = block_on(list_transfers(
        &writer(),
        &world.ports,
        &TransferFilter {
            status: Some(TransferStatus::InTransit),
            ..TransferFilter::default()
        },
        PageRequest {
            after: None,
            limit: PageLimit::new(10).unwrap(),
        },
    ))
    .unwrap();
    assert_eq!(
        page.items.iter().map(|s| s.id).collect::<Vec<_>>(),
        [second.id()]
    );
    assert!(
        block_on(world.ports.transfers.get(first.id()))
            .unwrap()
            .is_some()
    );
    let types: Vec<String> = block_on(world.ports.outbox().events_after(0, 100))
        .unwrap()
        .into_iter()
        .map(|e| e.event_type)
        .filter(|t| t.starts_with("transfer."))
        .collect();
    assert_eq!(
        types,
        ["transfer.created", "transfer.created", "transfer.shipped"]
    );
}
