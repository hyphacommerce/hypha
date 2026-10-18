use hypha_core::{
    FulfillmentId, LocationId, OrderId, OrderLineId, Quantity, RepoError, Timestamp, Version,
    Versioned,
};
use hypha_fulfillment::{
    Fulfillment, FulfillmentLine, FulfillmentRepo, NewFulfillment, Tracking, keys,
};
use uuid::Uuid;

use super::scenario;

/// Runs every `FulfillmentRepo` scenario, each on a fresh repo from `make`.
pub async fn fulfillment_repo<R: FulfillmentRepo>(make: impl AsyncFn() -> R) {
    scenario("fulfillment: insert stores every field at version 1");
    round_trips(&make().await).await;
    scenario("fulfillment: update stores tracking and a cancellation; stale and missing fail");
    updates(&make().await).await;
    scenario("fulfillment: unique ID");
    unique_id(&make().await).await;
    scenario("fulfillment: an order's fulfilments, newest first, and no other order's");
    lists_for_order(&make().await).await;
}

fn fid(n: u128) -> FulfillmentId {
    FulfillmentId::from_uuid(Uuid::from_u128(n))
}

fn oid(n: u128) -> OrderId {
    OrderId::from_uuid(Uuid::from_u128(1000 + n))
}

fn line(n: u128, quantity: u32) -> FulfillmentLine {
    FulfillmentLine {
        line: OrderLineId::from_uuid(Uuid::from_u128(n)),
        quantity: Quantity::new(quantity).unwrap(),
        stock_taken: true,
    }
}

/// A line whose units didn't come off any stock (an untracked item).
fn untracked_line(n: u128, quantity: u32) -> FulfillmentLine {
    FulfillmentLine {
        stock_taken: false,
        ..line(n, quantity)
    }
}

/// Fulfilment `n` of order `order`: one line, one tracking number with every field.
fn fulfillment(n: u128, order: u128) -> Fulfillment {
    with(n, order, vec![line(n * 10, 1)], vec![full_tracking("1")])
}

fn full_tracking(number: &str) -> Tracking {
    Tracking::new(
        Some("DHL Express"),
        number,
        Some("https://dhl.example/track?n=1"),
    )
    .unwrap()
}

fn with(n: u128, order: u128, lines: Vec<FulfillmentLine>, tracking: Vec<Tracking>) -> Fulfillment {
    Fulfillment::new(NewFulfillment {
        id: fid(n),
        order: oid(order),
        location: LocationId::from_uuid(Uuid::from_u128(2000 + n)),
        lines,
        tracking,
        notify_customer: false,
    })
    .unwrap()
}

async fn round_trips<R: FulfillmentRepo>(repo: &R) {
    let mut one = fulfillment(1, 1);
    repo.insert(&mut one).await.unwrap();
    assert_eq!(one.version(), Version::new(1));
    assert_eq!(repo.get(one.id()).await, Ok(Some(one)));

    // Lines in order (the largest quantity first), tracking with and without its
    // optional fields, in order; no tracking at all.
    let mut many = with(
        2,
        1,
        vec![line(23, u32::MAX), untracked_line(21, 1), line(22, 7)],
        vec![
            Tracking::new(None, "B-2", None).unwrap(),
            full_tracking("A-1"),
        ],
    );
    repo.insert(&mut many).await.unwrap();
    assert_eq!(repo.get(many.id()).await, Ok(Some(many)));
    let mut none = with(3, 2, vec![line(31, 1)], vec![]);
    repo.insert(&mut none).await.unwrap();
    assert_eq!(repo.get(none.id()).await, Ok(Some(none)));
    assert_eq!(repo.get(fid(99)).await, Ok(None));
}

async fn updates<R: FulfillmentRepo>(repo: &R) {
    let mut f = fulfillment(1, 1);
    repo.insert(&mut f).await.unwrap();
    let stale = f.clone();

    f.set_tracking(vec![full_tracking("2"), full_tracking("3")], false)
        .unwrap();
    repo.update(&mut f).await.unwrap();
    assert_eq!(f.version(), Version::new(2));
    assert_eq!(repo.get(f.id()).await, Ok(Some(f.clone())));

    // Cancelled to the microsecond; its tracking stays.
    f.cancel(Timestamp::from_unix_micros(1_767_225_600_000_001));
    repo.update(&mut f).await.unwrap();
    assert_eq!(repo.get(f.id()).await, Ok(Some(f)));

    let mut stale = stale;
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    let mut missing = fulfillment(2, 1);
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

async fn unique_id<R: FulfillmentRepo>(repo: &R) {
    repo.insert(&mut fulfillment(1, 1)).await.unwrap();
    assert_eq!(
        repo.insert(&mut fulfillment(1, 2)).await,
        Err(RepoError::Duplicate(keys::FULFILLMENT_ID))
    );
}

async fn lists_for_order<R: FulfillmentRepo>(repo: &R) {
    assert_eq!(repo.list_for_order(oid(1)).await, Ok(vec![]));
    for (n, order) in [(2, 1), (1, 1), (3, 2), (4, 1)] {
        repo.insert(&mut fulfillment(n, order)).await.unwrap();
    }
    let ids: Vec<FulfillmentId> = repo
        .list_for_order(oid(1))
        .await
        .unwrap()
        .iter()
        .map(Fulfillment::id)
        .collect();
    assert_eq!(ids, [fid(4), fid(2), fid(1)]);
    // Each as stored, cancelled ones too.
    let mut cancelled = repo.get(fid(2)).await.unwrap().unwrap();
    cancelled.cancel(Timestamp::from_unix_micros(1_767_225_600_000_000));
    repo.update(&mut cancelled).await.unwrap();
    let listed = repo.list_for_order(oid(1)).await.unwrap();
    assert_eq!(listed[1], cancelled);
    assert_eq!(listed[0], repo.get(fid(4)).await.unwrap().unwrap());
}
