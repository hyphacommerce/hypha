use hypha_cart::{Cart, CartRepo, HasCarts, StageCarts, repo::keys as cart_keys};
use hypha_core::{
    Actor, CartId, CountryCode, CustomerId, Email, FulfillmentId, HasBatch, HasIdempotency,
    IdempotencyKey, IdempotencyRecord, IdempotencyRepo, InventoryItemId, LocationId, MarketId,
    ProductId, Quantity, RepoError, StageIdempotency, Timestamp, VariantId, Version, Versioned,
    idempotency::keys as idempotency_keys,
};
use hypha_customers::{
    Customer, CustomerRepo, HasCustomers, StageCustomers, repo::keys as customer_keys,
};
use hypha_fulfillment::{
    Fulfillment, FulfillmentLine, FulfillmentRepo, HasFulfillments, NewFulfillment,
    StageFulfillments, Tracking,
};
use hypha_inventory::{HasInventoryItems, InventoryItem, InventoryItemRepo, StageInventoryItems};
use hypha_orders::{HasOrders, Order, OrderRepo, StageOrders, repo::keys as order_keys};
use uuid::Uuid;

use super::{orders::order, scenario};

/// Runs every `HasBatch` scenario, each on fresh ports from `make`: writes to several
/// aggregates commit together, with the versions checked as single writes check them.
pub async fn batch<S>(make: impl AsyncFn() -> S)
where
    S: HasBatch
        + HasCarts
        + HasInventoryItems
        + HasOrders
        + HasCustomers
        + HasIdempotency
        + HasFulfillments,
    S::Batch: StageCarts
        + StageInventoryItems
        + StageOrders
        + StageCustomers
        + StageIdempotency
        + StageFulfillments,
{
    scenario("batch: commits every staged write, at the versions staging promised");
    commits_everything(&make().await).await;
    scenario("batch: a stale cart writes nothing");
    stale_cart_writes_nothing(&make().await).await;
    scenario("batch: a stale inventory item writes nothing");
    stale_item_writes_nothing(&make().await).await;
    scenario("batch: inserts a new cart at version 1");
    inserts_a_cart(&make().await).await;
    scenario("batch: a duplicate new cart writes nothing");
    duplicate_cart_writes_nothing(&make().await).await;
    scenario("batch: inserts an order with a cart update");
    inserts_an_order(&make().await).await;
    scenario("batch: a duplicate order writes nothing");
    duplicate_order_writes_nothing(&make().await).await;
    scenario("batch: a second order for a cart writes nothing");
    second_order_for_a_cart_writes_nothing(&make().await).await;
    scenario("batch: inserts a customer; a taken email writes nothing");
    customers(&make().await).await;
    scenario("batch: updates a customer with a cart; a stale customer writes nothing");
    customer_updated(&make().await).await;
    scenario("batch: deletes a customer at its version with its carts unlinked");
    customer_deleted(&make().await).await;
    scenario("batch: a stale cart rolls back the order and customer inserted before it");
    failure_rolls_back_inserts(&make().await).await;
    scenario("batch: a taken idempotency key writes nothing else (checkout's batch)");
    taken_key_writes_nothing(&make().await).await;
    scenario("batch: an empty batch commits");
    empty_batch(&make().await).await;
    scenario("batch: a fulfilment with its order and stock (design note 0018)");
    fulfils_an_order(&make().await).await;
    scenario("batch: a stale order writes nothing, not even the new fulfilment");
    stale_order_writes_nothing(&make().await).await;
    scenario("batch: a stale fulfilment writes nothing");
    stale_fulfillment_writes_nothing(&make().await).await;
    scenario("batch: deletes a cart at its version; a stale or missing one writes nothing");
    cart_deleted(&make().await).await;
    scenario("batch: deletes an inventory item at its version");
    deletes_an_item(&make().await).await;
    scenario("batch: deleting a stale or missing inventory item writes nothing");
    stale_deleted_item_writes_nothing(&make().await).await;
}

fn cart(n: u128) -> Cart {
    Cart::new(
        CartId::from_uuid(Uuid::from_u128(n)),
        MarketId::from_uuid(Uuid::from_u128(1000 + n)),
        CountryCode::new("HR").unwrap(),
        hypha_core::ChannelId::from_uuid(Uuid::from_u128(500)),
    )
}

