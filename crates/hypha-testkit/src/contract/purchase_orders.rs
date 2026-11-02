//! What every purchase order store must do: orders with every field, their numbers, lists
//! by filter and sort, the supplier link, and batched writes.

use hypha_core::{
    Currency, ExchangeRate, HasBatch, HasOutbox, Keyset, LocationId, Money, OutboxRepo, PageLimit,
    PageRequest, PurchaseOrderId, RepoError, Stamped, SupplierId, Timestamp, UnitCost, VariantId,
    Version, Versioned,
};
use hypha_purchasing::{
    CostAllocation, CostDraft, CostKind, HasPurchaseOrders, HasSuppliers, LineDraft, PaymentDue,
    PaymentTermsDraft, PurchaseOrder, PurchaseOrderDraft, PurchaseOrderFilter, PurchaseOrderNumber,
    PurchaseOrderRepo, PurchaseOrderSort, PurchaseOrderStatus, ReceiptStatus, ReceivedQuantities,
    StagePurchaseOrders, Supplier, SupplierDraft, SupplierRepo, keys,
};
use uuid::Uuid;

use super::scenario;

fn poid(n: u128) -> PurchaseOrderId {
    PurchaseOrderId::from_uuid(Uuid::from_u128(n))
}

fn sid(n: u128) -> SupplierId {
    SupplierId::from_uuid(Uuid::from_u128(n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn lid(n: u128) -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(n))
}

fn at(secs: i64) -> Timestamp {
    Timestamp::from_unix_micros(secs * 1_000_000)
}

fn eur(minor: &str) -> UnitCost {
    UnitCost::parse(minor, Currency::EUR).unwrap()
}

fn line(variant: u128, quantity: u32, cost: &str) -> LineDraft {
    LineDraft {
        variant: vid(variant),
        product_title: "Oyster grow kit".to_string(),
        variant_title: Some("Large".to_string()),
        sku: Some(format!("GK-{variant}")),
        supplier_sku: Some(format!("SL-{variant}")),
        quantity,
        unit_cost: eur(cost),
    }
}

fn draft(lines: Vec<LineDraft>) -> PurchaseOrderDraft {
    PurchaseOrderDraft {
        destination: Some(lid(50)),
        lines,
        ..PurchaseOrderDraft::new(Currency::EUR)
    }
}

fn po(n: u128, number: i64, supplier: u128, lines: Vec<LineDraft>) -> PurchaseOrder {
    PurchaseOrder::new(
        poid(n),
        PurchaseOrderNumber::new(number).unwrap(),
        sid(supplier),
        draft(lines),
    )
    .unwrap()
}

/// Every field set: rate, terms, dates, notes, costs in two currencies, tax; ordered,
/// partly received.
fn full(n: u128, number: i64, supplier: u128) -> PurchaseOrder {
    let mut d = draft(vec![line(10, 5, "100.5"), line(11, 2, "0.35")]);
    d.currency = Currency::USD;
    for l in &mut d.lines {
        l.unit_cost = UnitCost::parse(&l.unit_cost.minor_units(), Currency::USD).unwrap();
    }
    d.exchange_rate = Some(ExchangeRate::parse("0.9213").unwrap());
    d.payment_terms = Some(PaymentTermsDraft {
        due: PaymentDue::Net { days: 30 },
        note: Some("2% off within 10 days".to_string()),
    });
    d.expected_at = Some(Timestamp::from_unix_micros(1_767_225_600_000_001));
    d.supplier_reference = Some("SO-778".to_string());
    d.note_to_supplier = Some("Deliver to the back door.".to_string());
    d.note = Some("Rush for the spring sale.".to_string());
    d.costs = vec![
        CostDraft {
            kind: CostKind::Shipping,
            description: Some("DHL".to_string()),
            amount: Money::new(2500, Currency::USD).unwrap(),
            allocation: CostAllocation::ByValue,
        },
        CostDraft {
            kind: CostKind::Duties,
            description: None,
            amount: Money::new(1200, Currency::EUR).unwrap(),
            allocation: CostAllocation::ByQuantity,
        },
        CostDraft {
            kind: CostKind::Fees,
            description: None,
            amount: Money::new(300, Currency::USD).unwrap(),
            allocation: CostAllocation::None,
        },
    ];
    d.tax = Some(Money::new(410, Currency::USD).unwrap());
    let mut po = PurchaseOrder::new(
        poid(n),
        PurchaseOrderNumber::new(number).unwrap(),
        sid(supplier),
        d,
    )
    .unwrap();
    po.order(Some(Currency::EUR), "Spore Labs", Some(14), at(1_000))
        .unwrap();
    po.record_receipt(
        &[ReceivedQuantities {
            variant: vid(10),
            accepted: 2,
            rejected: 1,
        }],
        false,
        at(2_000),
    )
    .unwrap();
    // A negative value: an extra cost lowered after the goods arrived.
    po.add_extra_applied(vid(10), 2, -12_345).unwrap();
    po
}

fn page(
    limit: u32,
    after: Option<Keyset<PurchaseOrderId>>,
) -> PageRequest<Keyset<PurchaseOrderId>> {
    PageRequest {
        limit: PageLimit::new(limit).unwrap(),
        after,
    }
}

async fn with_suppliers<S: HasSuppliers>(ports: &S, ns: &[u128]) {
    for &n in ns {
        let mut s = Supplier::new(sid(n), SupplierDraft::new("Spore Labs", Currency::EUR)).unwrap();
        ports.suppliers().insert(&mut s).await.unwrap();
    }
}

/// Runs every purchase order scenario, each on fresh ports from `make`.
pub async fn purchase_order_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasSuppliers + HasPurchaseOrders + HasOutbox + HasBatch<Batch: StagePurchaseOrders>,
{
    scenario("purchase order: every field survives a round trip, at version 1, stamped");
    round_trip(&make().await).await;
    scenario("purchase order: an unknown supplier, a used ID or number is refused");
    refusals(&make().await).await;
    scenario("purchase order: numbers are handed out increasing, never twice");
    numbers(&make().await).await;
    scenario("purchase order: update bumps the version, replaces lines, writes events");
    updates(&make().await).await;
    scenario("purchase order: delete removes it; a supplier on an order isn't deleted");
    deletes(&make().await).await;
    scenario("purchase order: lists filter by supplier, status, receipts, place, variant");
    filters(&make().await).await;
    scenario("purchase order: lists sort by number either way and by expected date, paged");
    sorts(&make().await).await;
    scenario("purchase order: a variant's last line from a supplier skips cancelled orders");
    last_line(&make().await).await;
    scenario("purchase order: incoming sums what ordered orders still bring, per place");
    incoming(&make().await).await;
    scenario("purchase order: the store's currency is named once one is ordered or rated");
    store_currency(&make().await).await;
    scenario("purchase order: a batch writes it with its events; a stale copy fails it");
    batches(&make().await).await;
}

async fn round_trip<S: HasSuppliers + HasPurchaseOrders>(ports: &S) {
    with_suppliers(ports, &[1]).await;
    let repo = ports.purchase_orders();
    let mut every = full(1, 1001, 1);
    repo.insert(&mut every).await.unwrap();
    assert_eq!(every.version(), Version::new(1));
    let stored = repo.get(poid(1)).await.unwrap().unwrap();
    assert_eq!(stored, every);
    assert!(stored.stamps().is_some());
    assert_eq!(stored.supplier_name(), Some("Spore Labs"));
    assert_eq!(stored.receipt_status(), ReceiptStatus::Partial);
    assert_eq!(stored.lines()[0].rejected(), 1);
    assert_eq!(stored.lines()[0].extra_applied(), Some(-24_690));
    assert_eq!(stored.lines()[1].extra_applied(), Some(0));

    // A bare draft: no destination, lines or anything optional.
    let mut bare = PurchaseOrder::new(
        poid(2),
        PurchaseOrderNumber::new(1002).unwrap(),
        sid(1),
        PurchaseOrderDraft::new(Currency::EUR),
    )
    .unwrap();
    repo.insert(&mut bare).await.unwrap();
    assert_eq!(repo.get(poid(2)).await, Ok(Some(bare)));
    assert_eq!(repo.get(poid(9)).await, Ok(None));

    // Closed and cancelled, with their times.
    let mut closed = full(3, 1003, 1);
    closed.close(at(3_000)).unwrap();
    repo.insert(&mut closed).await.unwrap();
    assert_eq!(repo.get(poid(3)).await, Ok(Some(closed)));
    let mut cancelled = po(4, 1004, 1, vec![line(10, 1, "1")]);
    cancelled.cancel(at(4_000)).unwrap();
    repo.insert(&mut cancelled).await.unwrap();
    assert_eq!(repo.get(poid(4)).await, Ok(Some(cancelled)));

    // A line received before its extra value was kept has none.
    let mut snapshot = full(5, 1005, 1).snapshot();
    snapshot.lines[0].extra_applied = None;
    let mut unkept = PurchaseOrder::restore(snapshot).unwrap();
    repo.insert(&mut unkept).await.unwrap();
    let stored = repo.get(poid(5)).await.unwrap().unwrap();
    assert_eq!(stored.lines()[0].extra_applied(), None);

    // Sent at an earlier revision than it's at now.
    let mut snapshot = full(6, 1006, 1).snapshot();
    snapshot.revision = 3;
    snapshot.sent_at = Some(at(6_000));
    snapshot.sent_revision = Some(2);
    let mut sent = PurchaseOrder::restore(snapshot).unwrap();
    repo.insert(&mut sent).await.unwrap();
    let stored = repo.get(poid(6)).await.unwrap().unwrap();
    assert_eq!((stored.revision(), stored.sent_revision()), (3, Some(2)));
    assert_eq!(stored, sent);
}

async fn refusals<S: HasSuppliers + HasPurchaseOrders>(ports: &S) {
    with_suppliers(ports, &[1]).await;
    let repo = ports.purchase_orders();
    assert_eq!(
        repo.insert(&mut po(1, 1001, 9, vec![])).await,
        Err(RepoError::Reference(keys::PURCHASE_ORDER_SUPPLIER))
    );
    repo.insert(&mut po(1, 1001, 1, vec![])).await.unwrap();
    assert_eq!(
        repo.insert(&mut po(1, 1002, 1, vec![])).await,
        Err(RepoError::Duplicate(keys::PURCHASE_ORDER_ID))
    );
    assert_eq!(
        repo.insert(&mut po(2, 1001, 1, vec![])).await,
        Err(RepoError::Duplicate(keys::PURCHASE_ORDER_NUMBER))
    );
}

async fn numbers<S: HasPurchaseOrders>(ports: &S) {
    let repo = ports.purchase_orders();
    let first = repo.next_number().await.unwrap();
    let second = repo.next_number().await.unwrap();
    assert!(second > first, "{second} after {first}");
}

async fn updates<S: HasSuppliers + HasPurchaseOrders + HasOutbox>(ports: &S) {
    with_suppliers(ports, &[1]).await;
    let repo = ports.purchase_orders();
    let mut order = po(1, 1001, 1, vec![line(10, 5, "100"), line(11, 1, "1")]);
    repo.insert(&mut order).await.unwrap();
    let stale = order.clone();

    // Fewer lines, then more, then ordered: each write as the order has it.
    let mut d = order.draft();
    d.lines.truncate(1);
    d.lines[0].quantity = 7;
    order.update(d, None, at(10)).unwrap();
    repo.update(&mut order).await.unwrap();
    assert_eq!(order.version(), Version::new(2));
    assert_eq!(repo.get(poid(1)).await, Ok(Some(order.clone())));
    let mut d = order.draft();
    d.lines.push(line(12, 3, "2.5"));
    d.lines.push(line(13, 4, "0"));
    order.update(d, None, at(11)).unwrap();
    repo.update(&mut order).await.unwrap();
    assert_eq!(repo.get(poid(1)).await, Ok(Some(order.clone())));
    order
        .order(Some(Currency::EUR), "Spore Labs", None, at(12))
        .unwrap();
    repo.update(&mut order).await.unwrap();
    assert_eq!(repo.get(poid(1)).await, Ok(Some(order)));

    let mut stale = stale;
    stale.cancel(at(13)).unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));

    let types: Vec<String> = ports
        .outbox()
        .events_after(0, 100)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.event_type.starts_with("purchase_order."))
        .map(|e| e.event_type)
        .collect();
    assert_eq!(
        types,
        [
            "purchase_order.created",
            "purchase_order.updated",
            "purchase_order.updated",
            "purchase_order.ordered",
        ]
    );
}

