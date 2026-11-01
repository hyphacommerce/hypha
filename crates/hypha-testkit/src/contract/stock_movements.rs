use std::collections::HashMap;

use hypha_core::{
    ActingAs, Actor, FulfillmentId, HasBatch, InventoryItemId, LocationId, OrderId, PageLimit,
    PageRequest, PurchaseOrderId, Quantity, ReceiptId, RepoError, Timestamp, TransferId, UserId,
    VariantId,
};
use hypha_inventory::{
    AdjustmentReason, HasInventoryItems, HasStockMovements, InventoryItem, InventoryItemRepo,
    MovementFilter, MovementNote, ReservationRef, StageInventoryItems, StockMovementEntry,
    StockMovementRepo, StockReason, StockSource,
};
use uuid::Uuid;

use super::scenario;

/// Runs every stock ledger scenario, each on fresh ports from `make`: every path that
/// writes an item writes its movements in the same commit, and the ledger reads back.
pub async fn stock_movements<S>(make: impl AsyncFn() -> S)
where
    S: HasBatch + HasInventoryItems + HasStockMovements + ActingAs,
    S::Batch: StageInventoryItems,
{
    scenario("stock movements: every write path records, and the deltas add up to on hand");
    every_path_adds_up(&make().await).await;
    scenario("stock movements: a stale write records nothing");
    stale_writes_record_nothing(&make().await).await;
    scenario("stock movements: every field round-trips, with the actor");
    fields_round_trip(&make().await).await;
    scenario("stock movements: newest first, paged, filtered, per item");
    lists(&make().await).await;
}

fn location(n: u128) -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(n))
}

fn new_item(n: u128) -> InventoryItem {
    InventoryItem::new(
        InventoryItemId::from_uuid(Uuid::from_u128(n)),
        VariantId::from_uuid(Uuid::from_u128(100 + n)),
        true,
        false,
    )
}

fn fulfilment(n: u128) -> StockSource {
    StockSource::Fulfillment {
        order: OrderId::from_uuid(Uuid::from_u128(200 + n)),
        fulfillment: FulfillmentId::from_uuid(Uuid::from_u128(300 + n)),
    }
}

fn page(after: Option<i64>, limit: u32) -> PageRequest<i64> {
    PageRequest {
        after,
        limit: PageLimit::new(limit).unwrap(),
    }
}

/// Every row of `item` that `filter` matches, newest first, through pages of 2.
async fn all<S: HasStockMovements>(
    ports: &S,
    item: InventoryItemId,
    filter: &MovementFilter,
) -> Vec<StockMovementEntry> {
    let mut rows = Vec::new();
    let mut after = None;
    loop {
        let found = ports
            .stock_movements()
            .list(item, filter, page(after, 2))
            .await
            .unwrap();
        after = found.items.last().map(|r| r.position);
        rows.extend(found.items);
        if !found.has_more {
            return rows;
        }
    }
}

/// The sum of each location's deltas.
fn sums(rows: &[StockMovementEntry]) -> HashMap<LocationId, i64> {
    let mut sums = HashMap::new();
    for row in rows {
        if let Some(location) = row.location {
            *sums.entry(location).or_default() += row.delta;
        }
    }
    sums
}

fn assert_adds_up(rows: &[StockMovementEntry], item: &InventoryItem) {
    let sums = sums(rows);
    for level in item.levels() {
        assert_eq!(
            sums.get(&level.location).copied().unwrap_or(0),
            level.on_hand,
            "{:?}",
            level.location
        );
    }
    assert_eq!(
        sums.len(),
        item.levels().iter().filter(|l| l.on_hand != 0).count()
    );
    // Each row's `after` is the sum up to it.
    let mut running: HashMap<LocationId, i64> = HashMap::new();
    for row in rows.iter().rev() {
        let at = running.entry(row.location.unwrap()).or_default();
        *at += row.delta;
        assert_eq!(row.after, Some(*at), "position {}", row.position);
    }
}

