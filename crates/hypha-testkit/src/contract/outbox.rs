use hypha_core::{
    ActingAs, Actor, BasisPoints, CatalogId, Currency, DeliveryOutcome, Event, HasBatch, HasOutbox,
    MarketId, Money, OutboxRepo, PendingEvent, Price, PriceListId, ProductId, RepoError,
    StageEvents, StoredEvent, Subject, Timestamp, UserId, VariantId, Version, Versioned,
};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, HasCatalogs, HasPriceLists, PriceList, PriceListRepo,
    StageCatalogProducts, StageFixedPrices,
};
use hypha_products::{Handle, HasProducts, Product, ProductRepo, ProductStatus, StageProducts};
use serde::Serialize;
use uuid::Uuid;

use super::scenario;

/// Runs every outbox scenario (design note 0016, section 3), each on a fresh store from
/// `make`: an event is written if and only if the change that recorded it committed,
/// with the actor the ports act as and the version the write gave the aggregate.
pub async fn outbox<S>(make: impl AsyncFn() -> S)
where
    S: HasProducts + HasCatalogs + HasPriceLists + HasOutbox + HasBatch + ActingAs,
    S::Batch: StageProducts + StageCatalogProducts + StageFixedPrices + StageEvents,
{
    scenario("outbox: an insert writes its events, by the actor, at version 1");
    insert_writes_events(&make().await).await;
    scenario("outbox: an update writes its events at the new version");
    update_writes_events(&make().await).await;
    scenario("outbox: a stale update writes no events");
    stale_update_writes_none(&make().await).await;
    scenario("outbox: a batch writes its aggregates' events and staged ones, in order");
    batch_writes_events(&make().await).await;
    scenario("outbox: a failed batch writes no events");
    failed_batch_writes_none(&make().await).await;
    scenario("outbox: staged membership and prices commit with their events, or neither");
    set_based_writes(&make().await).await;
    scenario("outbox: fan_out makes a delivery per destination, once");
    fan_out_once(&make().await).await;
    scenario("outbox: lease_due hands a due delivery out once until its lease ends");
    leases(&make().await).await;
    scenario("outbox: outcomes are final or due again, with the attempts counted");
    outcomes(&make().await).await;
    scenario("outbox: prune_events drops old fanned-out events and their deliveries");
    prunes(&make().await).await;
    scenario("outbox: events_after reads after a seq, up to a limit");
    events_after_pages(&make().await).await;
}

fn product(n: u128, handle: &str) -> Product {
    Product::new_simple(
        ProductId::from_uuid(Uuid::from_u128(n)),
        Handle::new(handle).unwrap(),
        format!("Product {n}"),
        VariantId::from_uuid(Uuid::from_u128(n * 100 + 1)),
        Price::new(Money::new(1000, Currency::Eur).unwrap()).unwrap(),
    )
    .unwrap()
}

fn staff() -> Actor {
    Actor::User(UserId::from_uuid(Uuid::from_u128(9)))
}

async fn all<S: HasOutbox>(store: &S) -> Vec<StoredEvent> {
    store.outbox().events_after(0, 100).await.unwrap()
}

fn types(events: &[StoredEvent]) -> Vec<&str> {
    events.iter().map(|e| e.event_type.as_str()).collect()
}

async fn insert_writes_events<S: HasProducts + HasOutbox + ActingAs>(store: &S) {
    store.act_as(staff());
    let mut mug = product(1, "mug");
    store.products().insert(&mut mug).await.unwrap();
    let events = all(store).await;
    assert_eq!(types(&events), ["product.created"]);
    let created = &events[0];
    assert_eq!(created.version, 1);
    assert_eq!(
        (created.subject_kind.as_str(), created.subject_id),
        ("product", mug.id().as_uuid())
    );
    assert_eq!(created.subject_version, Some(Version::NEW.next()));
    assert_eq!(created.actor, staff());
    assert_eq!(created.data["handle"], "mug");
    assert_eq!(created.data["product"], mug.id().to_string());
    // Written once: a second write of the same aggregate has nothing left to write.
    mug.rename("Big mug").unwrap();
    store.products().update(&mut mug).await.unwrap();
    assert_eq!(
        types(&all(store).await),
        ["product.created", "product.updated"]
    );
}

