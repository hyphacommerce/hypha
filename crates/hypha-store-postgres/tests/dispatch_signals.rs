//! A store tells its dispatch hook (`hypha_core::SignalsDispatch`) after a write that
//! gave the dispatcher work commits, and only then: the host wakes the jobs on it, so
//! writes without events (a cart's) cost no dispatch run. Needs `DATABASE_URL` like the
//! contract tests; skips itself without it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hypha_cart::{Cart, CartRepo};
use hypha_core::{
    CartId, ChannelId, CountryCode, Currency, HasBatch, MarketId, Money, OutboxRepo, Price,
    ProductId, SignalsDispatch, Timestamp, VariantId,
};
use hypha_products::{Handle, Product, ProductRepo, ProductStatus, StageProducts};
use hypha_store_postgres::{CORE, PgStore, migrate};
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

const SCHEMA: &str = "dispatch_signals";

/// A store on a fresh schema, with a hook that counts its calls.
async fn store(url: &str) -> (PgStore<Client>, Arc<AtomicUsize>) {
    let (mut client, connection) = hypha_store_postgres::database_config(url)
        .expect("DATABASE_URL")
        .connect(NoTls)
        .await
        .expect("connect to DATABASE_URL");
    tokio::spawn(connection);
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE; CREATE SCHEMA {SCHEMA}; \
             SET search_path TO {SCHEMA};"
        ))
        .await
        .expect("reset schema");
    migrate(&mut client, &[CORE]).await.expect("migrate");
    let store = PgStore::new(client, Arc::new(hypha_testkit::FixedClock::default()));
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    store.on_dispatch_work(Arc::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
    }));
    (store, calls)
}

fn product(n: u128, handle: &str) -> Product {
    Product::new_simple(
        ProductId::from_uuid(Uuid::from_u128(n)),
        Handle::new(handle).unwrap(),
        format!("Product {n}"),
        VariantId::from_uuid(Uuid::from_u128(n * 100 + 1)),
        Price::new(Money::new(1000, Currency::EUR).unwrap()).unwrap(),
    )
    .unwrap()
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_unix_micros(1_700_000_000_000_000 + seconds * 1_000_000)
}

#[tokio::test]
async fn only_committed_work_for_the_dispatcher_is_signalled() {
    let Some(url) = hypha_store_postgres::testing::test_database_url() else {
        eprintln!("DATABASE_URL not set: skipping the dispatch signal test");
        return;
    };
    let (store, calls) = store(&url).await;
    let count = || calls.load(Ordering::Relaxed);

    // A write with events: told once, after it commits.
    let mut mug = product(1, "mug");
    ProductRepo::insert(&store, &mut mug).await.unwrap();
    assert_eq!(count(), 1);

    // A write that records none (a new cart): not told.
    let mut cart = Cart::new(
        CartId::from_uuid(Uuid::from_u128(2)),
        MarketId::from_uuid(Uuid::from_u128(3)),
        CountryCode::new("HR").unwrap(),
        ChannelId::from_uuid(Uuid::from_u128(4)),
    );
    CartRepo::insert(&store, &mut cart).await.unwrap();
    assert_eq!(count(), 1);

    // A stale write rolls its events back: not told.
    let mut stale = mug.clone();
    mug.set_status(ProductStatus::Active).unwrap();
    ProductRepo::update(&store, &mut mug).await.unwrap();
    assert_eq!(count(), 2);
    stale.set_status(ProductStatus::Active).unwrap();
    assert!(ProductRepo::update(&store, &mut stale).await.is_err());
    assert_eq!(count(), 2);

    // A batch with events: told once.
    mug.set_status(ProductStatus::Archived).unwrap();
    let mut batch = store.batch();
    batch.update_product(&mut mug);
    store.commit(batch).await.unwrap();
    assert_eq!(count(), 3);

    // A delivery made due again is work too; one that doesn't exist isn't.
    store
        .fan_out(10, at(0), &|_| vec!["handler:audit".to_string()])
        .await
        .unwrap();
    let leased = store.lease_due(at(0), at(60), 1).await.unwrap();
    assert_eq!(count(), 3);
    store
        .retry_delivery("handler:audit", leased[0].id, at(120))
        .await
        .unwrap();
    assert_eq!(count(), 4);
    store
        .retry_delivery("handler:audit", Uuid::from_u128(99), at(120))
        .await
        .unwrap();
    assert_eq!(count(), 4);
}