fn item(n: u128) -> InventoryItem {
    InventoryItem::new(
        InventoryItemId::from_uuid(Uuid::from_u128(n)),
        VariantId::from_uuid(Uuid::from_u128(2000 + n)),
        true,
        false,
    )
}

fn add_mug(cart: &mut Cart) {
    cart.add_line(
        ProductId::from_uuid(Uuid::from_u128(1)),
        VariantId::from_uuid(Uuid::from_u128(11)),
        Quantity::new(2).unwrap(),
    )
    .unwrap();
}

/// A stored cart and two stored items, all at version 1.
async fn stored<S: HasCarts + HasInventoryItems>(
    ports: &S,
) -> (Cart, InventoryItem, InventoryItem) {
    let (mut c, mut a, mut b) = (cart(1), item(1), item(2));
    ports.carts().insert(&mut c).await.unwrap();
    ports.inventory_items().insert(&mut a).await.unwrap();
    ports.inventory_items().insert(&mut b).await.unwrap();
    (c, a, b)
}

async fn commits_everything<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems,
    S::Batch: StageCarts + StageInventoryItems,
{
    let (mut c, mut a, mut b) = stored(ports).await;
    let mut d = cart(2);
    ports.carts().insert(&mut d).await.unwrap();
    add_mug(&mut c);
    add_mug(&mut d);
    a.set_tracked(false);
    b.set_continue_selling(true);

    // Staged highest ID first: adapters that lock rows must sort them, not trust the
    // staging order.
    let mut batch = ports.batch();
    batch.update_cart(&mut d);
    batch.update_cart(&mut c);
    batch.update_inventory_item(&mut b);
    batch.update_inventory_item(&mut a);
    assert_eq!(c.version(), Version::new(2));
    assert_eq!(ports.commit(batch).await, Ok(()));

    // What was staged is what is stored, versions included.
    assert_eq!(ports.carts().get(d.id()).await, Ok(Some(d)));
    assert_eq!(ports.carts().get(c.id()).await, Ok(Some(c)));
    assert_eq!(ports.inventory_items().get(a.id()).await, Ok(Some(a)));
    assert_eq!(ports.inventory_items().get(b.id()).await, Ok(Some(b)));
}

async fn stale_cart_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems,
    S::Batch: StageCarts + StageInventoryItems,
{
    let (c, a, _) = stored(ports).await;
    // Someone else changes the cart after this command loaded it.
    let mut rival = c.clone();
    add_mug(&mut rival);
    ports.carts().update(&mut rival).await.unwrap();

    let (mut stale, mut changed) = (c, a.clone());
    stale.set_email(None).unwrap();
    changed.set_tracked(false);
    let mut batch = ports.batch();
    batch.update_inventory_item(&mut changed);
    batch.update_cart(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));

    assert_eq!(ports.carts().get(rival.id()).await, Ok(Some(rival)));
    assert_eq!(ports.inventory_items().get(a.id()).await, Ok(Some(a)));
}

async fn stale_item_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems,
    S::Batch: StageCarts + StageInventoryItems,
{
    let (c, a, b) = stored(ports).await;
    let mut rival = b.clone();
    rival.set_tracked(false);
    ports.inventory_items().update(&mut rival).await.unwrap();

    let (mut changed_cart, mut changed_a, mut stale_b) = (c.clone(), a.clone(), b);
    add_mug(&mut changed_cart);
    changed_a.set_tracked(false);
    stale_b.set_continue_selling(true);
    let mut batch = ports.batch();
    batch.update_cart(&mut changed_cart);
    batch.update_inventory_item(&mut changed_a);
    batch.update_inventory_item(&mut stale_b);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));

    assert_eq!(ports.carts().get(c.id()).await, Ok(Some(c)));
    assert_eq!(ports.inventory_items().get(a.id()).await, Ok(Some(a)));
    assert_eq!(
        ports.inventory_items().get(rival.id()).await,
        Ok(Some(rival))
    );
}

