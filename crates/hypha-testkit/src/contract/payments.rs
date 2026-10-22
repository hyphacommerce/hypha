use hypha_core::{
    CartId, ChannelId, Currency, FinancialStatus, HasBatch, HasOutbox, MarketId, Money, OrderId,
    OutboxRepo, PaymentId, PaymentMethodId, RepoError, StoredEvent, Timestamp, Version, Versioned,
};
use hypha_orders::{HasOrders, OrderRepo, StageOrders};
use hypha_payments::{
    HasPaymentMethods, HasPayments, NewPayment, NewPaymentMethod, Payment, PaymentMethod,
    PaymentMethodRepo, PaymentRepo, PaymentStatus, ProviderKey, StagePayments, keys,
};
use uuid::Uuid;

use super::{orders::order, scenario};

/// Runs every `PaymentMethodRepo` scenario, each on a fresh repo from `make`.
pub async fn payment_method_repo<R: PaymentMethodRepo>(make: impl AsyncFn() -> R) {
    scenario("payment method: insert stores every field at version 1");
    method_round_trips(&make().await).await;
    scenario("payment method: update stores every field; stale and missing fail");
    method_updates(&make().await).await;
    scenario("payment method: unique ID");
    method_unique_id(&make().await).await;
    scenario("payment method: listed by position, then ID");
    methods_by_position(&make().await).await;
    scenario("payment method: delete at its version; stale and missing fail");
    method_deletes(&make().await).await;
}

/// Runs every `PaymentRepo` scenario, each on a fresh repo from `make`.
pub async fn payment_repo<R: PaymentRepo>(make: impl AsyncFn() -> R) {
    scenario("payment: insert stores every field at version 1");
    payment_round_trips(&make().await).await;
    scenario("payment: update stores a payment and a void; stale and missing fail");
    payment_updates(&make().await).await;
    scenario("payment: refunds' references and the attempt are stored, oldest first");
    refund_references_round_trip(&make().await).await;
    scenario("payment: unique ID");
    payment_unique_id(&make().await).await;
    scenario("payment: an order's payments, newest first, and no other order's");
    lists_for_order(&make().await).await;
    scenario("payment: a cart's payments before the order, newest first; attached later");
    cart_payments(&make().await).await;
}

/// Runs the scenarios that span methods and payments: a method an open payment uses
/// can't be deleted, checked in the same step as the delete.
pub async fn payment_method_delete<S: HasPaymentMethods + HasPayments>(make: impl AsyncFn() -> S) {
    scenario("payment method: delete is refused while an open payment uses it");
    open_payments_keep_their_method(&make().await).await;
}

/// Runs the batch scenarios for payments: a payment and its order commit together, with
/// their events, or not at all.
pub async fn payment_batch<S>(make: impl AsyncFn() -> S)
where
    S: HasBatch + HasOrders + HasPayments + HasOutbox,
    S::Batch: StageOrders + StagePayments,
{
    scenario("payment batch: inserts a payment with its order, events included");
    inserts_with_order(&make().await).await;
    scenario("payment batch: updates a payment with its order");
    updates_with_order(&make().await).await;
    scenario("payment batch: a stale order writes nothing, not even the new payment");
    stale_order_writes_no_payment(&make().await).await;
    scenario("payment batch: a stale payment writes nothing");
    stale_payment_writes_nothing(&make().await).await;
}

fn mid(n: u128) -> PaymentMethodId {
    PaymentMethodId::from_uuid(Uuid::from_u128(n))
}

fn pid(n: u128) -> PaymentId {
    PaymentId::from_uuid(Uuid::from_u128(n))
}

fn cid(n: u128) -> CartId {
    CartId::from_uuid(Uuid::from_u128(0x6a00 + n))
}

fn oid(n: u128) -> OrderId {
    OrderId::from_uuid(Uuid::from_u128(1000 + n))
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::EUR).unwrap()
}

fn at(micros: i64) -> Timestamp {
    Timestamp::from_unix_micros(1_767_225_600_000_000 + micros)
}