async fn store_currency<S: HasSuppliers + HasPurchaseOrders>(ports: &S) {
    with_suppliers(ports, &[1]).await;
    let repo = ports.purchase_orders();
    assert_eq!(repo.store_currency_named().await, Ok(false), "no orders");
    let mut plain = po(1, 1001, 1, vec![line(10, 1, "1")]);
    repo.insert(&mut plain).await.unwrap();
    assert_eq!(
        repo.store_currency_named().await,
        Ok(false),
        "a draft at no rate"
    );

    let rated = |n: u128, number: i64| {
        let mut d = draft(vec![line(10, 1, "1")]);
        d.exchange_rate = Some(ExchangeRate::parse("1.17").unwrap());
        PurchaseOrder::new(
            poid(n),
            PurchaseOrderNumber::new(number).unwrap(),
            sid(1),
            d,
        )
        .unwrap()
    };
    let mut cancelled = rated(2, 1002);
    cancelled.cancel(at(10)).unwrap();
    repo.insert(&mut cancelled).await.unwrap();
    assert_eq!(
        repo.store_currency_named().await,
        Ok(false),
        "a cancelled one"
    );

    let mut draft_at_rate = rated(3, 1003);
    repo.insert(&mut draft_at_rate).await.unwrap();
    assert_eq!(
        repo.store_currency_named().await,
        Ok(true),
        "a draft at a rate"
    );
    repo.delete(&mut draft_at_rate).await.unwrap();
    assert_eq!(repo.store_currency_named().await, Ok(false));

    plain
        .order(Some(Currency::EUR), "Spore Labs", None, at(20))
        .unwrap();
    repo.update(&mut plain).await.unwrap();
    assert_eq!(
        repo.store_currency_named().await,
        Ok(true),
        "an ordered one"
    );
}