async fn inserts_a_cart<S>(ports: &S)
where
    S: HasBatch + HasCarts,
    S::Batch: StageCarts,
{
    let mut new = cart(7);
    add_mug(&mut new);
    let mut batch = ports.batch();
    batch.insert_cart(&mut new);
    assert_eq!(new.version(), Version::new(1));
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(ports.carts().get(new.id()).await, Ok(Some(new)));
}

async fn duplicate_cart_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems,
    S::Batch: StageCarts + StageInventoryItems,
{
    let (c, a, _) = stored(ports).await;
    let (mut again, mut changed) = (cart(1), a.clone());
    changed.set_tracked(false);
    let mut batch = ports.batch();
    batch.update_inventory_item(&mut changed);
    batch.insert_cart(&mut again);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(cart_keys::CART_ID))
    );
    assert_eq!(ports.carts().get(c.id()).await, Ok(Some(c)));
    assert_eq!(ports.inventory_items().get(a.id()).await, Ok(Some(a)));
}

async fn inserts_an_order<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasOrders,
    S::Batch: StageCarts + StageOrders,
{
    let (mut c, _, _) = stored(ports).await;
    add_mug(&mut c);
    let mut new = order(1);
    let mut batch = ports.batch();
    batch.insert_order(&mut new);
    batch.update_cart(&mut c);
    assert_eq!(new.version(), Version::new(1));
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(ports.orders().get(new.id()).await, Ok(Some(new)));
    assert_eq!(ports.carts().get(c.id()).await, Ok(Some(c)));
}

async fn duplicate_order_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasOrders,
    S::Batch: StageCarts + StageOrders,
{
    let (c, _, _) = stored(ports).await;
    let mut first = order(1);
    ports.orders().insert(&mut first).await.unwrap();

    let (mut again, mut changed) = (order(1), c.clone());
    add_mug(&mut changed);
    let mut batch = ports.batch();
    batch.update_cart(&mut changed);
    batch.insert_order(&mut again);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(order_keys::ORDER_ID))
    );
    assert_eq!(ports.carts().get(c.id()).await, Ok(Some(c)));
}

async fn customers<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasCustomers,
    S::Batch: StageCarts + StageCustomers,
{
    let mut first = Customer::new(CustomerId::from_uuid(Uuid::from_u128(1)), ana());
    let mut batch = ports.batch();
    batch.insert_customer(&mut first);
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(first.version(), Version::new(1));
    assert_eq!(ports.customers().get(first.id()).await, Ok(Some(first)));

    let (c, _, _) = stored(ports).await;
    let (mut changed, mut twin) = (
        c.clone(),
        Customer::new(CustomerId::from_uuid(Uuid::from_u128(2)), ana()),
    );
    add_mug(&mut changed);
    let mut batch = ports.batch();
    batch.update_cart(&mut changed);
    batch.insert_customer(&mut twin);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(customer_keys::CUSTOMER_EMAIL))
    );
    assert_eq!(ports.carts().get(c.id()).await, Ok(Some(c)));
}

async fn cart_deleted<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems,
    S::Batch: StageCarts,
{
    let (c, _, _) = stored(ports).await;
    let mut stale = c.clone();
    stale.set_version(Version::new(7));
    let mut batch = ports.batch();
    batch.delete_cart(&stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.carts().get(c.id()).await, Ok(Some(c.clone())));

    let mut batch = ports.batch();
    batch.delete_cart(&c);
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(ports.carts().get(c.id()).await, Ok(None));
    let mut batch = ports.batch();
    batch.delete_cart(&c);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
}