async fn update_writes_events<S: HasProducts + HasOutbox + ActingAs>(store: &S) {
    let mut mug = product(1, "mug");
    store.products().insert(&mut mug).await.unwrap();
    mug.rename("Big mug").unwrap();
    mug.set_handle(Handle::new("big-mug").unwrap());
    store.products().update(&mut mug).await.unwrap();
    let events = all(store).await;
    let updated = events.last().unwrap();
    assert_eq!(updated.event_type, "product.updated");
    assert_eq!(
        updated.data["fields"],
        serde_json::json!(["handle", "title"])
    );
    assert_eq!(updated.subject_version, Some(mug.version()));
    assert_eq!(updated.actor, Actor::System);
}

async fn stale_update_writes_none<S: HasProducts + HasOutbox>(store: &S) {
    let mut mug = product(1, "mug");
    store.products().insert(&mut mug).await.unwrap();
    let mut stale = mug.clone();
    mug.rename("Big mug").unwrap();
    store.products().update(&mut mug).await.unwrap();
    stale.rename("Small mug").unwrap();
    assert_eq!(
        store.products().update(&mut stale).await,
        Err(RepoError::StaleVersion)
    );
    assert_eq!(
        types(&all(store).await),
        ["product.created", "product.updated"]
    );
}

/// A payload of `StageEvents`' own, as a set-based write would stage.
#[derive(Serialize)]
struct Tagged {
    tag: &'static str,
}

impl Event for Tagged {
    const TYPE: &'static str = "contract.tagged";
    const VERSION: u16 = 1;
}

fn tagged(tag: &'static str) -> PendingEvent {
    PendingEvent::new(
        Subject::new("contract", Uuid::from_u128(77)),
        &Tagged { tag },
    )
}

async fn batch_writes_events<S>(store: &S)
where
    S: HasProducts + HasOutbox + HasBatch + ActingAs,
    S::Batch: StageProducts + StageEvents,
{
    let mut mug = product(1, "mug");
    store.products().insert(&mut mug).await.unwrap();
    store.act_as(staff());
    mug.rename("Big mug").unwrap();
    let mut batch = store.batch();
    batch.update_product(&mut mug);
    batch.record_events(vec![tagged("a"), tagged("b")]);
    store.commit(batch).await.unwrap();

    let events = all(store).await;
    assert_eq!(
        types(&events),
        [
            "product.created",
            "product.updated",
            "contract.tagged",
            "contract.tagged"
        ]
    );
    assert_eq!(events[1].subject_version, Some(Version::new(2)));
    assert_eq!(events[1].actor, staff());
    let tags: Vec<&serde_json::Value> = events[2..].iter().map(|e| &e.data["tag"]).collect();
    assert_eq!(tags, ["a", "b"]);
    assert_eq!(events[2].subject_version, None);
    assert_eq!(mug.version(), Version::new(2));
    // What the batch stored carries no events: loading it and writing it again writes
    // only what changed since.
    let mut stored = store.products().get(mug.id()).await.unwrap().unwrap();
    stored.set_status(ProductStatus::Active).unwrap();
    store.products().update(&mut stored).await.unwrap();
    let events = all(store).await;
    assert_eq!(events.len(), 5);
    assert_eq!(events[4].data["fields"], serde_json::json!(["status"]));
}