async fn deletes<S: HasSuppliers + HasPurchaseOrders>(ports: &S) {
    with_suppliers(ports, &[1]).await;
    let repo = ports.purchase_orders();
    let mut order = po(1, 1001, 1, vec![line(10, 1, "1")]);
    repo.insert(&mut order).await.unwrap();
    let mut supplier = ports.suppliers().get(sid(1)).await.unwrap().unwrap();
    supplier.delete();
    assert_eq!(
        ports.suppliers().delete(&mut supplier.clone()).await,
        Err(RepoError::Reference(keys::SUPPLIER_IN_USE))
    );

    let mut stale = order.clone();
    let mut d = order.draft();
    d.note = Some("changed".to_string());
    order.update(d, None, at(1)).unwrap();
    repo.update(&mut order).await.unwrap();
    assert_eq!(repo.delete(&mut stale).await, Err(RepoError::StaleVersion));
    repo.delete(&mut order).await.unwrap();
    assert_eq!(repo.get(poid(1)).await, Ok(None));

    // With no order naming it, the supplier goes.
    ports.suppliers().delete(&mut supplier).await.unwrap();
}

async fn filters<S: HasSuppliers + HasPurchaseOrders>(ports: &S) {
    with_suppliers(ports, &[1, 2]).await;
    let repo = ports.purchase_orders();
    let mut a = po(1, 1001, 1, vec![line(10, 1, "1")]);
    let mut b = full(2, 1002, 2);
    let mut c = po(3, 1003, 1, vec![line(11, 1, "1")]);
    let mut d = c.draft();
    d.destination = Some(lid(60));
    c.update(d, None, at(1)).unwrap();
    for order in [&mut a, &mut b, &mut c] {
        repo.insert(order).await.unwrap();
    }
    let found = async |filter: PurchaseOrderFilter| -> Vec<i64> {
        repo.list(&filter, PurchaseOrderSort::NumberAsc, page(10, None))
            .await
            .unwrap()
            .items
            .iter()
            .map(|s| s.number.value())
            .collect()
    };
    assert_eq!(
        found(PurchaseOrderFilter::default()).await,
        [1001, 1002, 1003]
    );
    let by_supplier = PurchaseOrderFilter {
        supplier: Some(sid(1)),
        ..PurchaseOrderFilter::default()
    };
    assert_eq!(found(by_supplier).await, [1001, 1003]);
    let ordered = PurchaseOrderFilter {
        status: Some(PurchaseOrderStatus::Ordered),
        ..PurchaseOrderFilter::default()
    };
    assert_eq!(found(ordered).await, [1002]);
    let partial = PurchaseOrderFilter {
        receipt_status: Some(ReceiptStatus::Partial),
        ..PurchaseOrderFilter::default()
    };
    assert_eq!(found(partial).await, [1002]);
    let to_60 = PurchaseOrderFilter {
        destination: Some(lid(60)),
        ..PurchaseOrderFilter::default()
    };
    assert_eq!(found(to_60).await, [1003]);
    let with_10 = PurchaseOrderFilter {
        variant: Some(vid(10)),
        ..PurchaseOrderFilter::default()
    };
    assert_eq!(found(with_10).await, [1001, 1002]);

    // A summary is the stored order's.
    let listed = repo
        .list(
            &PurchaseOrderFilter {
                supplier: Some(sid(2)),
                ..PurchaseOrderFilter::default()
            },
            PurchaseOrderSort::NumberDesc,
            page(10, None),
        )
        .await
        .unwrap();
    assert_eq!(
        listed.items,
        [repo.get(poid(2)).await.unwrap().unwrap().summary()]
    );
}

