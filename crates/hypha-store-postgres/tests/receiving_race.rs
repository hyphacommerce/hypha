//! Receiving against a real Postgres, each request on its own connection: receipts racing
//! for one order never take in more than is outstanding, and a receipt racing checkouts'
//! reservations on the same items retries until both are written, with the counts adding
//! up.
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
    Attempts, Ctx, Currency, InventoryItemId, LocationId, MarketId, PurchaseOrderId, Quantity,
    RepoError, SupplierId, Timestamp, UnitCost, VariantId,
};
use hypha_inventory::{InventoryItem, InventoryItemRepo, Location, LocationRepo, ReservationRef};
use hypha_purchasing::{
    LineDraft, PurchaseOrder, PurchaseOrderDraft, PurchaseOrderNumber, PurchaseOrderRepo,
    PurchasingError, ReceiptRepo, Supplier, SupplierDraft, SupplierRepo,
};
use hypha_receiving::{CommandError, Receive, ReceiveLine, receive};
use hypha_store_postgres::{CORE, PgStore, migrate};
use hypha_testkit::{FixedClock, SequentialIds};
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

/// IDs from 1000 up, so they never collide with `SequentialIds` (which counts from 1).
fn po_id() -> PurchaseOrderId {
    PurchaseOrderId::from_uuid(Uuid::from_u128(1000))
}

fn warehouse() -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(1001))
}

fn market() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(1002))
}

fn variant(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(2000 + n))
}

