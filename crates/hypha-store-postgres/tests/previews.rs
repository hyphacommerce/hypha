//! A store in a preview (`PgStore::begin_preview`) runs its writes for real in one
//! transaction that's rolled back: each write is a savepoint, so a failing one is undone
//! alone and the preview goes on, reads see the writes before them, and `PgPreview::end`
//! gives back the events the kept writes recorded. Needs `HYPHA_TEST_DATABASE_URL` (or
//! `DATABASE_URL`) like the contract tests; skips itself without it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use hypha_core::{
    Currency, HasBatch, Money, Price, ProductId, RepoError, SignalsDispatch, VariantId,
};
use hypha_products::{Handle, Product, ProductRepo, ProductStatus, StageProducts, repo::keys};
use hypha_store_postgres::testing::TestDatabase;
use hypha_store_postgres::{CORE, PgConnection, PgStore, Queries, database_config};
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

/// A connection that notes what the store tells it about previews.
struct Watched {
    client: Client,
    open: Arc<AtomicBool>,
}

impl PgConnection for Watched {
    fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    fn preview_open(&mut self, open: bool) {
        self.open.store(open, Ordering::Relaxed);
    }
}

async fn connect(url: &str) -> Client {
    let (client, connection) = database_config(url)
        .expect("the test database's URL")
        .connect(NoTls)
        .await
        .expect("connect to the test database");
    tokio::spawn(connection);
    client
}