async fn sorts<S: HasSuppliers + HasPurchaseOrders>(ports: &S) {
    with_suppliers(ports, &[1]).await;
    let repo = ports.purchase_orders();
    // Expected: 1001 at 300, 1002 none, 1003 at 100, 1004 at 300.
    for (n, number, expected) in [
        (1, 1001, Some(300)),
        (2, 1002, None),
        (3, 1003, Some(100)),
        (4, 1004, Some(300)),
    ] {
        let mut order = po(n, number, 1, vec![]);
        let mut d = order.draft();
        d.expected_at = expected.map(at);
        order.update(d, None, at(1)).unwrap();
        repo.insert(&mut order).await.unwrap();
    }
    let all = async |sort: PurchaseOrderSort| -> Vec<i64> {
        let mut numbers = Vec::new();
        let mut after = None;
        loop {
            let found = repo
                .list(&PurchaseOrderFilter::default(), sort, page(1, after))
                .await
                .unwrap();
            let Some(last) = found.items.last() else {
                break;
            };
            after = Some(Keyset {
                key: sort.key(last),
                id: last.id,
            });
            numbers.extend(found.items.iter().map(|s| s.number.value()));
            if !found.has_more {
                break;
            }
        }
        numbers
    };
    assert_eq!(
        all(PurchaseOrderSort::NumberDesc).await,
        [1004, 1003, 1002, 1001]
    );
    assert_eq!(
        all(PurchaseOrderSort::NumberAsc).await,
        [1001, 1002, 1003, 1004]
    );
    // Ties on the date go by ID; no date sorts last.
    assert_eq!(
        all(PurchaseOrderSort::ExpectedAt).await,
        [1003, 1001, 1004, 1002]
    );
}