async fn customer_updated<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasCustomers,
    S::Batch: StageCarts + StageCustomers,
{
    let mut ana = Customer::new(CustomerId::from_uuid(Uuid::from_u128(1)), ana());
    ports.customers().insert(&mut ana).await.unwrap();
    let (c, _, _) = stored(ports).await;

    let mut changed = ana.clone();
    changed.set_first_name(Some("Ana".to_string())).unwrap();
    let mut cart = c.clone();
    cart.set_customer(Some((ana.id(), ana.email().clone())))
        .unwrap();
    let mut batch = ports.batch();
    batch.update_customer(&mut changed);
    batch.update_cart(&mut cart);
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(changed.version(), Version::new(2));
    assert_eq!(
        ports.customers().get(ana.id()).await,
        Ok(Some(changed.clone()))
    );
    assert_eq!(ports.carts().get(c.id()).await, Ok(Some(cart.clone())));

    // From the old copy: nothing, not even the cart staged with it.
    let mut stale = ana;
    stale.set_phone(Some("+385".to_string())).unwrap();
    let mut cart_again = cart.clone();
    cart_again.set_customer(None).unwrap();
    let mut batch = ports.batch();
    batch.update_customer(&mut stale);
    batch.update_cart(&mut cart_again);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.customers().get(changed.id()).await, Ok(Some(changed)));
    assert_eq!(ports.carts().get(c.id()).await, Ok(Some(cart)));
}

async fn customer_deleted<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasCustomers,
    S::Batch: StageCarts + StageCustomers,
{
    let mut ana = Customer::new(CustomerId::from_uuid(Uuid::from_u128(1)), ana());
    ports.customers().insert(&mut ana).await.unwrap();
    let (c, _, _) = stored(ports).await;
    let mut cart = c.clone();
    cart.set_customer(Some((ana.id(), ana.email().clone())))
        .unwrap();
    ports.carts().update(&mut cart).await.unwrap();

    // Deleting from a stale copy writes nothing.
    let mut stale = ana.clone();
    stale.set_version(Version::new(7));
    let mut batch = ports.batch();
    batch.delete_customer(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.customers().get(ana.id()).await, Ok(Some(ana.clone())));

    let mut unlinked = cart.clone();
    unlinked.set_customer(None).unwrap();
    let mut batch = ports.batch();
    batch.update_cart(&mut unlinked);
    ana.delete();
    batch.delete_customer(&mut ana);
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(ports.customers().get(ana.id()).await, Ok(None));
    assert_eq!(ports.customers().get_by_email(ana.email()).await, Ok(None));
    assert_eq!(ports.carts().of_customer(ana.id()).await, Ok(vec![]));
    let stored = ports.carts().get(c.id()).await.unwrap().unwrap();
    assert_eq!(
        (stored.customer(), stored.email()),
        (None, Some(ana.email()))
    );

    // Deleting what's gone: stale, like products.
    let mut batch = ports.batch();
    batch.delete_customer(&mut ana);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
}

async fn second_order_for_a_cart_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasOrders,
    S::Batch: StageCarts + StageInventoryItems + StageOrders,
{
    let (_, a, _) = stored(ports).await;
    let mut first = order(1);
    ports.orders().insert(&mut first).await.unwrap();

    // Another checkout of the same cart: a new order ID and number, the same cart.
    let mut snapshot = order(2).snapshot();
    snapshot.cart = first.cart();
    let mut second = Order::restore(snapshot).unwrap();
    let mut changed = a.clone();
    changed.set_tracked(false);
    let mut batch = ports.batch();
    batch.insert_order(&mut second);
    batch.update_inventory_item(&mut changed);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(order_keys::ORDER_CART))
    );
    assert_eq!(ports.orders().get(second.id()).await, Ok(None));
    assert_eq!(ports.inventory_items().get(a.id()).await, Ok(Some(a)));
}

fn ana() -> Email {
    Email::new("ana@example.hr").unwrap()
}

async fn failure_rolls_back_inserts<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasOrders + HasCustomers,
    S::Batch: StageCarts + StageOrders + StageCustomers,
{
    let (c, _, _) = stored(ports).await;
    let mut rival = c.clone();
    add_mug(&mut rival);
    ports.carts().update(&mut rival).await.unwrap();

    // Inserts are staged before the stale update, so an adapter that writes in order
    // has already written them when the update fails.
    let (mut new_order, mut guest, mut stale) = (
        order(1),
        Customer::new(CustomerId::from_uuid(Uuid::from_u128(1)), ana()),
        c,
    );
    stale.set_email(None).unwrap();
    let mut batch = ports.batch();
    batch.insert_order(&mut new_order);
    batch.insert_customer(&mut guest);
    batch.update_cart(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));

    assert_eq!(ports.orders().get(new_order.id()).await, Ok(None));
    assert_eq!(ports.customers().get_by_email(&ana()).await, Ok(None));
    assert_eq!(ports.carts().get(rival.id()).await, Ok(Some(rival)));
}