async fn every_path_adds_up<S>(ports: &S)
where
    S: HasBatch + HasInventoryItems + HasStockMovements,
    S::Batch: StageInventoryItems,
{
    let items = ports.inventory_items();
    let none = MovementFilter::default();

    // insert with levels
    let mut a = new_item(1);
    a.set_on_hand(location(1), 5, None, None).unwrap();
    a.set_on_hand(location(2), 3, None, None).unwrap();
    items.insert(&mut a).await.unwrap();
    // update
    a.adjust_on_hand(location(1), -2, AdjustmentReason::Damaged, None)
        .unwrap();
    items.update(&mut a).await.unwrap();
    // update_many, beside another item
    let mut b = new_item(2);
    items.insert(&mut b).await.unwrap();
    a.set_on_hand(location(2), 10, None, None).unwrap();
    b.adjust_on_hand(location(1), 4, AdjustmentReason::Correction, None)
        .unwrap();
    let mut both = [a, b];
    items.update_many(&mut both).await.unwrap();
    let [mut a, mut b] = both;
    // a batch update: a fulfilment and a restock
    let reference = ReservationRef::new("line").unwrap();
    let mut batch = ports.batch();
    a.fulfil(
        &reference,
        location(1),
        Quantity::new(2).unwrap(),
        fulfilment(1),
    )
    .unwrap();
    a.restock(
        &reference,
        location(2),
        Quantity::new(1).unwrap(),
        fulfilment(1),
    )
    .unwrap();
    batch.update_inventory_item(&mut a);
    ports.commit(batch).await.unwrap();

    let stored = items.get(a.id()).await.unwrap().unwrap();
    let rows = all(ports, a.id(), &none).await;
    assert_eq!(rows.len(), 6);
    assert_adds_up(&rows, &stored);
    assert_eq!(
        rows.iter().map(|r| r.reason).collect::<Vec<_>>(),
        [
            StockReason::Restocked,
            StockReason::Sold,
            StockReason::Counted,
            StockReason::Damaged,
            StockReason::Counted,
            StockReason::Counted
        ]
    );

    // a batch delete and a repo delete keep their last rows
    b.adjust_on_hand(location(1), -4, AdjustmentReason::Shrinkage, None)
        .unwrap();
    b.delete().unwrap();
    let mut batch = ports.batch();
    batch.delete_inventory_item(&mut b);
    ports.commit(batch).await.unwrap();
    assert_eq!(items.get(b.id()).await, Ok(None));
    let rows = all(ports, b.id(), &none).await;
    assert_eq!(
        rows.iter().map(|r| (r.delta, r.reason)).collect::<Vec<_>>(),
        [(-4, StockReason::Shrinkage), (4, StockReason::Correction)]
    );

    let mut c = new_item(3);
    c.set_on_hand(location(1), 2, None, None).unwrap();
    items.insert(&mut c).await.unwrap();
    c.adjust_on_hand(location(1), -2, AdjustmentReason::Other, None)
        .unwrap();
    c.delete().unwrap();
    items.delete(&mut c).await.unwrap();
    assert_eq!(sums(&all(ports, c.id(), &none).await)[&location(1)], 0);
}

async fn stale_writes_record_nothing<S>(ports: &S)
where
    S: HasBatch + HasInventoryItems + HasStockMovements,
    S::Batch: StageInventoryItems,
{
    let items = ports.inventory_items();
    let mut item = new_item(1);
    item.set_on_hand(location(1), 5, None, None).unwrap();
    items.insert(&mut item).await.unwrap();
    let mut stale = item.clone();
    item.adjust_on_hand(location(1), 1, AdjustmentReason::Other, None)
        .unwrap();
    items.update(&mut item).await.unwrap();

    stale
        .adjust_on_hand(location(1), 7, AdjustmentReason::Other, None)
        .unwrap();
    let mut once = stale.clone();
    assert_eq!(
        items.update_many(std::slice::from_mut(&mut once)).await,
        Err(RepoError::StaleVersion)
    );
    let mut batch = ports.batch();
    batch.update_inventory_item(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));

    let rows = all(ports, item.id(), &MovementFilter::default()).await;
    assert_eq!(
        rows.iter().map(|r| r.delta).collect::<Vec<_>>(),
        [1, 5],
        "only what committed"
    );
}