async fn last_line<S: HasSuppliers + HasPurchaseOrders>(ports: &S) {
    with_suppliers(ports, &[1, 2]).await;
    let repo = ports.purchase_orders();
    let mut old = po(1, 1001, 1, vec![line(10, 1, "100")]);
    let mut newer = po(2, 1002, 1, vec![line(10, 1, "120"), line(11, 1, "5")]);
    let mut cancelled = po(3, 1003, 1, vec![line(10, 1, "999")]);
    cancelled.cancel(at(1)).unwrap();
    let mut other = po(4, 1004, 2, vec![line(10, 1, "50")]);
    for order in [&mut old, &mut newer, &mut cancelled, &mut other] {
        repo.insert(order).await.unwrap();
    }
    let cost = |found: Option<hypha_purchasing::PurchaseOrderLine>| {
        found.map(|l| l.unit_cost().minor_units())
    };
    assert_eq!(
        cost(repo.last_line(sid(1), vid(10)).await.unwrap()),
        Some("120".to_string())
    );
    assert_eq!(
        cost(repo.last_line(sid(2), vid(10)).await.unwrap()),
        Some("50".to_string())
    );
    assert_eq!(cost(repo.last_line(sid(1), vid(12)).await.unwrap()), None);
}

/// An order to `destination`, expected at `expected` seconds, ordered unless `draft`.
fn incoming_po(
    n: u128,
    lines: Vec<LineDraft>,
    destination: u128,
    expected: Option<i64>,
) -> PurchaseOrder {
    let mut order = po(n, 1000 + i64::try_from(n).unwrap(), 1, lines);
    let mut d = order.draft();
    d.destination = Some(lid(destination));
    d.expected_at = expected.map(at);
    order.update(d, None, at(1)).unwrap();
    order
        .order(Some(Currency::EUR), "Spore Labs", None, at(2))
        .unwrap();
    order
}

