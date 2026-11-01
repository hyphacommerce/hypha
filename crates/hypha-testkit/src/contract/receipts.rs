//! What every receipt store must do: receipts written in a batch with their order and the
//! stock they brought (new items included), read back whole, listed per order, and taken
//! back once.

use hypha_core::{
    Currency, ExchangeRate, HasBatch, HasOutbox, InventoryItemId, LocationId, OutboxRepo,
    PageLimit, PageRequest, PurchaseOrderId, Quantity, ReceiptId, RepoError, Stamped, SupplierId,
    Timestamp, UnitCost, VariantId, Version, Versioned,
};
use hypha_inventory::{
    HasInventoryItems, HasStockMovements, InventoryItem, InventoryItemRepo, MovementFilter,
    StageInventoryItems, StockMovementRepo, StockReason, StockSource, repo::keys as inventory_keys,
};
use hypha_purchasing::{
    HasPurchaseOrders, HasReceipts, HasSuppliers, LineDraft, NewReceipt, PurchaseOrder,
    PurchaseOrderDraft, PurchaseOrderNumber, PurchaseOrderRepo, Receipt, ReceiptLine, ReceiptRepo,
    ReceiptState, StagePurchaseOrders, StageReceipts, Supplier, SupplierDraft, SupplierRepo, keys,
};
use uuid::Uuid;

use super::scenario;

fn poid(n: u128) -> PurchaseOrderId {
    PurchaseOrderId::from_uuid(Uuid::from_u128(n))
}