async fn failed_batch_writes_none<S>(store: &S)
where
    S: HasProducts + HasOutbox + HasBatch,
    S::Batch: StageProducts + StageEvents,
{
    let mut mug = product(1, "mug");
    store.products().insert(&mut mug).await.unwrap();
    let mut stale = mug.clone();
    mug.rename("Big mug").unwrap();
    store.products().update(&mut mug).await.unwrap();

    stale.rename("Small mug").unwrap();
    let mut batch = store.batch();
    batch.record_events(vec![tagged("a")]);
    batch.update_product(&mut stale);
    assert_eq!(store.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(
        types(&all(store).await),
        ["product.created", "product.updated"]
    );
}

async fn events_after_pages<S: HasProducts + HasOutbox>(store: &S) {
    for n in 1..=3 {
        let mut product = product(n, &format!("p{n}"));
        store.products().insert(&mut product).await.unwrap();
    }
    let events = all(store).await;
    assert_eq!(events.len(), 3);
    assert!(events.windows(2).all(|w| w[0].seq < w[1].seq));
    let page = store.outbox().events_after(events[0].seq, 1).await.unwrap();
    assert_eq!(page, [events[1].clone()]);
    let ids: std::collections::HashSet<Uuid> = events.iter().map(|e| e.id).collect();
    assert_eq!(ids.len(), 3, "event IDs are unique");
}

async fn set_based_writes<S>(store: &S)
where
    S: HasProducts + HasCatalogs + HasPriceLists + HasOutbox + HasBatch,
    S::Batch: StageProducts + StageCatalogProducts + StageFixedPrices + StageEvents,
{
    let market = MarketId::from_uuid(Uuid::from_u128(5));
    let mut catalog = Catalog::new(
        CatalogId::from_uuid(Uuid::from_u128(6)),
        CatalogContext::Market(market),
        None,
    );
    store.catalogs().insert(&mut catalog).await.unwrap();
    let mut list = PriceList::new(
        PriceListId::from_uuid(Uuid::from_u128(7)),
        Currency::Eur,
        BasisPoints::new(0).unwrap(),
    );
    store.price_lists().insert(&mut list).await.unwrap();
    let member = ProductId::from_uuid(Uuid::from_u128(1));
    let variant = VariantId::from_uuid(Uuid::from_u128(101));
    let price = Price::new(Money::new(900, Currency::Eur).unwrap()).unwrap();

    let mut batch = store.batch();
    batch.add_catalog_products(catalog.id(), &[member]);
    batch.set_fixed_prices(list.id(), &[(variant, price)]);
    batch.record_events(vec![tagged("set")]);
    store.commit(batch).await.unwrap();
    let included = store
        .catalogs()
        .included_products(catalog.id(), &[member])
        .await
        .unwrap();
    assert!(included.contains(&member));
    let prices = store
        .price_lists()
        .fixed_prices(list.id(), &[variant])
        .await
        .unwrap();
    assert_eq!(prices.get(&variant), Some(&price));

    // A failure later in the batch takes the membership, prices and event with it.
    let mut stale = product(2, "stale");
    store.products().insert(&mut stale).await.unwrap();
    let mut older = stale.clone();
    stale.rename("Fresh").unwrap();
    store.products().update(&mut stale).await.unwrap();
    older.rename("Older").unwrap();
    let mut batch = store.batch();
    batch.remove_catalog_products(catalog.id(), &[member]);
    batch.remove_fixed_prices(list.id(), &[variant]);
    batch.record_events(vec![tagged("undone")]);
    batch.update_product(&mut older);
    assert_eq!(store.commit(batch).await, Err(RepoError::StaleVersion));
    let included = store
        .catalogs()
        .included_products(catalog.id(), &[member])
        .await
        .unwrap();
    assert!(included.contains(&member), "membership rolled back");
    let prices = store
        .price_lists()
        .fixed_prices(list.id(), &[variant])
        .await
        .unwrap();
    assert_eq!(prices.get(&variant), Some(&price), "price rolled back");
    let tags: Vec<serde_json::Value> = all(store)
        .await
        .iter()
        .filter(|e| e.event_type == "contract.tagged")
        .map(|e| e.data["tag"].clone())
        .collect();
    assert_eq!(tags, ["set"]);
}

fn at(secs: i64) -> Timestamp {
    Timestamp::from_unix_micros(1_800_000_000_000_000).plus_secs(secs)
}

/// `handler:audit` for every event, `handler:search` for products' too.
fn routes(event_type: &str) -> Vec<String> {
    let mut to = vec!["handler:audit".to_string()];
    if event_type.starts_with("product.") {
        to.push("handler:search".to_string());
    }
    to
}

fn audit_only(_: &str) -> Vec<String> {
    vec!["handler:audit".to_string()]
}

async fn three_products<S: HasProducts>(store: &S) {
    for n in 1..=3 {
        let mut product = product(n, &format!("p{n}"));
        store.products().insert(&mut product).await.unwrap();
    }
}

async fn fan_out_once<S: HasProducts + HasOutbox>(store: &S) {
    three_products(store).await;
    let outbox = store.outbox();
    assert_eq!(outbox.fan_out(2, at(0), &routes).await, Ok(2));
    assert_eq!(outbox.fan_out(10, at(0), &routes).await, Ok(1));
    assert_eq!(outbox.fan_out(10, at(0), &routes).await, Ok(0));
    let leased = outbox.lease_due(at(0), at(60), 100).await.unwrap();
    assert_eq!(leased.len(), 6, "two destinations for each of three events");
    let mut seen: Vec<(i64, &str)> = leased
        .iter()
        .map(|d| (d.event.seq, d.destination.as_str()))
        .collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 6, "no destination twice for an event");
    assert!(leased.iter().all(|d| d.attempts == 0));
    assert!(
        leased
            .iter()
            .all(|d| d.event.event_type == "product.created")
    );
}