async fn incoming<S: HasSuppliers + HasPurchaseOrders>(ports: &S) {
    with_suppliers(ports, &[1]).await;
    let repo = ports.purchase_orders();
    // 5 of variant 10 ordered for place 50, 2 arrived and 1 refused: 2 still to come.
    let mut partly = incoming_po(1, vec![line(10, 5, "1"), line(11, 2, "1")], 50, Some(500));
    partly
        .record_receipt(
            &[ReceivedQuantities {
                variant: vid(10),
                accepted: 2,
                rejected: 1,
            }],
            false,
            at(3),
        )
        .unwrap();
    let mut sooner = incoming_po(2, vec![line(10, 3, "1")], 50, Some(300));
    let mut elsewhere = incoming_po(3, vec![line(10, 4, "1")], 60, None);
    // Neither a draft nor a closed order brings anything.
    let mut draft = po(4, 1004, 1, vec![line(10, 100, "1")]);
    let mut closed = incoming_po(5, vec![line(10, 7, "1")], 50, Some(100));
    closed.close(at(4)).unwrap();
    let mut other = incoming_po(6, vec![line(12, 1, "1")], 50, Some(100));
    for order in [
        &mut partly,
        &mut sooner,
        &mut elsewhere,
        &mut draft,
        &mut closed,
        &mut other,
    ] {
        repo.insert(order).await.unwrap();
    }
    let found: Vec<_> = repo
        .incoming(&[vid(11), vid(10)])
        .await
        .unwrap()
        .into_iter()
        .map(|i| (i.variant, i.location, i.quantity, i.next_expected_at))
        .collect();
    assert_eq!(
        found,
        [
            (vid(10), lid(50), 5, Some(at(300))),
            (vid(10), lid(60), 4, None),
            (vid(11), lid(50), 2, Some(at(500))),
        ]
    );
    assert_eq!(repo.incoming(&[]).await, Ok(vec![]));
    assert_eq!(repo.incoming(&[vid(99)]).await, Ok(vec![]));
}

async fn batches<S>(ports: &S)
where
    S: HasSuppliers + HasPurchaseOrders + HasOutbox + HasBatch<Batch: StagePurchaseOrders>,
{
    with_suppliers(ports, &[1]).await;
    let repo = ports.purchase_orders();
    let mut order = po(1, 1001, 1, vec![line(10, 2, "1")]);
    repo.insert(&mut order).await.unwrap();
    order
        .order(Some(Currency::EUR), "Spore Labs", None, at(1))
        .unwrap();
    let stale = order.clone();
    let mut batch = ports.batch();
    batch.update_purchase_order(&mut order);
    ports.commit(batch).await.unwrap();
    assert_eq!(order.version(), Version::new(2));
    assert_eq!(repo.get(poid(1)).await, Ok(Some(order.clone())));
    let ordered = ports
        .outbox()
        .events_after(0, 100)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.event_type == "purchase_order.ordered")
        .count();
    assert_eq!(ordered, 1);

    let mut stale = stale;
    let mut batch = ports.batch();
    batch.update_purchase_order(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(poid(1)).await, Ok(Some(order)));
}