/// Method `n` at `position`: every field set.
fn method(n: u128, position: u32) -> PaymentMethod {
    PaymentMethod::new(NewPaymentMethod {
        id: mid(n),
        provider: ProviderKey::manual(),
        name: format!("Bank transfer {n}"),
        instructions: Some("IBAN HR12 3456 7890\nReference: your order number".to_string()),
        markets: vec![
            MarketId::from_uuid(Uuid::from_u128(30)),
            MarketId::from_uuid(Uuid::from_u128(31)),
        ],
        channels: vec![ChannelId::from_uuid(Uuid::from_u128(40))],
        enabled: true,
        position,
        settings: serde_json::json!({"nested": {"list": [1, "two", null]}, "flag": true}),
    })
    .unwrap()
}

/// A method with nothing optional: no instructions, every market and channel.
fn bare_method(n: u128) -> PaymentMethod {
    PaymentMethod::new(NewPaymentMethod {
        id: mid(n),
        provider: ProviderKey::new("app_stripe").unwrap(),
        name: "Card".to_string(),
        instructions: None,
        markets: vec![],
        channels: vec![],
        enabled: false,
        position: u32::MAX,
        settings: serde_json::json!({}),
    })
    .unwrap()
}

async fn method_round_trips<R: PaymentMethodRepo>(repo: &R) {
    let mut full = method(1, 3);
    repo.insert(&mut full).await.unwrap();
    assert_eq!(full.version(), Version::new(1));
    assert_eq!(repo.get(full.id()).await, Ok(Some(full)));
    let mut bare = bare_method(2);
    repo.insert(&mut bare).await.unwrap();
    assert_eq!(repo.get(bare.id()).await, Ok(Some(bare)));
    assert_eq!(repo.get(mid(99)).await, Ok(None));
}

