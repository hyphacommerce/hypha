use hypha_core::{
    ActingAs, Actor, Currency, Event, HasBatch, HasOutbox, Money, OutboxRepo, PendingEvent, Price,
    ProductId, RepoError, StageEvents, StoredEvent, Subject, UserId, VariantId, Version, Versioned,
};
use hypha_products::{Handle, HasProducts, Product, ProductRepo, StageProducts};
use serde::Serialize;
use uuid::Uuid;

use super::scenario;

/// Runs every outbox scenario (design note 0016, section 3), each on a fresh store from
/// `make`: an event is written if and only if the change that recorded it committed,
/// with the actor the ports act as and the version the write gave the aggregate.
pub async fn outbox<S>(make: impl AsyncFn() -> S)
where
    S: HasProducts + HasOutbox + HasBatch + ActingAs,
    S::Batch: StageProducts + StageEvents,
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
    // Staging took the events: committing the aggregate again writes none.
    assert_eq!(mug.version(), Version::new(2));
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
