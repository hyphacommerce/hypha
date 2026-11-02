//! What every transfer store must do: transfers kept whole with their lines, numbered
//! uniquely, listed newest first by filter, written in a batch with the stock they move,
//! deleted at their version, and what's in transit read back as incoming.

use hypha_core::{
    HasBatch, HasOutbox, InventoryItemId, LocationId, OutboxRepo, PageLimit, PageRequest, Quantity,
    RepoError, Stamped, Timestamp, TransferId, VariantId, Version, Versioned,
};
use hypha_inventory::{
    HasInventoryItems, HasTransfers, IncomingTransfer, InventoryItem, InventoryItemRepo,
    StageInventoryItems, StageTransfers, StockSource, Transfer, TransferArrival, TransferDraft,
    TransferFilter, TransferLineDraft, TransferNumber, TransferRepo, TransferStatus,
    incoming_transfers, repo::keys,
};
use uuid::Uuid;

use super::scenario;

fn tid(n: u128) -> TransferId {
    TransferId::from_uuid(Uuid::from_u128(n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(100 + n))
}

fn lid(n: u128) -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(200 + n))
}

fn at(secs: i64) -> Timestamp {
    Timestamp::from_unix_micros(secs * 1_000_000)
}

/// Transfer `n` (number `1000 + n`) from location 1 to 2, with `lines` (variant,
/// quantity).
fn transfer(n: u128, lines: &[(u128, u32)]) -> Transfer {
    Transfer::new(
        tid(n),
        TransferNumber::new(1000 + i64::try_from(n).unwrap()).unwrap(),
        TransferDraft {
            origin: lid(1),
            destination: lid(2),
            expected_at: None,
            reference: None,
            note: None,
            lines: lines
                .iter()
                .map(|&(v, quantity)| TransferLineDraft {
                    variant: vid(v),
                    quantity,
                })
                .collect(),
        },
    )
    .unwrap()
}

fn first_page(limit: u32) -> PageRequest<TransferId> {
    PageRequest {
        after: None,
        limit: PageLimit::new(limit).unwrap(),
    }
}

/// Runs every transfer scenario, each on fresh ports from `make`.
pub async fn transfer_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasTransfers
        + HasInventoryItems
        + HasOutbox
        + HasBatch<Batch: StageTransfers + StageInventoryItems>,
{
    scenario("transfer: every field survives a round trip; versions bump and go stale");
    round_trip(&make().await).await;
    scenario("transfer: numbers are handed out increasing and unique");
    numbers(&make().await).await;
    scenario("transfer: a list is newest first, filtered and paged");
    lists(&make().await).await;
    scenario("transfer: a batch writes it with the stock it moves, or neither");
    batches(&make().await).await;
    scenario("transfer: deleted only at its version");
    deletes(&make().await).await;
    scenario("transfer: what's in transit is incoming at the destination");
    incoming(&make().await).await;
}

