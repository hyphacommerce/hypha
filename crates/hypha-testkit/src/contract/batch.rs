use hypha_cart::{Cart, CartRepo, HasCarts, StageCarts, repo::keys as cart_keys};
use hypha_core::{
    CartId, CountryCode, CustomerId, HasBatch, InventoryItemId, MarketId, ProductId, Quantity,
    RepoError, VariantId, Version, Versioned,
};
use hypha_customers::{
    Customer, CustomerRepo, HasCustomers, StageCustomers, repo::keys as customer_keys,
};
use hypha_inventory::{HasInventoryItems, InventoryItem, InventoryItemRepo, StageInventoryItems};
use hypha_orders::{HasOrders, OrderRepo, StageOrders, repo::keys as order_keys};
use uuid::Uuid;

use super::{orders::order, scenario};

/// Runs every `HasBatch` scenario, each on fresh ports from `make`: writes to several
/// aggregates commit together, with the versions checked as single writes check them.
pub async fn batch<S>(make: impl AsyncFn() -> S)
where
    S: HasBatch + HasCarts + HasInventoryItems + HasOrders + HasCustomers,
    S::Batch: StageCarts + StageInventoryItems + StageOrders + StageCustomers,
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
    scenario("batch: inserts a customer; a taken email writes nothing");
    customers(&make().await).await;
    scenario("batch: an empty batch commits");
    empty_batch(&make().await).await;
}

fn cart(n: u128) -> Cart {
    Cart::new(
        CartId::from_uuid(Uuid::from_u128(n)),
        MarketId::from_uuid(Uuid::from_u128(1000 + n)),
        CountryCode::new("HR").unwrap(),
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
    add_mug(&mut c);
    a.set_tracked(false);
    b.set_continue_selling(true);

    let mut batch = ports.batch();
    batch.update_cart(&mut c);
    batch.update_inventory_item(&mut a);
    batch.update_inventory_item(&mut b);
    assert_eq!(c.version(), Version::new(2));
    assert_eq!(ports.commit(batch).await, Ok(()));

    // What was staged is what is stored, versions included.
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
    let ana = || hypha_core::Email::new("ana@example.hr").unwrap();
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

async fn empty_batch<S: HasBatch>(ports: &S) {
    assert_eq!(ports.commit(ports.batch()).await, Ok(()));
}