fn item_id(n: u128) -> InventoryItemId {
    InventoryItemId::from_uuid(Uuid::from_u128(3000 + n))
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

/// A fresh schema with a warehouse, `lines` variants each tracked with 100 on hand at
/// €5, and order 1000 ordering `per_line` of each at €10.
async fn shop(url: &str, schema: &str, lines: u128, per_line: u32) -> PgStore<Client> {
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

    let mut location = Location::new(warehouse(), "Zagreb".to_string(), vec![market()], 0).unwrap();
    LocationRepo::insert(&store, &mut location).await.unwrap();
    let eur = |minor: &str| UnitCost::parse(minor, Currency::EUR).unwrap();
    for n in 0..lines {
        let mut item = InventoryItem::new(item_id(n), variant(n), true, false);
        item.set_on_hand(warehouse(), 100, None, None).unwrap();
        item.set_unit_cost(Some(eur("500")), None);
        InventoryItemRepo::insert(&store, &mut item).await.unwrap();
    }

    let supplier = SupplierId::from_uuid(Uuid::from_u128(1003));
    let mut s = Supplier::new(supplier, SupplierDraft::new("Spore Labs", Currency::EUR)).unwrap();
    SupplierRepo::insert(&store, &mut s).await.unwrap();
    let mut po = PurchaseOrder::new(
        po_id(),
        PurchaseOrderNumber::new(1001).unwrap(),
        supplier,
        PurchaseOrderDraft {
            destination: Some(warehouse()),
            lines: (0..lines)
                .map(|n| LineDraft {
                    variant: variant(n),
                    product_title: format!("Kit {n}"),
                    variant_title: None,
                    sku: None,
                    supplier_sku: None,
                    quantity: per_line,
                    unit_cost: eur("1000"),
                })
                .collect(),
            ..PurchaseOrderDraft::new(Currency::EUR)
        },
    )
    .unwrap();
    po.order(
        Some(Currency::EUR),
        "Spore Labs",
        None,
        Timestamp::from_unix_micros(1),
    )
    .unwrap();
    PurchaseOrderRepo::insert(&store, &mut po).await.unwrap();
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

fn receipt_of(lines: &[(u128, u32)]) -> Receive {
    Receive {
        purchase_order: po_id(),
        location: None,
        lines: lines
            .iter()
            .map(|&(n, accepted)| ReceiveLine {
                variant: variant(n),
                accepted,
                rejected: 0,
            })
            .collect(),
        allow_over: false,
        note: None,
        idempotency: None,
    }
}

/// A checkout's reservation of one unit of `n`, retried on a lost race up to
/// `MAX_ATTEMPTS` times as checkout does: whether it was written.
async fn reserve_one(store: &PgStore<Client>, n: u128, reference: String) -> bool {
    let location = LocationRepo::get(store, warehouse())
        .await
        .unwrap()
        .unwrap();
    let reference = ReservationRef::new(&reference).unwrap();
    let mut attempts = Attempts::first();
    loop {
        let mut item = InventoryItemRepo::get(store, item_id(n))
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
async fn receipts_racing_for_one_order_never_take_in_more_than_was_ordered() {
    let Some(url) = hypha_store_postgres::testing::test_database_url() else {
        eprintln!("DATABASE_URL not set: skipping the receiving race test");
        return;
    };
    const SCHEMA: &str = "receiving_race_order";
    let setup = shop(&url, SCHEMA, 1, 3).await;
    let (ids, clock) = (SequentialIds::default(), FixedClock::default());
    let ctx = Ctx::system();

    // 6 people each receive 1 of the 3 kits ordered at the same moment.
    let stores = staff(&url, SCHEMA, 6).await;
    let results = join_all(stores.iter().map(|store| {
        receive(
            &ctx,
            store,
            &ids,
            &clock,
            Some(Currency::EUR),
            receipt_of(&[(0, 1)]),
        )
    }))
    .await;
    for r in &results {
        // Losers saw nothing outstanding (or a closed order), or kept losing the race.
        assert!(
            matches!(
                r,
                Ok(_)
                    | Err(CommandError::Purchasing(
                        PurchasingError::OverReceived(_) | PurchasingError::NotOrdered { .. }
                    ))
                    | Err(CommandError::Repo(RepoError::StaleVersion))
            ),
            "unexpected result {r:?}"
        );
    }
    let won = results.iter().filter(|r| r.is_ok()).count();
    assert!((1..=3).contains(&won), "{won} receipts of 3 kits");
    let po = PurchaseOrderRepo::get(&setup, po_id())
        .await
        .unwrap()
        .unwrap();
    let receipts = ReceiptRepo::for_purchase_order(&setup, po_id())
        .await
        .unwrap();
    let item = InventoryItemRepo::get(&setup, item_id(0))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            po.lines()[0].received(),
            receipts.len(),
            item.on_hand(warehouse())
        ),
        (won as u32, won, 100 + won as i64),
        "the order, its receipts and the stock agree"
    );
    eprintln!("{won} of 6 receipts won");
}

#[tokio::test]
async fn a_receipt_racing_checkouts_on_its_items_keeps_the_counts_whole() {
    let Some(url) = hypha_store_postgres::testing::test_database_url() else {
        eprintln!("DATABASE_URL not set: skipping the receiving race test");
        return;
    };
    let (ids, clock) = (SequentialIds::default(), FixedClock::default());
    let ctx = Ctx::system();
    let eur = |minor: &str| UnitCost::parse(minor, Currency::EUR).unwrap();
    // A few rounds, so either side gets to win at least sometimes.
    let mut receipts_won = 0;
    for round in 0..4 {
        let schema = format!("receiving_race_checkout_{round}");
        let setup = shop(&url, &schema, 1, 10).await;
        let stores = staff(&url, &schema, 9).await;
        let (received, reserved) = tokio::join!(
            receive(
                &ctx,
                &stores[0],
                &ids,
                &clock,
                Some(Currency::EUR),
                receipt_of(&[(0, 10)]),
            ),
            join_all(stores[1..].iter().enumerate().map(|(i, store)| reserve_one(
                store,
                0,
                format!("line-{i}")
            )),),
        );
        let reservations = reserved.iter().filter(|won| **won).count() as i64;
        let item = InventoryItemRepo::get(&setup, item_id(0))
            .await
            .unwrap()
            .unwrap();
        let po = PurchaseOrderRepo::get(&setup, po_id())
            .await
            .unwrap()
            .unwrap();
        match received {
            // In whole: 100 at €5 and 10 at €10, the order counting them.
            Ok(_) => {
                receipts_won += 1;
                assert_eq!(
                    (
                        item.on_hand(warehouse()),
                        item.unit_cost(),
                        po.lines()[0].received()
                    ),
                    (110, Some(eur("545.4545")), 10)
                );
            }
            // Lost every attempt: nothing of it was written.
            Err(e) => {
                assert_eq!(e, CommandError::Repo(RepoError::StaleVersion));
                assert_eq!(
                    (
                        item.on_hand(warehouse()),
                        item.unit_cost(),
                        po.lines()[0].received()
                    ),
                    (100, Some(eur("500")), 0)
                );
            }
        }
        assert_eq!(item.reserved(warehouse()), reservations);
    }
    eprintln!("the receipt was written in {receipts_won} of 4 rounds against 8 checkouts");
}

#[tokio::test]
async fn a_big_receipt_during_checkouts() {
    let Some(url) = hypha_store_postgres::testing::test_database_url() else {
        eprintln!("DATABASE_URL not set: skipping the receiving race test");
        return;
    };
    const SCHEMA: &str = "receiving_race_big";
    const LINES: u128 = 300;
    let setup = shop(&url, SCHEMA, LINES, 5).await;
    let (ids, clock) = (SequentialIds::default(), FixedClock::default());
    let ctx = Ctx::system();
    // 20 checkouts, each reserving a unit of a different item the receipt also brings.
    let stores = staff(&url, SCHEMA, 21).await;
    let every: Vec<(u128, u32)> = (0..LINES).map(|n| (n, 5)).collect();
    let (received, checkouts) = tokio::join!(
        receive(
            &ctx,
            &stores[0],
            &ids,
            &clock,
            Some(Currency::EUR),
            receipt_of(&every),
        ),
        join_all(stores[1..].iter().enumerate().map(|(i, store)| reserve_one(
            store,
            (i as u128) * 15,
            format!("line-{i}")
        )),),
    );
    eprintln!(
        "a {LINES}-line receipt during 20 checkouts: {}; {} of the checkouts were written",
        if received.is_ok() {
            "written"
        } else {
            "gave up"
        },
        checkouts.iter().filter(|won| **won).count()
    );
    let po = PurchaseOrderRepo::get(&setup, po_id())
        .await
        .unwrap()
        .unwrap();
    let total: i64 = {
        let ids: Vec<VariantId> = (0..LINES).map(variant).collect();
        InventoryItemRepo::for_variants(&setup, &ids)
            .await
            .unwrap()
            .iter()
            .map(|i| i.on_hand(warehouse()))
            .sum()
    };
    match received {
        // Written whole: every line received, every item 5 up.
        Ok(_) => {
            assert!(po.lines().iter().all(|l| l.received() == 5));
            assert_eq!(total, (100 + 5) * LINES as i64);
        }
        // Gave up after its attempts: nothing of it was written.
        Err(e) => {
            assert_eq!(e, CommandError::Repo(RepoError::StaleVersion));
            assert!(po.lines().iter().all(|l| l.received() == 0));
            assert_eq!(total, 100 * LINES as i64);
        }
    }
}
