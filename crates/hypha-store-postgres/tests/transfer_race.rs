//! Transfers against a real Postgres, each request on its own connection: a shipment
//! racing checkouts' reservations on the same item retries until both are written with
//! the counts adding up, and receipts racing for one transfer never take in more than
//! shipped.
//!
//! Needs `DATABASE_URL` like the contract tests; skips itself without it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]
use futures_util::future::join_all;
use hypha_core::{
    Attempts, Ctx, InventoryItemId, LocationId, MarketId, Quantity, RepoError, TransferId,
    VariantId,
};
use hypha_inventory::{
    CommandError, InventoryError, InventoryItem, InventoryItemRepo, Location, LocationRepo,
    ReservationRef, Transfer, TransferArrival, TransferDraft, TransferLineDraft, TransferNumber,
    TransferRepo, TransferStatus, receive_transfer, ship_transfer,
};
use hypha_store_postgres::{CORE, PgStore, migrate};
use hypha_testkit::FixedClock;
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

/// IDs from 1000 up, so they never collide with `SequentialIds` (which counts from 1).
fn transfer_id() -> TransferId {
    TransferId::from_uuid(Uuid::from_u128(1000))
}

fn warehouse() -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(1001))
}

fn shop_floor() -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(1004))
}

fn market() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(1002))
}

fn variant() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(2000))
}

fn item_id() -> InventoryItemId {
    InventoryItemId::from_uuid(Uuid::from_u128(3000))
}

fn clock() -> std::sync::Arc<FixedClock> {
    std::sync::Arc::new(FixedClock::default())
}

async fn connect(url: &str, schema: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("connect to DATABASE_URL");
    tokio::spawn(connection);
    client
        .batch_execute(&format!("SET search_path TO {schema}"))
        .await
        .expect("set search_path");
    client
}

/// A fresh schema with a warehouse holding 100 of one tracked variant, a shop, and
/// transfer 1000 of `quantity` from the warehouse to the shop, shipped when `shipped`.
async fn stores(url: &str, schema: &str, quantity: u32, shipped: bool) -> PgStore<Client> {
    let mut client = connect(url, "public").await;
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; \
             SET search_path TO {schema};"
        ))
        .await
        .expect("reset schema");
    migrate(&mut client, &[CORE]).await.expect("migrate");
    let store = PgStore::new(client, clock());
    for (id, name) in [(warehouse(), "Warehouse"), (shop_floor(), "Shop")] {
        let mut location = Location::new(id, name.to_string(), vec![market()], 0).unwrap();
        LocationRepo::insert(&store, &mut location).await.unwrap();
    }
    let mut item = InventoryItem::new(item_id(), variant(), true, false);
    item.set_on_hand(warehouse(), 100, None, None).unwrap();
    InventoryItemRepo::insert(&store, &mut item).await.unwrap();
    let mut transfer = Transfer::new(
        transfer_id(),
        TransferNumber::new(1001).unwrap(),
        TransferDraft {
            origin: warehouse(),
            destination: shop_floor(),
            expected_at: None,
            reference: None,
            note: None,
            lines: vec![TransferLineDraft {
                variant: variant(),
                quantity,
            }],
        },
    )
    .unwrap();
    TransferRepo::insert(&store, &mut transfer).await.unwrap();
    if shipped {
        ship_transfer(
            &Ctx::system(),
            &store,
            &FixedClock::default(),
            transfer_id(),
            None,
        )
        .await
        .unwrap();
    }
    store
}

/// `n` stores, each on its own connection, so their transactions really overlap.
async fn staff(url: &str, schema: &str, n: usize) -> Vec<PgStore<Client>> {
    let mut stores = Vec::new();
    for _ in 0..n {
        stores.push(PgStore::new(connect(url, schema).await, clock()));
    }
    stores
}