fn rid(n: u128) -> ReceiptId {
    ReceiptId::from_uuid(Uuid::from_u128(n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn lid(n: u128) -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(n))
}

fn eur(minor: &str) -> UnitCost {
    UnitCost::parse(minor, Currency::EUR).unwrap()
}

fn at(secs: i64) -> Timestamp {
    Timestamp::from_unix_micros(secs * 1_000_000)
}

/// Supplier 1 and order `n` (number `1000 + n`) for variants 10 and 11, ordered.
async fn ordered<S: HasSuppliers + HasPurchaseOrders>(ports: &S, n: u128) -> PurchaseOrder {
    let supplier = SupplierId::from_uuid(Uuid::from_u128(1));
    if ports.suppliers().get(supplier).await.unwrap().is_none() {
        let mut s =
            Supplier::new(supplier, SupplierDraft::new("Spore Labs", Currency::EUR)).unwrap();
        ports.suppliers().insert(&mut s).await.unwrap();
    }
    let lines = [10, 11]
        .into_iter()
        .map(|v| LineDraft {
            variant: vid(v),
            product_title: "Oyster grow kit".to_string(),
            variant_title: None,
            sku: None,
            supplier_sku: None,
            quantity: 5,
            unit_cost: eur("500"),
        })
        .collect();
    let mut po = PurchaseOrder::new(
        poid(n),
        PurchaseOrderNumber::new(1000 + i64::try_from(n).unwrap()).unwrap(),
        supplier,
        PurchaseOrderDraft {
            destination: Some(lid(50)),
            lines,
            ..PurchaseOrderDraft::new(Currency::EUR)
        },
    )
    .unwrap();
    po.order(Some(Currency::EUR), "Spore Labs", None, at(1))
        .unwrap();
    ports.purchase_orders().insert(&mut po).await.unwrap();
    po
}

fn receipt(id: u128, po: u128, lines: &[(u128, u32, u32)]) -> Receipt {
    Receipt::new(NewReceipt {
        id: rid(id),
        purchase_order: poid(po),
        location: lid(50),
        lines: lines
            .iter()
            .map(|&(v, accepted, rejected)| ReceiptLine {
                variant: vid(v),
                accepted,
                rejected,
                landed_unit_cost: Some(eur("512.3456")),
            })
            .collect(),
        exchange_rate: Some(ExchangeRate::parse("1.1723").unwrap()),
        note: Some("pallet 1 of 2".to_string()),
    })
    .unwrap()
}

/// Runs every receipt scenario, each on fresh ports from `make`.
pub async fn receipt_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasSuppliers
        + HasPurchaseOrders
        + HasReceipts
        + HasInventoryItems
        + HasStockMovements
        + HasOutbox
        + HasBatch<Batch: StagePurchaseOrders + StageReceipts + StageInventoryItems>,
{
    scenario("receipt: a batch writes it with its order and new stock, every field kept");
    round_trip(&make().await).await;
    scenario("receipt: an unknown order or a used ID fails the batch, writing nothing");
    refusals(&make().await).await;
    scenario("receipt: an order's receipts are listed oldest first, only its own");
    per_order(&make().await).await;
    scenario("receipt: taking it back bumps its version; a stale copy fails");
    cancels(&make().await).await;
    scenario("receipt: a batch's new item for a variant that has one fails it");
    one_item_per_variant(&make().await).await;
}

async fn round_trip<S>(ports: &S)
where
    S: HasSuppliers
        + HasPurchaseOrders
        + HasReceipts
        + HasInventoryItems
        + HasStockMovements
        + HasOutbox
        + HasBatch<Batch: StagePurchaseOrders + StageReceipts + StageInventoryItems>,
{
    let mut po = ordered(ports, 1).await;
    let mut r = receipt(1, 1, &[(10, 4, 1), (11, 0, 2)]);
    po.record_receipt(&r.quantities(), false, at(2)).unwrap();
    let mut item = InventoryItem::new(
        InventoryItemId::from_uuid(Uuid::from_u128(70)),
        vid(10),
        false,
        false,
    );
    let source = StockSource::Receipt {
        purchase_order: poid(1),
        receipt: rid(1),
    };
    item.receive(
        lid(50),
        Quantity::new(4).unwrap(),
        Some(eur("512.3456")),
        source,
    )
    .unwrap();
    let mut batch = ports.batch();
    batch.update_purchase_order(&mut po);
    batch.insert_receipt(&mut r);
    batch.insert_inventory_item(&mut item);
    ports.commit(batch).await.unwrap();

    assert_eq!(r.version(), Version::new(1));
    let stored = ports.receipts().get(rid(1)).await.unwrap().unwrap();
    assert_eq!(stored, r);
    assert!(stored.stamps().is_some());
    assert_eq!(stored.note(), Some("pallet 1 of 2"));
    assert_eq!(ports.receipts().get(rid(9)).await, Ok(None));
    assert_eq!(ports.purchase_orders().get(poid(1)).await, Ok(Some(po)));

    // The new item, with its cost and its first row.
    assert_eq!(item.version(), Version::new(1));
    let stored_item = ports
        .inventory_items()
        .get(item.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored_item, item);
    assert_eq!(stored_item.unit_cost(), Some(eur("512.3456")));
    let rows = ports
        .stock_movements()
        .list(
            item.id(),
            &MovementFilter::default(),
            PageRequest {
                after: None,
                limit: PageLimit::new(10).unwrap(),
            },
        )
        .await
        .unwrap()
        .items;
    assert_eq!(
        rows.iter()
            .map(|r| (r.reason, r.delta, r.source, r.unit_cost))
            .collect::<Vec<_>>(),
        [(StockReason::Received, 4, source, Some(eur("512.3456")))]
    );

    let types: Vec<String> = ports
        .outbox()
        .events_after(0, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert!(types.iter().any(|t| t == "receipt.created"), "{types:?}");
    assert!(
        types.iter().any(|t| t == "inventory_item.cost_changed"),
        "{types:?}"
    );
}

async fn refusals<S>(ports: &S)
where
    S: HasSuppliers
        + HasPurchaseOrders
        + HasReceipts
        + HasBatch<Batch: StagePurchaseOrders + StageReceipts>,
{
    ordered(ports, 1).await;
    let mut orphan = receipt(1, 9, &[(10, 1, 0)]);
    let mut batch = ports.batch();
    batch.insert_receipt(&mut orphan);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Reference(keys::RECEIPT_PURCHASE_ORDER))
    );
    assert_eq!(ports.receipts().get(rid(1)).await, Ok(None));

    let mut first = receipt(1, 1, &[(10, 1, 0)]);
    let mut batch = ports.batch();
    batch.insert_receipt(&mut first);
    ports.commit(batch).await.unwrap();
    // The same ID again, with an order change that must not be written either.
    let mut po = ports.purchase_orders().get(poid(1)).await.unwrap().unwrap();
    let before = po.clone();
    po.record_receipt(&[first.quantities()[0]], false, at(3))
        .unwrap();
    let mut again = receipt(1, 1, &[(11, 1, 0)]);
    let mut batch = ports.batch();
    batch.update_purchase_order(&mut po);
    batch.insert_receipt(&mut again);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::RECEIPT_ID))
    );
    assert_eq!(ports.receipts().get(rid(1)).await, Ok(Some(first)));
    assert_eq!(ports.purchase_orders().get(poid(1)).await, Ok(Some(before)));
}