/// A database of the test's own, a store on it (on a `Watched` connection, whose flag
/// comes back too) and another connection, which sees only what was committed.
async fn setup(name: &str) -> Option<(TestDatabase, PgStore<Watched>, Arc<AtomicBool>, Client)> {
    let Some(db) = TestDatabase::new(name, &[CORE]).await else {
        eprintln!("no test database: skipping the preview tests");
        return None;
    };
    let open = Arc::new(AtomicBool::new(false));
    let conn = Watched {
        client: connect(db.url()).await,
        open: Arc::clone(&open),
    };
    let store = PgStore::new(conn, Arc::new(hypha_testkit::FixedClock::default()));
    let other = connect(db.url()).await;
    Some((db, store, open, other))
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

async fn count(client: &Client, table: &str) -> i64 {
    client
        .query_one(&format!("SELECT count(*) FROM {table}"), &[])
        .await
        .unwrap()
        .get(0)
}

const TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn a_previews_writes_are_seen_inside_it_and_gone_after() {
    let Some((_db, store, open, other)) = setup("store_preview_rolls_back").await else {
        return;
    };
    let mut committed = product(1, "committed");
    ProductRepo::insert(&store, &mut committed).await.unwrap();
    let before = count(&other, "outbox_events").await;

    let preview = store.begin_preview(TIMEOUT).await.unwrap();
    assert!(open.load(Ordering::Relaxed), "the connection knows");
    let mut mug = product(2, "mug");
    ProductRepo::insert(&store, &mut mug).await.unwrap();
    let mut changed = ProductRepo::get(&store, committed.id())
        .await
        .unwrap()
        .unwrap();
    changed.set_status(ProductStatus::Active).unwrap();
    ProductRepo::update(&store, &mut changed).await.unwrap();
    // Reads in the preview see its writes; others don't.
    assert!(ProductRepo::get(&store, mug.id()).await.unwrap().is_some());
    assert_eq!(count(&other, "products").await, 1);

    let events = preview.end().await.unwrap();
    assert!(!open.load(Ordering::Relaxed), "rolled back");
    let subjects: Vec<Uuid> = events.iter().map(|e| e.subject_id).collect();
    assert!(subjects.contains(&mug.id().as_uuid()), "{events:?}");
    assert!(subjects.contains(&committed.id().as_uuid()), "{events:?}");
    assert!(events.windows(2).all(|w| w[0].seq < w[1].seq), "in order");

    assert_eq!(count(&other, "products").await, 1);
    assert_eq!(count(&other, "outbox_events").await, before);
    let status: String = other
        .query_one("SELECT status FROM products", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(status, "draft");
}

#[tokio::test]
async fn a_duplicate_in_a_preview_fails_alone() {
    let Some((_db, store, _open, other)) = setup("store_preview_duplicate").await else {
        return;
    };
    let preview = store.begin_preview(TIMEOUT).await.unwrap();
    let mut mug = product(1, "mug");
    ProductRepo::insert(&store, &mut mug).await.unwrap();
    let mut again = product(2, "mug");
    assert_eq!(
        ProductRepo::insert(&store, &mut again).await,
        Err(RepoError::Duplicate(keys::HANDLE))
    );
    // The preview goes on: the failed write's savepoint was rolled back, not the
    // preview's transaction.
    let mut cup = product(3, "cup");
    ProductRepo::insert(&store, &mut cup).await.unwrap();

    let events = preview.end().await.unwrap();
    let subjects: Vec<Uuid> = events.iter().map(|e| e.subject_id).collect();
    assert!(subjects.contains(&mug.id().as_uuid()));
    assert!(subjects.contains(&cup.id().as_uuid()));
    assert!(
        !subjects.contains(&again.id().as_uuid()),
        "the failed write's events went with it"
    );
    assert_eq!(count(&other, "products").await, 0);
}

#[tokio::test]
async fn a_stale_write_in_a_preview_fails_as_it_would_and_the_next_one_works() {
    let Some((_db, store, _open, other)) = setup("store_preview_stale").await else {
        return;
    };
    let mut mug = product(1, "mug");
    ProductRepo::insert(&store, &mut mug).await.unwrap();

    let preview = store.begin_preview(TIMEOUT).await.unwrap();
    let mut stale = mug.clone();
    mug.set_status(ProductStatus::Active).unwrap();
    ProductRepo::update(&store, &mut mug).await.unwrap();
    stale.set_status(ProductStatus::Active).unwrap();
    assert_eq!(
        ProductRepo::update(&store, &mut stale).await,
        Err(RepoError::StaleVersion)
    );
    mug.set_status(ProductStatus::Archived).unwrap();
    ProductRepo::update(&store, &mut mug).await.unwrap();
    let events = preview.end().await.unwrap();
    assert_eq!(events.len(), 2, "{events:?}");

    let version: i64 = other
        .query_one("SELECT version FROM products", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(version, 1, "nothing committed");
}

#[tokio::test]
async fn batches_in_a_preview_commit_to_it_or_roll_back_alone() {
    let Some((_db, store, _open, other)) = setup("store_preview_batches").await else {
        return;
    };
    let mut mug = product(1, "mug");
    ProductRepo::insert(&store, &mut mug).await.unwrap();
    let signals = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&signals);
    store.on_dispatch_work(Arc::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
    }));

    let preview = store.begin_preview(TIMEOUT).await.unwrap();
    let mut stale = mug.clone();
    mug.set_status(ProductStatus::Active).unwrap();
    let mut batch = store.batch();
    batch.update_product(&mut mug);
    store.commit(batch).await.unwrap();
    // A batch that loses a race is rolled back to its savepoint, its events with it.
    stale.set_status(ProductStatus::Active).unwrap();
    let mut batch = store.batch();
    batch.update_product(&mut stale);
    assert_eq!(store.commit(batch).await, Err(RepoError::StaleVersion));

    let events = preview.end().await.unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        signals.load(Ordering::Relaxed),
        0,
        "nothing for the dispatcher"
    );
    assert_eq!(
        other
            .query_one("SELECT status FROM products", &[])
            .await
            .unwrap()
            .get::<_, String>(0),
        "draft"
    );
    assert_eq!(count(&other, "outbox_events").await, 1, "only the setup's");
}