/// A checkout's reservation of one unit at the warehouse, retried on a lost race up to
/// `MAX_ATTEMPTS` times as checkout does: whether it was written.
async fn reserve_one(store: &PgStore<Client>, reference: String) -> bool {
    let location = LocationRepo::get(store, warehouse())
        .await
        .unwrap()
        .unwrap();
    let reference = ReservationRef::new(&reference).unwrap();
    let mut attempts = Attempts::first();
    loop {
        let mut item = InventoryItemRepo::get(store, item_id())
            .await
            .unwrap()
            .unwrap();
        item.reserve(
            &reference,
            Quantity::new(1).unwrap(),
            std::slice::from_ref(&location),
        )
        .unwrap();
        match InventoryItemRepo::update(store, &mut item).await {
            Ok(()) => return true,
            Err(RepoError::StaleVersion) if attempts.another() => {}
            Err(RepoError::StaleVersion) => return false,
            Err(e) => panic!("reserving: {e:?}"),
        }
    }
}

#[tokio::test]
async fn a_shipment_racing_checkouts_on_its_item_keeps_the_counts_whole() {
    let Some(url) = hypha_store_postgres::testing::test_database_url() else {
        eprintln!("DATABASE_URL not set: skipping the transfer race test");
        return;
    };
    let ctx = Ctx::system();
    let clock = FixedClock::default();
    let mut shipments_won = 0;
    for round in 0..4 {
        let schema = format!("transfer_race_ship_{round}");
        let setup = stores(&url, &schema, 10, false).await;
        let racing = staff(&url, &schema, 9).await;
        let (shipped, reserved) = tokio::join!(
            ship_transfer(&ctx, &racing[0], &clock, transfer_id(), None),
            join_all(
                racing[1..]
                    .iter()
                    .enumerate()
                    .map(|(i, store)| reserve_one(store, format!("line-{i}")))
            ),
        );
        let reservations = reserved.iter().filter(|won| **won).count() as i64;
        let item = InventoryItemRepo::get(&setup, item_id())
            .await
            .unwrap()
            .unwrap();
        let transfer = TransferRepo::get(&setup, transfer_id())
            .await
            .unwrap()
            .unwrap();
        match shipped {
            // In whole: 10 left the warehouse and the transfer says so.
            Ok(_) => {
                shipments_won += 1;
                assert_eq!(
                    (item.on_hand(warehouse()), transfer.status()),
                    (90, TransferStatus::InTransit)
                );
            }
            // Lost every attempt: nothing of it was written.
            Err(e) => {
                assert_eq!(e, CommandError::Repo(RepoError::StaleVersion));
                assert_eq!(
                    (item.on_hand(warehouse()), transfer.status()),
                    (100, TransferStatus::Draft)
                );
            }
        }
        assert_eq!(item.reserved(warehouse()), reservations);
    }
    eprintln!("the shipment was written in {shipments_won} of 4 rounds against 8 checkouts");
}

#[tokio::test]
async fn receipts_racing_for_one_transfer_never_take_in_more_than_shipped() {
    let Some(url) = hypha_store_postgres::testing::test_database_url() else {
        eprintln!("DATABASE_URL not set: skipping the transfer race test");
        return;
    };
    const SCHEMA: &str = "transfer_race_receive";
    let setup = stores(&url, SCHEMA, 3, true).await;
    let ctx = Ctx::system();
    let clock = FixedClock::default();
    let one = [TransferArrival {
        variant: variant(),
        accepted: 1,
        rejected: 0,
    }];

    // 6 people each receive 1 of the 3 units shipped at the same moment.
    let racing = staff(&url, SCHEMA, 6).await;
    let results = join_all(
        racing
            .iter()
            .map(|store| receive_transfer(&ctx, store, &clock, transfer_id(), &one, None)),
    )
    .await;
    for r in &results {
        // Losers saw nothing left in transit (or a received transfer), or kept losing.
        assert!(
            matches!(
                r,
                Ok(_)
                    | Err(CommandError::Inventory(
                        InventoryError::OverReceived(_)
                            | InventoryError::TransferNotInTransit { .. }
                    ))
                    | Err(CommandError::Repo(RepoError::StaleVersion))
            ),
            "unexpected result {r:?}"
        );
    }
    let won = results.iter().filter(|r| r.is_ok()).count();
    assert!((1..=3).contains(&won), "{won} receipts of 3 units");
    let transfer = TransferRepo::get(&setup, transfer_id())
        .await
        .unwrap()
        .unwrap();
    let item = InventoryItemRepo::get(&setup, item_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (transfer.lines()[0].received(), item.on_hand(shop_floor())),
        (won as u32, won as i64),
        "the transfer and the stock agree"
    );
    eprintln!("{won} of 6 receipts won");
}