async fn round_trip<S: HasTransfers + HasOutbox>(ports: &S) {
    let mut t = Transfer::new(
        tid(1),
        TransferNumber::new(1001).unwrap(),
        TransferDraft {
            origin: lid(1),
            destination: lid(2),
            expected_at: Some(Timestamp::from_unix_micros(1_767_225_600_000_001)),
            reference: Some("DHL 1234".to_string()),
            note: Some("Two boxes".to_string()),
            lines: vec![
                TransferLineDraft {
                    variant: vid(1),
                    quantity: 5,
                },
                TransferLineDraft {
                    variant: vid(2),
                    quantity: 3,
                },
            ],
        },
    )
    .unwrap();
    ports.transfers().insert(&mut t).await.unwrap();
    assert_eq!(t.version(), Version::new(1));
    let stored = ports.transfers().get(tid(1)).await.unwrap().unwrap();
    assert_eq!(stored, t);
    assert!(stored.stamps().is_some());
    assert_eq!(ports.transfers().get(tid(9)).await, Ok(None));

    // Shipped and partly received, every count and time is kept.
    let stale = t.clone();
    t.ship(at(10)).unwrap();
    t.receive(
        &[TransferArrival {
            variant: vid(1),
            accepted: 3,
            rejected: 1,
        }],
        at(20),
    )
    .unwrap();
    ports.transfers().update(&mut t).await.unwrap();
    assert_eq!(t.version(), Version::new(2));
    let stored = ports.transfers().get(tid(1)).await.unwrap().unwrap();
    assert_eq!(stored, t);
    assert_eq!(stored.status(), TransferStatus::InTransit);
    assert_eq!(stored.shipped_at(), Some(at(10)));
    assert_eq!(
        stored
            .lines()
            .iter()
            .map(|l| (l.shipped(), l.received(), l.rejected()))
            .collect::<Vec<_>>(),
        [(5, 3, 1), (3, 0, 0)]
    );

    let mut stale = stale;
    stale.cancel(at(30)).unwrap();
    assert_eq!(
        ports.transfers().update(&mut stale).await,
        Err(RepoError::StaleVersion)
    );
    assert_eq!(ports.transfers().get(tid(1)).await, Ok(Some(t)));

    let types: Vec<String> = ports
        .outbox()
        .events_after(0, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert_eq!(
        types,
        ["transfer.created", "transfer.shipped", "transfer.received"]
    );
}

async fn numbers<S: HasTransfers>(ports: &S) {
    let first = ports.transfers().next_number().await.unwrap();
    let second = ports.transfers().next_number().await.unwrap();
    assert!(second > first, "{second} after {first}");

    let mut t = transfer(1, &[(1, 1)]);
    ports.transfers().insert(&mut t).await.unwrap();
    let mut same_number = Transfer::new(tid(2), t.number(), t.draft()).unwrap();
    assert_eq!(
        ports.transfers().insert(&mut same_number).await,
        Err(RepoError::Duplicate(keys::TRANSFER_NUMBER))
    );
    let mut same_id = Transfer::new(tid(1), TransferNumber::new(5000).unwrap(), t.draft()).unwrap();
    assert_eq!(
        ports.transfers().insert(&mut same_id).await,
        Err(RepoError::Duplicate(keys::TRANSFER_ID))
    );
}

async fn lists<S: HasTransfers>(ports: &S) {
    for (n, lines) in [
        (1, &[(1, 2)][..]),
        (2, &[(2, 1)][..]),
        (3, &[(1, 1), (2, 1)][..]),
    ] {
        let mut t = transfer(n, lines);
        ports.transfers().insert(&mut t).await.unwrap();
    }
    let mut shipped = ports.transfers().get(tid(2)).await.unwrap().unwrap();
    shipped.ship(at(5)).unwrap();
    ports.transfers().update(&mut shipped).await.unwrap();

    let ids = |page: hypha_core::Page<hypha_inventory::TransferSummary>| {
        page.items.iter().map(|s| s.id).collect::<Vec<_>>()
    };
    let all = ports
        .transfers()
        .list(&TransferFilter::default(), first_page(10))
        .await
        .unwrap();
    assert_eq!(ids(all.clone()), [tid(3), tid(2), tid(1)]);
    let summary = &all.items[2];
    assert_eq!(
        (
            summary.number,
            summary.origin,
            summary.destination,
            summary.status,
            summary.line_count,
            summary.units
        ),
        (
            TransferNumber::new(1001).unwrap(),
            lid(1),
            lid(2),
            TransferStatus::Draft,
            1,
            2
        )
    );
    assert!(summary.created_at.is_some());
    assert_eq!(all.items[1].shipped_at, Some(at(5)));

    let filtered = |filter: TransferFilter| async move {
        ids(ports
            .transfers()
            .list(&filter, first_page(10))
            .await
            .unwrap())
    };
    assert_eq!(
        filtered(TransferFilter {
            status: Some(TransferStatus::InTransit),
            ..TransferFilter::default()
        })
        .await,
        [tid(2)]
    );
    assert_eq!(
        filtered(TransferFilter {
            variant: Some(vid(1)),
            ..TransferFilter::default()
        })
        .await,
        [tid(3), tid(1)]
    );
    assert_eq!(
        filtered(TransferFilter {
            origin: Some(lid(2)),
            ..TransferFilter::default()
        })
        .await,
        Vec::<TransferId>::new()
    );
    assert_eq!(
        filtered(TransferFilter {
            destination: Some(lid(2)),
            ..TransferFilter::default()
        })
        .await,
        [tid(3), tid(2), tid(1)]
    );

    let first = ports
        .transfers()
        .list(&TransferFilter::default(), first_page(2))
        .await
        .unwrap();
    assert!(first.has_more);
    let rest = ports
        .transfers()
        .list(
            &TransferFilter::default(),
            PageRequest {
                after: Some(tid(2)),
                limit: PageLimit::new(2).unwrap(),
            },
        )
        .await
        .unwrap();
    assert_eq!(ids(rest), [tid(1)]);
}

async fn batches<S>(ports: &S)
where
    S: HasTransfers + HasInventoryItems + HasBatch<Batch: StageTransfers + StageInventoryItems>,
{
    let mut item = InventoryItem::new(
        InventoryItemId::from_uuid(Uuid::from_u128(70)),
        vid(1),
        true,
        false,
    );
    item.set_on_hand(lid(1), 10, None, None).unwrap();
    ports.inventory_items().insert(&mut item).await.unwrap();
    let mut t = transfer(1, &[(1, 4)]);
    ports.transfers().insert(&mut t).await.unwrap();
    let stale = t.clone();

    t.ship(at(5)).unwrap();
    item.transfer_out(
        lid(1),
        Quantity::new(4).unwrap(),
        StockSource::Transfer(tid(1)),
    )
    .unwrap();
    let mut batch = ports.batch();
    batch.update_transfer(&mut t);
    batch.update_inventory_item(&mut item);
    ports.commit(batch).await.unwrap();
    assert_eq!(t.version(), Version::new(2));
    assert_eq!(ports.transfers().get(tid(1)).await, Ok(Some(t)));
    let stored = ports
        .inventory_items()
        .get(item.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.on_hand(lid(1)), 6);

    // A stale transfer fails the batch: the item's change isn't written either.
    let mut stale = stale;
    stale.ship(at(6)).unwrap();
    let mut item = stored;
    item.transfer_out(
        lid(1),
        Quantity::new(4).unwrap(),
        StockSource::Transfer(tid(1)),
    )
    .unwrap();
    let mut batch = ports.batch();
    batch.update_transfer(&mut stale);
    batch.update_inventory_item(&mut item);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    let unchanged = ports
        .inventory_items()
        .get(item.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.on_hand(lid(1)), 6);
}

async fn deletes<S: HasTransfers>(ports: &S) {
    let mut t = transfer(1, &[(1, 1)]);
    ports.transfers().insert(&mut t).await.unwrap();
    let mut stale = t.clone();
    let mut d = t.draft();
    d.note = Some("changed".to_string());
    t.update(d).unwrap();
    ports.transfers().update(&mut t).await.unwrap();
    assert_eq!(
        ports.transfers().delete(&mut stale).await,
        Err(RepoError::StaleVersion)
    );
    ports.transfers().delete(&mut t).await.unwrap();
    assert_eq!(ports.transfers().get(tid(1)).await, Ok(None));
    assert_eq!(
        ports.transfers().delete(&mut t).await,
        Err(RepoError::StaleVersion)
    );
}

async fn incoming<S: HasTransfers>(ports: &S) {
    let mut sooner = transfer(1, &[(1, 5), (2, 2)]);
    let mut d = sooner.draft();
    d.expected_at = Some(at(100));
    sooner.update(d).unwrap();
    let mut later = transfer(2, &[(1, 3)]);
    let mut d = later.draft();
    d.expected_at = Some(at(200));
    later.update(d).unwrap();
    let mut draft = transfer(3, &[(1, 9)]);
    for t in [&mut sooner, &mut later, &mut draft] {
        ports.transfers().insert(t).await.unwrap();
    }
    for t in [&mut sooner, &mut later] {
        t.ship(at(10)).unwrap();
    }
    sooner
        .receive(
            &[TransferArrival {
                variant: vid(1),
                accepted: 1,
                rejected: 1,
            }],
            at(20),
        )
        .unwrap();
    for t in [&mut sooner, &mut later] {
        ports.transfers().update(t).await.unwrap();
    }

    let found = ports
        .transfers()
        .incoming(&[vid(1), vid(2), vid(3)])
        .await
        .unwrap();
    let all = [sooner, later, draft];
    assert_eq!(
        found,
        incoming_transfers(all.iter(), &[vid(1), vid(2), vid(3)])
    );
    assert_eq!(
        found,
        [
            IncomingTransfer {
                variant: vid(1),
                location: lid(2),
                quantity: 3 + 3,
                next_expected_at: Some(at(100)),
            },
            IncomingTransfer {
                variant: vid(2),
                location: lid(2),
                quantity: 2,
                next_expected_at: Some(at(100)),
            },
        ]
    );
    assert_eq!(ports.transfers().incoming(&[]).await, Ok(Vec::new()));
}