async fn taken_key_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasOrders + HasCustomers + HasIdempotency,
    S::Batch: StageCarts + StageInventoryItems + StageOrders + StageCustomers + StageIdempotency,
{
    let record = |result: u128| IdempotencyRecord {
        actor: Actor::System,
        key: IdempotencyKey::new("checkout-1").unwrap(),
        operation: "checkout".to_string(),
        fingerprint: [7; 32],
        result: Uuid::from_u128(result),
        created_at: Timestamp::from_unix_micros(0),
    };
    let mut batch = ports.batch();
    batch.record_idempotency(record(1));
    ports.commit(batch).await.unwrap();

    // Everything checkout writes, with the key another request already used.
    let (c, a, _) = stored(ports).await;
    let (mut new_order, mut cart, mut item, mut guest) = (
        order(2),
        c.clone(),
        a.clone(),
        Customer::new(CustomerId::from_uuid(Uuid::from_u128(1)), ana()),
    );
    add_mug(&mut cart);
    item.set_tracked(false);
    let mut batch = ports.batch();
    batch.insert_order(&mut new_order);
    batch.update_cart(&mut cart);
    batch.update_inventory_item(&mut item);
    batch.insert_customer(&mut guest);
    batch.record_idempotency(record(2));
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(idempotency_keys::IDEMPOTENCY_KEY))
    );

    assert_eq!(ports.orders().get(new_order.id()).await, Ok(None));
    assert_eq!(ports.carts().get(c.id()).await, Ok(Some(c)));
    assert_eq!(ports.inventory_items().get(a.id()).await, Ok(Some(a)));
    assert_eq!(ports.customers().get_by_email(&ana()).await, Ok(None));
    let kept = ports
        .idempotency()
        .get(Actor::System, &IdempotencyKey::new("checkout-1").unwrap())
        .await
        .unwrap()
        .map(|r| r.result);
    assert_eq!(kept, Some(Uuid::from_u128(1)));
}

async fn empty_batch<S: HasBatch>(ports: &S) {
    assert_eq!(ports.commit(ports.batch()).await, Ok(()));
}

/// Fulfilment `n` of `order`'s first line, one unit, with tracking.
fn fulfillment(n: u128, order: &Order) -> Fulfillment {
    Fulfillment::new(NewFulfillment {
        id: FulfillmentId::from_uuid(Uuid::from_u128(n)),
        order: order.id(),
        location: LocationId::from_uuid(Uuid::from_u128(500)),
        lines: vec![FulfillmentLine {
            line: order.lines()[0].id,
            quantity: Quantity::new(1).unwrap(),
            stock_taken: true,
        }],
        tracking: vec![Tracking::new(Some("DHL"), "1", None).unwrap()],
        notify_customer: false,
    })
    .unwrap()
}

/// A stored order (one line of 1), and stored items 1 and 2.
async fn stored_order<S: HasOrders + HasCarts + HasInventoryItems>(
    ports: &S,
) -> (Order, InventoryItem, InventoryItem) {
    let (_, a, b) = stored(ports).await;
    let mut o = order(1);
    ports.orders().insert(&mut o).await.unwrap();
    (o, a, b)
}

fn first_line(o: &Order) -> [(hypha_core::OrderLineId, Quantity); 1] {
    [(o.lines()[0].id, Quantity::new(1).unwrap())]
}

async fn fulfils_an_order<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasOrders + HasFulfillments,
    S::Batch: StageOrders + StageInventoryItems + StageFulfillments,
{
    let (mut o, mut a, _) = stored_order(ports).await;
    let mut f = fulfillment(1, &o);
    o.fulfil(&first_line(&o)).unwrap();
    a.set_tracked(false);
    let mut batch = ports.batch();
    batch.insert_fulfillment(&mut f);
    batch.update_order(&mut o);
    batch.update_inventory_item(&mut a);
    assert_eq!(
        (f.version(), o.version()),
        (Version::new(1), Version::new(2))
    );
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(ports.fulfillments().get(f.id()).await, Ok(Some(f.clone())));
    assert_eq!(ports.orders().get(o.id()).await, Ok(Some(o.clone())));
    assert_eq!(ports.inventory_items().get(a.id()).await, Ok(Some(a)));

    // Then cancelled: the fulfilment and the order updated together.
    f.cancel(Timestamp::from_unix_micros(1_767_225_600_000_001));
    o.unfulfil(&first_line(&o)).unwrap();
    let mut batch = ports.batch();
    batch.update_order(&mut o);
    batch.update_fulfillment(&mut f);
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(ports.fulfillments().get(f.id()).await, Ok(Some(f)));
    assert_eq!(ports.orders().get(o.id()).await, Ok(Some(o)));
}