async fn leases<S: HasProducts + HasOutbox>(store: &S) {
    three_products(store).await;
    let outbox = store.outbox();
    outbox.fan_out(10, at(0), &audit_only).await.unwrap();
    let count = |leased: Result<Vec<_>, RepoError>| leased.map(|d| d.len());
    // Not due yet.
    assert_eq!(count(outbox.lease_due(at(-1), at(60), 10).await), Ok(0));
    let first = outbox.lease_due(at(0), at(60), 2).await.unwrap();
    assert_eq!(first.len(), 2);
    // Leased: the next call only gets what's left.
    let second = outbox.lease_due(at(1), at(60), 10).await.unwrap();
    assert_eq!(second.len(), 1);
    assert!(first.iter().all(|d| d.id != second[0].id));
    assert_eq!(count(outbox.lease_due(at(30), at(90), 10).await), Ok(0));
    // A lease that ran out (its runner died) hands them out again.
    assert_eq!(count(outbox.lease_due(at(60), at(120), 10).await), Ok(3));
}

async fn outcomes<S: HasProducts + HasOutbox>(store: &S) {
    three_products(store).await;
    let outbox = store.outbox();
    outbox.fan_out(10, at(0), &audit_only).await.unwrap();
    let leased = outbox.lease_due(at(0), at(60), 10).await.unwrap();
    let outcomes = vec![
        (leased[0].id, DeliveryOutcome::Succeeded),
        (
            leased[1].id,
            DeliveryOutcome::Retry {
                error: "busy".to_string(),
                at: at(300),
            },
        ),
        (
            leased[2].id,
            DeliveryOutcome::Failed {
                error: "gone".to_string(),
            },
        ),
    ];
    outbox.record_outcomes(&outcomes).await.unwrap();
    // The lease is free, but the retry isn't due before its time.
    let early = outbox.lease_due(at(299), at(400), 10).await.unwrap();
    assert!(early.is_empty(), "{early:?}");
    let again = outbox.lease_due(at(300), at(400), 10).await.unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!((again[0].id, again[0].attempts), (leased[1].id, 1));
    // An unknown delivery is skipped.
    let unknown = (Uuid::from_u128(424242), DeliveryOutcome::Succeeded);
    assert_eq!(outbox.record_outcomes(&[unknown]).await, Ok(()));
}

async fn prunes<S: HasProducts + HasOutbox>(store: &S) {
    three_products(store).await;
    let outbox = store.outbox();
    // Only the first two are fanned out; the third stays until it is.
    outbox.fan_out(2, at(0), &audit_only).await.unwrap();
    let far_future = Timestamp::from_unix_micros(i64::MAX / 2);
    assert_eq!(outbox.prune_events(far_future, 100).await, Ok(2));
    assert_eq!(all(store).await.len(), 1);
    // Their deliveries went with them.
    let leased = outbox.lease_due(at(0), at(60), 10).await.unwrap();
    assert!(leased.is_empty(), "{leased:?}");
    // Nothing is older than the epoch.
    assert_eq!(outbox.prune_events(Timestamp::UNIX_EPOCH, 100).await, Ok(0));
}
