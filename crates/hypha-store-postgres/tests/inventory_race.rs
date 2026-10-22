//! Concurrent checkouts against a real Postgres: buyers on separate connections race for
//! the last units, and stock is never oversold.
//!
//! Needs `DATABASE_URL` like the contract tests; skips itself without it.

use futures_util::future::join_all;
use hypha_core::{Ctx, InventoryItemId, LocationId, MarketId, RepoError, VariantId};
use hypha_inventory::{
    CommandError, InventoryItem, InventoryItemRepo, Location, LocationRepo, ReserveLine,
    ReserveStock, reserve_stock,
};
use hypha_store_postgres::{CORE, PgStore, migrate};
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

const SCHEMA: &str = "inventory_race";

async fn connect(url: &str) -> Client {
    let (client, connection) = hypha_store_postgres::database_config(url)
        .expect("DATABASE_URL")
        .connect(NoTls)
        .await
        .expect("connect to DATABASE_URL");
    tokio::spawn(connection);
    client
        .batch_execute(&format!("SET search_path TO {SCHEMA}"))
        .await
        .expect("set search_path");
    client
}

#[tokio::test]
async fn buyers_racing_for_the_last_units_never_oversell() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping the inventory race test");
        return;
    };
    let mut setup = connect(&url).await;
    setup
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE; CREATE SCHEMA {SCHEMA}; \
             SET search_path TO {SCHEMA};"
        ))
        .await
        .unwrap();
    migrate(&mut setup, &[CORE]).await.unwrap();
    let setup = PgStore::new(setup, clock());

    let market = MarketId::from_uuid(Uuid::from_u128(1));
    let location = LocationId::from_uuid(Uuid::from_u128(2));
    let variant = VariantId::from_uuid(Uuid::from_u128(3));
    let item_id = InventoryItemId::from_uuid(Uuid::from_u128(4));
    let mut l = Location::new(location, "Zagreb".to_string(), vec![market], 0).unwrap();
    LocationRepo::insert(&setup, &mut l).await.unwrap();
    let mut item = InventoryItem::new(item_id, variant, true, false);
    item.set_on_hand(location, 3, None).unwrap();
    InventoryItemRepo::insert(&setup, &mut item).await.unwrap();

    // 8 buyers, each on its own connection, each wanting 1 of the 3 units.
    let mut buyers = Vec::new();
    for _ in 0..8 {
        buyers.push(PgStore::new(connect(&url).await, clock()));
    }
    let results = join_all(buyers.iter().enumerate().map(|(n, store)| async move {
        let input = ReserveStock {
            market,
            lines: vec![ReserveLine {
                reference: format!("buyer-{n}"),
                variant,
                quantity: 1,
            }],
        };
        reserve_stock(&Ctx::system(), store, store, input).await
    }))
    .await;

    let won = results.iter().filter(|r| r.is_ok()).count();
    for r in &results {
        // Losers either saw the stock gone or kept losing the race: never anything else.
        assert!(
            matches!(
                r,
                Ok(())
                    | Err(CommandError::OutOfStock { .. })
                    | Err(CommandError::Repo(RepoError::StaleVersion))
            ),
            "unexpected result {r:?}"
        );
    }
    let stored = InventoryItemRepo::get(&setup, item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.reservations().len(),
        won,
        "one reservation per winner"
    );
    assert!(won <= 3, "oversold: {won} buyers got 3 units");
    assert!(stored.available(location) >= 0);
    eprintln!("{won} of 8 buyers reserved; results: {results:?}");
}

/// Writes are stamped with a fixed time in these tests.
fn clock() -> std::sync::Arc<hypha_testkit::FixedClock> {
    std::sync::Arc::new(hypha_testkit::FixedClock::default())
}