async fn stale_order_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasOrders + HasFulfillments,
    S::Batch: StageOrders + StageInventoryItems + StageFulfillments,
{
    let (o, a, _) = stored_order(ports).await;
    // Another request fulfils it first.
    let mut rival = o.clone();
    rival.fulfil(&first_line(&o)).unwrap();
    ports.orders().update(&mut rival).await.unwrap();

    let (mut stale, mut changed) = (o, a.clone());
    let mut f = fulfillment(1, &stale);
    stale.fulfil(&first_line(&rival)).unwrap();
    changed.set_tracked(false);
    let mut batch = ports.batch();
    batch.insert_fulfillment(&mut f);
    batch.update_inventory_item(&mut changed);
    batch.update_order(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.fulfillments().get(f.id()).await, Ok(None));
    assert_eq!(ports.orders().get(rival.id()).await, Ok(Some(rival)));
    assert_eq!(ports.inventory_items().get(a.id()).await, Ok(Some(a)));
}

async fn stale_fulfillment_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasOrders + HasFulfillments,
    S::Batch: StageOrders + StageFulfillments,
{
    let (mut o, _, _) = stored_order(ports).await;
    let mut f = fulfillment(1, &o);
    ports.fulfillments().insert(&mut f).await.unwrap();
    o.fulfil(&first_line(&o)).unwrap();
    ports.orders().update(&mut o).await.unwrap();
    let mut rival = f.clone();
    rival.cancel(Timestamp::UNIX_EPOCH);
    ports.fulfillments().update(&mut rival).await.unwrap();

    let (mut stale, mut changed) = (f, o.clone());
    stale.set_tracking(vec![], false).unwrap();
    changed.set_financial_status(hypha_core::FinancialStatus::Paid);
    let mut batch = ports.batch();
    batch.update_order(&mut changed);
    batch.update_fulfillment(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.orders().get(o.id()).await, Ok(Some(o)));
    assert_eq!(ports.fulfillments().get(rival.id()).await, Ok(Some(rival)));
}

async fn deletes_an_item<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems,
    S::Batch: StageInventoryItems,
{
    let (_, mut a, mut b) = stored(ports).await;
    a.delete().unwrap();
    b.set_tracked(false);
    let mut batch = ports.batch();
    batch.delete_inventory_item(&mut a);
    batch.update_inventory_item(&mut b);
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(ports.inventory_items().get(a.id()).await, Ok(None));
    assert_eq!(ports.inventory_items().get(b.id()).await, Ok(Some(b)));
}

async fn stale_deleted_item_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasCarts + HasInventoryItems,
    S::Batch: StageInventoryItems,
{
    let (_, a, b) = stored(ports).await;
    // Changed since it was loaded (say, a new reservation).
    let mut rival = a.clone();
    rival.set_tracked(false);
    ports.inventory_items().update(&mut rival).await.unwrap();

    let (mut stale, mut changed) = (a, b.clone());
    stale.delete().unwrap();
    changed.set_tracked(false);
    let mut batch = ports.batch();
    batch.update_inventory_item(&mut changed);
    batch.delete_inventory_item(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(
        ports.inventory_items().get(rival.id()).await,
        Ok(Some(rival.clone()))
    );
    assert_eq!(ports.inventory_items().get(b.id()).await, Ok(Some(b)));

    // Already gone.
    let mut gone = rival.clone();
    ports.inventory_items().delete(&mut rival).await.unwrap();
    gone.delete().unwrap();
    let mut batch = ports.batch();
    batch.delete_inventory_item(&mut gone);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
}