async fn per_order<S>(ports: &S)
where
    S: HasSuppliers + HasPurchaseOrders + HasReceipts + HasBatch<Batch: StageReceipts>,
{
    ordered(ports, 1).await;
    ordered(ports, 2).await;
    let mut written = Vec::new();
    for (id, po) in [(3, 1), (1, 1), (2, 2), (4, 1)] {
        let mut r = receipt(id, po, &[(10, 1, 0)]);
        let mut batch = ports.batch();
        batch.insert_receipt(&mut r);
        ports.commit(batch).await.unwrap();
        written.push(r);
    }
    let ids = |rs: Vec<Receipt>| rs.iter().map(Receipt::id).collect::<Vec<_>>();
    assert_eq!(
        ids(ports.receipts().for_purchase_order(poid(1)).await.unwrap()),
        [rid(1), rid(3), rid(4)]
    );
    assert_eq!(
        ids(ports.receipts().for_purchase_order(poid(2)).await.unwrap()),
        [rid(2)]
    );
    assert_eq!(
        ports.receipts().for_purchase_order(poid(9)).await,
        Ok(Vec::new())
    );
}

async fn cancels<S>(ports: &S)
where
    S: HasSuppliers + HasPurchaseOrders + HasReceipts + HasOutbox + HasBatch<Batch: StageReceipts>,
{
    ordered(ports, 1).await;
    let mut r = receipt(1, 1, &[(10, 1, 0)]);
    let mut batch = ports.batch();
    batch.insert_receipt(&mut r);
    ports.commit(batch).await.unwrap();
    let stale = r.clone();

    r.cancel(at(5)).unwrap();
    let mut batch = ports.batch();
    batch.update_receipt(&mut r);
    ports.commit(batch).await.unwrap();
    assert_eq!(r.version(), Version::new(2));
    let stored = ports.receipts().get(rid(1)).await.unwrap().unwrap();
    assert_eq!(stored, r);
    assert_eq!(stored.state(), ReceiptState::Cancelled { at: at(5) });
    let cancelled = ports
        .outbox()
        .events_after(0, 100)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.event_type == "receipt.cancelled")
        .count();
    assert_eq!(cancelled, 1);

    let mut stale = stale;
    stale.cancel(at(6)).unwrap();
    let mut batch = ports.batch();
    batch.update_receipt(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.receipts().get(rid(1)).await, Ok(Some(r)));
}

async fn one_item_per_variant<S>(ports: &S)
where
    S: HasInventoryItems + HasBatch<Batch: StageInventoryItems>,
{
    let mut first = InventoryItem::new(
        InventoryItemId::from_uuid(Uuid::from_u128(70)),
        vid(10),
        true,
        false,
    );
    ports.inventory_items().insert(&mut first).await.unwrap();
    let mut second = InventoryItem::new(
        InventoryItemId::from_uuid(Uuid::from_u128(71)),
        vid(10),
        false,
        false,
    );
    let mut batch = ports.batch();
    batch.insert_inventory_item(&mut second);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(inventory_keys::INVENTORY_ITEM_VARIANT))
    );
    assert_eq!(ports.inventory_items().get(second.id()).await, Ok(None));
}
