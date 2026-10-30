//! Batches that update and delete inventory items lock them in one order (review fix): a
//! batch that updates X and deletes Y racing one that updates Y and X must queue, never
//! deadlock. A deadlock is retried like a stale write, but Postgres only finds it after
//! `deadlock_timeout` (1 s by default), so every one costs a second.
//!
//! Needs `DATABASE_URL` like the contract tests; skips itself without it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]
use futures_util::future::join;
use hypha_core::{HasBatch, InventoryItemId, RepoError, VariantId};
use hypha_inventory::{InventoryItem, InventoryItemRepo, StageInventoryItems};
use hypha_store_postgres::{CORE, PgStore, migrate};
use hypha_testkit::FixedClock;
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

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

fn item(n: u128) -> InventoryItem {
    InventoryItem::new(
        InventoryItemId::from_uuid(Uuid::from_u128(n)),
        VariantId::from_uuid(Uuid::from_u128(1_000_000 + n)),
        true,
        false,
    )
}

#[tokio::test]
async fn updating_and_deleting_items_never_deadlocks() {
    let Some(url) = hypha_store_postgres::testing::test_database_url() else {
        eprintln!("DATABASE_URL not set: skipping the batch lock test");
        return;
    };
    const SCHEMA: &str = "batch_locks";
    let mut client = connect(&url, "public").await;
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE; CREATE SCHEMA {SCHEMA}; \
             SET search_path TO {SCHEMA};"
        ))
        .await
        .expect("reset schema");
    migrate(&mut client, &[CORE]).await.expect("migrate");
    let deadlocks = || async {
        let stats = connect(&url, SCHEMA).await;
        let row = stats
            .query_one(
                "SELECT deadlocks FROM pg_stat_database WHERE datname = current_database()",
                &[],
            )
            .await
            .unwrap();
        row.get::<_, i64>(0)
    };
    let before = deadlocks().await;
    let setup = PgStore::new(client, clock());
    let a = PgStore::new(connect(&url, SCHEMA).await, clock());
    let b = PgStore::new(connect(&url, SCHEMA).await, clock());

    for round in 0..40u128 {
        // Y sorts before X, so locking in staging order would cross.
        let (mut x, mut y) = (item(round * 2 + 1), item(round * 2));
        setup.insert(&mut x).await.unwrap();
        setup.insert(&mut y).await.unwrap();

        // A: update X, delete Y. B: update Y and X.
        let (mut ax, mut ay) = (x.clone(), y.clone());
        ax.set_continue_selling(true);
        ay.delete().unwrap();
        let mut batch_a = a.batch();
        batch_a.update_inventory_item(&mut ax);
        batch_a.delete_inventory_item(&mut ay);
        let (mut bx, mut by) = (x.clone(), y.clone());
        bx.set_tracked(false);
        by.set_tracked(false);
        let mut batch_b = b.batch();
        batch_b.update_inventory_item(&mut by);
        batch_b.update_inventory_item(&mut bx);

        let (ra, rb) = join(a.commit(batch_a), b.commit(batch_b)).await;
        for r in [&ra, &rb] {
            assert!(
                matches!(r, Ok(()) | Err(RepoError::StaleVersion)),
                "round {round}: {r:?}"
            );
        }
        // Exactly one won: both staged X at version 1.
        assert!(ra.is_ok() != rb.is_ok(), "round {round}: {ra:?} {rb:?}");
    }
    // Backends report their counts to the statistics within a second of going idle.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(deadlocks().await, before, "the batches deadlocked");
}