#[tokio::test]
async fn a_failing_statement_outside_a_write_doesnt_abort_the_preview() {
    let Some((_db, store, _open, _other)) = setup("store_preview_statement").await else {
        return;
    };
    let preview = store.begin_preview(TIMEOUT).await.unwrap();
    let mut mug = product(1, "mug");
    ProductRepo::insert(&store, &mut mug).await.unwrap();
    {
        let mut conn = store.conn().await;
        assert!(conn.db().query_typed("SELECT 1 / 0", &[]).await.is_err());
    }
    // Still in the preview, its write still there.
    assert!(ProductRepo::get(&store, mug.id()).await.unwrap().is_some());
    assert_eq!(preview.end().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_preview_dropped_unended_leaves_its_connection_marked() {
    let Some((_db, store, open, other)) = setup("store_preview_dropped").await else {
        return;
    };
    let preview = store.begin_preview(TIMEOUT).await.unwrap();
    let mut mug = product(1, "mug");
    ProductRepo::insert(&store, &mut mug).await.unwrap();
    drop(preview);
    drop(store);
    assert!(
        open.load(Ordering::Relaxed),
        "a pool rolls a connection marked open back before handing it out"
    );
    assert_eq!(count(&other, "products").await, 0, "never committed");
}

#[tokio::test]
async fn a_preview_cuts_off_slow_statements() {
    let Some((_db, store, _open, _other)) = setup("store_preview_timeout").await else {
        return;
    };
    let preview = store
        .begin_preview(Duration::from_millis(50))
        .await
        .unwrap();
    {
        let mut conn = store.conn().await;
        assert!(
            conn.db()
                .query_typed("SELECT pg_sleep(1)", &[])
                .await
                .is_err()
        );
    }
    preview.end().await.unwrap();
}

#[tokio::test]
async fn an_apply_commits_or_rolls_back_as_a_whole_once_its_events_are_read() {
    use hypha_core::{Actor, PreviewToken, PreviewTokenRepo, Timestamp};
    let Some((_db, store, open, other)) = setup("store_apply").await else {
        return;
    };
    let signals = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&signals);
    store.on_dispatch_work(Arc::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
    }));

    // Rolled back: nothing kept, though two writes were; the token is stored after it.
    let apply = store.begin_apply().await.unwrap();
    ProductRepo::insert(&store, &mut product(1, "mug"))
        .await
        .unwrap();
    ProductRepo::insert(&store, &mut product(2, "cup"))
        .await
        .unwrap();
    let events = apply.events().await.unwrap();
    assert_eq!(events.len(), 2, "{events:?}");
    let token = PreviewToken {
        hash: [3; 32],
        actor: Actor::System,
        request: [4; 32],
        ids: vec![Uuid::from_u128(5)],
        digest: "sha256:00".to_string(),
        expires_at: Timestamp::from_unix_micros(9),
        used_at: None,
    };
    apply.finish(false, Some(token.clone())).await.unwrap();
    assert!(!open.load(Ordering::Relaxed));
    assert_eq!(count(&other, "products").await, 0);
    assert_eq!(signals.load(Ordering::Relaxed), 0);
    // The store that ran the apply takes no more statements; a fresh one finds it.
    let fresh = PgStore::new(
        connect(_db.url()).await,
        Arc::new(hypha_testkit::FixedClock::default()),
    );
    assert_eq!(
        PreviewTokenRepo::get(&fresh, &token.hash).await,
        Ok(Some(token))
    );

    // Committed: both writes, one signal for the dispatcher, after the COMMIT.
    let store = {
        let conn = Watched {
            client: connect(_db.url()).await,
            open: Arc::clone(&open),
        };
        PgStore::new(conn, Arc::new(hypha_testkit::FixedClock::default()))
    };
    let counted = Arc::clone(&signals);
    store.on_dispatch_work(Arc::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
    }));
    let apply = store.begin_apply().await.unwrap();
    ProductRepo::insert(&store, &mut product(1, "mug"))
        .await
        .unwrap();
    ProductRepo::insert(&store, &mut product(2, "cup"))
        .await
        .unwrap();
    assert_eq!(count(&other, "products").await, 0, "not before the commit");
    assert_eq!(apply.events().await.unwrap().len(), 2);
    apply.finish(true, None).await.unwrap();
    assert_eq!(count(&other, "products").await, 2);
    assert_eq!(signals.load(Ordering::Relaxed), 1);
}