async fn fields_round_trip<S>(ports: &S)
where
    S: HasBatch + HasInventoryItems + HasStockMovements + ActingAs,
    S::Batch: StageInventoryItems,
{
    let items = ports.inventory_items();
    let staff = Actor::User(UserId::from_uuid(Uuid::from_u128(9)));
    ports.act_as(staff);
    let mut item = new_item(1);
    let note = MovementNote::new("  Counted twice  ").unwrap();
    item.set_on_hand(location(1), 3, None, note).unwrap();
    items.insert(&mut item).await.unwrap();

    // Every kind of source survives storage.
    let sources = [
        fulfilment(1),
        StockSource::Receipt {
            purchase_order: PurchaseOrderId::from_uuid(Uuid::from_u128(400)),
            receipt: ReceiptId::from_uuid(Uuid::from_u128(401)),
        },
        StockSource::PurchaseOrder(PurchaseOrderId::from_uuid(Uuid::from_u128(402))),
        StockSource::Transfer(TransferId::from_uuid(Uuid::from_u128(403))),
        StockSource::Migration,
    ];
    let reference = ReservationRef::new("line").unwrap();
    for source in sources {
        item.restock(&reference, location(2), Quantity::new(1).unwrap(), source)
            .unwrap();
    }
    items.update(&mut item).await.unwrap();

    let rows = all(ports, item.id(), &MovementFilter::default()).await;
    let first = rows.last().unwrap();
    assert_eq!(
        (
            first.item,
            first.variant,
            first.location,
            first.delta,
            first.after,
            first.reason,
            first.source,
            first.note.as_deref(),
            first.actor
        ),
        (
            item.id(),
            item.variant(),
            Some(location(1)),
            3,
            Some(3),
            StockReason::Counted,
            StockSource::Manual,
            Some("Counted twice"),
            staff
        )
    );
    assert!(first.at.unix_micros() > 0);
    let read: Vec<StockSource> = rows.iter().rev().skip(1).map(|r| r.source).collect();
    assert_eq!(read, sources);
    assert!(rows.windows(2).all(|w| w[0].position > w[1].position));
}

async fn lists<S>(ports: &S)
where
    S: HasBatch + HasInventoryItems + HasStockMovements,
{
    let items = ports.inventory_items();
    let mut a = new_item(1);
    let mut b = new_item(2);
    a.set_on_hand(location(1), 5, None, None).unwrap();
    b.set_on_hand(location(1), 1, None, None).unwrap();
    items.insert(&mut a).await.unwrap();
    items.insert(&mut b).await.unwrap();
    a.adjust_on_hand(location(2), 2, AdjustmentReason::Promotion, None)
        .unwrap();
    a.adjust_on_hand(location(1), -1, AdjustmentReason::Promotion, None)
        .unwrap();
    items.update(&mut a).await.unwrap();

    let movements = ports.stock_movements();
    let first = movements
        .list(a.id(), &MovementFilter::default(), page(None, 2))
        .await
        .unwrap();
    assert!(first.has_more);
    assert_eq!(
        first.items.iter().map(|r| r.delta).collect::<Vec<_>>(),
        [-1, 2]
    );
    let rest = movements
        .list(
            a.id(),
            &MovementFilter::default(),
            page(first.items.last().map(|r| r.position), 2),
        )
        .await
        .unwrap();
    assert!(!rest.has_more);
    assert_eq!(rest.items.iter().map(|r| r.delta).collect::<Vec<_>>(), [5]);

    let at_one = MovementFilter {
        location: Some(location(1)),
        ..MovementFilter::default()
    };
    assert_eq!(
        all(ports, a.id(), &at_one)
            .await
            .iter()
            .map(|r| r.delta)
            .collect::<Vec<_>>(),
        [-1, 5]
    );
    let promoted = MovementFilter {
        reason: Some(StockReason::Promotion),
        ..MovementFilter::default()
    };
    assert_eq!(all(ports, a.id(), &promoted).await.len(), 2);
    let both = MovementFilter {
        location: Some(location(2)),
        reason: Some(StockReason::Counted),
        ..MovementFilter::default()
    };
    assert_eq!(all(ports, a.id(), &both).await, []);

    let rows = all(ports, a.id(), &MovementFilter::default()).await;
    let at = rows[0].at;
    let since = MovementFilter {
        from: Some(at),
        ..MovementFilter::default()
    };
    let expected: Vec<_> = rows.iter().filter(|r| r.at >= at).cloned().collect();
    assert!(expected.len() >= 2, "the newest write's rows at least");
    assert_eq!(
        all(ports, a.id(), &since).await,
        expected,
        "from is inclusive"
    );
    let before = MovementFilter {
        to: Some(at),
        ..MovementFilter::default()
    };
    let expected: Vec<_> = rows.iter().filter(|r| r.at < at).cloned().collect();
    assert_eq!(
        all(ports, a.id(), &before).await,
        expected,
        "to is exclusive"
    );
    let epoch = MovementFilter {
        to: Some(Timestamp::from_unix_micros(0)),
        ..MovementFilter::default()
    };
    assert_eq!(all(ports, a.id(), &epoch).await, []);

    assert_eq!(
        all(ports, b.id(), &MovementFilter::default()).await.len(),
        1
    );
}