async fn method_updates<R: PaymentMethodRepo>(repo: &R) {
    let mut m = method(1, 0);
    repo.insert(&mut m).await.unwrap();
    let stale = m.clone();
    m.rename("Cash on delivery").unwrap();
    m.set_instructions(None).unwrap();
    m.set_markets(vec![]);
    m.set_channels(vec![
        ChannelId::from_uuid(Uuid::from_u128(42)),
        ChannelId::from_uuid(Uuid::from_u128(41)),
    ]);
    m.set_enabled(false);
    m.set_position(9);
    m.set_settings(serde_json::json!({}));
    repo.update(&mut m).await.unwrap();
    assert_eq!(m.version(), Version::new(2));
    assert_eq!(repo.get(m.id()).await, Ok(Some(m)));

    let mut stale = stale;
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    let mut missing = method(2, 0);
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

async fn method_unique_id<R: PaymentMethodRepo>(repo: &R) {
    repo.insert(&mut method(1, 0)).await.unwrap();
    assert_eq!(
        repo.insert(&mut method(1, 1)).await,
        Err(RepoError::Duplicate(keys::PAYMENT_METHOD_ID))
    );
}

async fn methods_by_position<R: PaymentMethodRepo>(repo: &R) {
    assert_eq!(repo.list().await, Ok(vec![]));
    // Same position: by ID.
    for (n, position) in [(3, 1), (1, 2), (4, 0), (2, 1)] {
        repo.insert(&mut method(n, position)).await.unwrap();
    }
    let ids: Vec<PaymentMethodId> = repo
        .list()
        .await
        .unwrap()
        .iter()
        .map(PaymentMethod::id)
        .collect();
    assert_eq!(ids, [mid(4), mid(2), mid(3), mid(1)]);
    let listed = repo.list().await.unwrap();
    assert_eq!(Some(&listed[0]), repo.get(mid(4)).await.unwrap().as_ref());
}

async fn method_deletes<R: PaymentMethodRepo>(repo: &R) {
    let mut m = method(1, 0);
    repo.insert(&mut m).await.unwrap();
    let stale = m.clone();
    m.set_enabled(false);
    repo.update(&mut m).await.unwrap();
    assert_eq!(repo.delete(&stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.delete(&m).await, Ok(()));
    assert_eq!(repo.get(m.id()).await, Ok(None));
    assert_eq!(repo.delete(&m).await, Err(RepoError::StaleVersion));
}

/// Payment `n` of order `order`, by method `method`: every field set.
fn payment(n: u128, order: u128, method: u128) -> Payment {
    Payment::new(NewPayment {
        id: pid(n),
        cart: Some(cid(order)),
        order: Some(oid(order)),
        method: Some(mid(method)),
        method_name: Some("Bank transfer".to_string()),
        provider: ProviderKey::manual(),
        amount: eur(12_345),
        instructions: Some("IBAN HR12\nReference: #1001".to_string()),
    })
    .unwrap()
}

async fn payment_round_trips<R: PaymentRepo>(repo: &R) {
    let mut full = payment(1, 1, 1);
    repo.insert(&mut full).await.unwrap();
    assert_eq!(full.version(), Version::new(1));
    assert_eq!(repo.get(full.id()).await, Ok(Some(full)));
    // Recorded by hand: no method, name or instructions; nothing to pay.
    let mut bare = Payment::new(NewPayment::recorded_by_hand(pid(2), oid(1), eur(0))).unwrap();
    repo.insert(&mut bare).await.unwrap();
    assert_eq!(repo.get(bare.id()).await, Ok(Some(bare)));
    assert_eq!(repo.get(pid(99)).await, Ok(None));
}

async fn payment_updates<R: PaymentRepo>(repo: &R) {
    let mut p = payment(1, 1, 1);
    repo.insert(&mut p).await.unwrap();
    let stale = p.clone();
    p.pay(at(1), Some("RF-2026-1".to_string())).unwrap();
    repo.update(&mut p).await.unwrap();
    assert_eq!(p.version(), Version::new(2));
    let stored = repo.get(p.id()).await.unwrap().unwrap();
    assert_eq!(stored, p);
    assert_eq!(
        (stored.status(), stored.paid_at(), stored.reference()),
        (PaymentStatus::Paid, Some(at(1)), Some("RF-2026-1"))
    );

    let mut voided = payment(2, 1, 1);
    repo.insert(&mut voided).await.unwrap();
    voided.void(at(2)).unwrap();
    repo.update(&mut voided).await.unwrap();
    assert_eq!(repo.get(voided.id()).await, Ok(Some(voided)));

    let mut stale = stale;
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    let mut missing = payment(3, 1, 1);
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

async fn refund_references_round_trip<R: PaymentRepo>(repo: &R) {
    let mut p = payment(1, 1, 1);
    repo.insert(&mut p).await.unwrap();
    p.pay(at(1), Some("pi_1".to_string())).unwrap();
    p.note_attempt(Some("pi_1".to_string())).unwrap();
    repo.update(&mut p).await.unwrap();
    for (amount, refund) in [(100, "re_2"), (200, "re_1")] {
        p.refund(
            Some(eur(amount)),
            PaymentStatus::PartiallyRefunded,
            Some(refund.to_string()),
        )
        .unwrap();
        repo.update(&mut p).await.unwrap();
    }
    let stored = repo.get(p.id()).await.unwrap().unwrap();
    assert_eq!(stored, p);
    assert_eq!(stored.refund_references(), ["re_2", "re_1"]);
    assert_eq!(stored.reference(), Some("pi_1"));
    assert_eq!(stored.attempt(), Some("pi_1"));
    assert_eq!(repo.list_for_order(oid(1)).await, Ok(vec![p]));
}

async fn payment_unique_id<R: PaymentRepo>(repo: &R) {
    repo.insert(&mut payment(1, 1, 1)).await.unwrap();
    assert_eq!(
        repo.insert(&mut payment(1, 2, 1)).await,
        Err(RepoError::Duplicate(keys::PAYMENT_ID))
    );
}

async fn lists_for_order<R: PaymentRepo>(repo: &R) {
    assert_eq!(repo.list_for_order(oid(1)).await, Ok(vec![]));
    for (n, order) in [(2, 1), (1, 1), (3, 2), (4, 1)] {
        repo.insert(&mut payment(n, order, 1)).await.unwrap();
    }
    let ids: Vec<PaymentId> = repo
        .list_for_order(oid(1))
        .await
        .unwrap()
        .iter()
        .map(Payment::id)
        .collect();
    assert_eq!(ids, [pid(4), pid(2), pid(1)]);
    let mut voided = repo.get(pid(2)).await.unwrap().unwrap();
    voided.void(at(0)).unwrap();
    repo.update(&mut voided).await.unwrap();
    let listed = repo.list_for_order(oid(1)).await.unwrap();
    assert_eq!(listed[1], voided);
}

/// Payment `n` started for cart `cart` before its order.
fn cart_payment(n: u128, cart: u128) -> Payment {
    Payment::new(NewPayment {
        id: pid(n),
        cart: Some(cid(cart)),
        order: None,
        method: Some(mid(1)),
        method_name: Some("Card".to_string()),
        provider: ProviderKey::new("app_pay").unwrap(),
        amount: eur(12_345),
        instructions: None,
    })
    .unwrap()
}

async fn cart_payments<R: PaymentRepo>(repo: &R) {
    assert_eq!(repo.list_for_cart(cid(1)).await, Ok(vec![]));
    for (n, cart) in [(2, 1), (1, 1), (3, 2)] {
        repo.insert(&mut cart_payment(n, cart)).await.unwrap();
    }
    let ids: Vec<PaymentId> = repo
        .list_for_cart(cid(1))
        .await
        .unwrap()
        .iter()
        .map(Payment::id)
        .collect();
    assert_eq!(ids, [pid(2), pid(1)]);
    let stored = repo.get(pid(2)).await.unwrap().unwrap();
    assert_eq!((stored.cart(), stored.order()), (Some(cid(1)), None));
    assert_eq!(repo.list_for_order(oid(1)).await, Ok(vec![]));
    // Processing, then attached to its order: an order's payment now.
    let mut paid = stored;
    paid.process(Some("pi_2".to_string())).unwrap();
    paid.attach(oid(1)).unwrap();
    repo.update(&mut paid).await.unwrap();
    assert_eq!(repo.get(pid(2)).await, Ok(Some(paid.clone())));
    assert_eq!(repo.list_for_order(oid(1)).await, Ok(vec![paid]));
}

async fn open_payments_keep_their_method<S: HasPaymentMethods + HasPayments>(ports: &S) {
    let mut m = method(1, 0);
    ports.payment_methods().insert(&mut m).await.unwrap();
    let mut other = method(2, 1);
    ports.payment_methods().insert(&mut other).await.unwrap();
    // Pending, then authorized: both hold the method.
    let mut p = payment(1, 1, 1);
    ports.payments().insert(&mut p).await.unwrap();
    let in_use = Err(RepoError::Reference(keys::OPEN_PAYMENT_METHOD));
    assert_eq!(ports.payment_methods().delete(&m).await, in_use);
    p.authorize(None).unwrap();
    ports.payments().update(&mut p).await.unwrap();
    assert_eq!(ports.payment_methods().delete(&m).await, in_use);
    // Refused, it's still there, unchanged.
    assert_eq!(
        ports.payment_methods().get(m.id()).await,
        Ok(Some(m.clone()))
    );
    // A stale method is stale, whatever its payments.
    let mut newer = m.clone();
    newer.set_enabled(false);
    ports.payment_methods().update(&mut newer).await.unwrap();
    assert_eq!(
        ports.payment_methods().delete(&m).await,
        Err(RepoError::StaleVersion)
    );
    // A processing payment holds its method too.
    let mut third = method(3, 2);
    ports.payment_methods().insert(&mut third).await.unwrap();
    let mut slow = payment(3, 3, 3);
    ports.payments().insert(&mut slow).await.unwrap();
    slow.process(None).unwrap();
    ports.payments().update(&mut slow).await.unwrap();
    assert_eq!(ports.payment_methods().delete(&third).await, in_use);
    // Another method's open payment doesn't hold this one.
    assert_eq!(ports.payment_methods().delete(&other).await, Ok(()));
    // Once nothing open uses it (paid, voided), it goes.
    p.pay(at(0), None).unwrap();
    ports.payments().update(&mut p).await.unwrap();
    let mut voided = payment(2, 1, 1);
    ports.payments().insert(&mut voided).await.unwrap();
    voided.void(at(0)).unwrap();
    ports.payments().update(&mut voided).await.unwrap();
    assert_eq!(ports.payment_methods().delete(&newer).await, Ok(()));
    assert_eq!(ports.payment_methods().get(m.id()).await, Ok(None));
}

async fn events<S: HasOutbox>(ports: &S) -> Vec<StoredEvent> {
    ports.outbox().events_after(0, 100).await.unwrap()
}

fn types(events: &[StoredEvent]) -> Vec<&str> {
    events.iter().map(|e| e.event_type.as_str()).collect()
}

/// Order 1 stored, and a payment for its total.
async fn order_and_payment<S: HasOrders>(ports: &S) -> (hypha_orders::Order, Payment) {
    let mut o = order(1);
    ports.orders().insert(&mut o).await.unwrap();
    let p = Payment::new(NewPayment {
        id: pid(1),
        cart: Some(o.cart()),
        order: Some(o.id()),
        method: Some(mid(1)),
        method_name: Some("Cash on delivery".to_string()),
        provider: ProviderKey::manual(),
        amount: o.total(),
        instructions: None,
    })
    .unwrap();
    (o, p)
}

async fn inserts_with_order<S>(ports: &S)
where
    S: HasBatch + HasOrders + HasPayments + HasOutbox,
    S::Batch: StageOrders + StagePayments,
{
    let (mut o, mut p) = order_and_payment(ports).await;
    let before = events(ports).await.len();
    let mut batch = ports.batch();
    batch.insert_payment(&mut p);
    batch.update_order(&mut o);
    assert_eq!(
        (p.version(), o.version()),
        (Version::new(1), Version::new(2))
    );
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(ports.payments().get(p.id()).await, Ok(Some(p.clone())));
    assert_eq!(ports.orders().get(o.id()).await, Ok(Some(o)));
    let written = events(ports).await;
    assert_eq!(types(&written[before..]), ["payment.created"]);
    assert_eq!(written[before].subject_id, p.id().as_uuid());
    assert_eq!(written[before].subject_version, Some(Version::new(1)));
}

async fn updates_with_order<S>(ports: &S)
where
    S: HasBatch + HasOrders + HasPayments + HasOutbox,
    S::Batch: StageOrders + StagePayments,
{
    let (mut o, mut p) = order_and_payment(ports).await;
    ports.payments().insert(&mut p).await.unwrap();
    p.pay(at(5), Some("receipt 7".to_string())).unwrap();
    o.set_financial_status(FinancialStatus::Paid);
    let mut batch = ports.batch();
    batch.update_order(&mut o);
    batch.update_payment(&mut p);
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(ports.payments().get(p.id()).await, Ok(Some(p)));
    let stored = ports.orders().get(o.id()).await.unwrap().unwrap();
    assert_eq!(stored.financial_status(), FinancialStatus::Paid);
    let written = events(ports).await;
    let tail = types(&written[written.len() - 2..]);
    assert!(
        tail.contains(&"payment.paid") && tail.contains(&"order.paid"),
        "{tail:?}"
    );
}

async fn stale_order_writes_no_payment<S>(ports: &S)
where
    S: HasBatch + HasOrders + HasPayments + HasOutbox,
    S::Batch: StageOrders + StagePayments,
{
    let (o, mut p) = order_and_payment(ports).await;
    let mut rival = o.clone();
    rival.set_financial_status(FinancialStatus::Voided);
    ports.orders().update(&mut rival).await.unwrap();
    let before = events(ports).await.len();

    let mut stale = o;
    stale.set_financial_status(FinancialStatus::Paid);
    let mut batch = ports.batch();
    batch.insert_payment(&mut p);
    batch.update_order(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.payments().get(pid(1)).await, Ok(None));
    assert_eq!(ports.orders().get(rival.id()).await, Ok(Some(rival)));
    assert_eq!(events(ports).await.len(), before);
}

async fn stale_payment_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasOrders + HasPayments + HasOutbox,
    S::Batch: StageOrders + StagePayments,
{
    let (o, mut p) = order_and_payment(ports).await;
    ports.payments().insert(&mut p).await.unwrap();
    let mut rival = p.clone();
    rival.void(at(1)).unwrap();
    ports.payments().update(&mut rival).await.unwrap();

    let (mut order, mut stale) = (o.clone(), p);
    stale.pay(at(2), None).unwrap();
    order.set_financial_status(FinancialStatus::Paid);
    let mut batch = ports.batch();
    batch.update_order(&mut order);
    batch.update_payment(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.payments().get(pid(1)).await, Ok(Some(rival)));
    assert_eq!(ports.orders().get(o.id()).await, Ok(Some(o)));
}
