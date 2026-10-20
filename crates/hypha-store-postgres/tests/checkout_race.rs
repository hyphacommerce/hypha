//! Concurrent checkouts against a real Postgres, each request on its own connection
//! (design note 0007, section 2): carts racing for the last units never oversell, a cart
//! checked out twice at once becomes one order, and retries with the same idempotency
//! key all get that one order back (and, the same way, add a line only once).
//!
//! Needs `DATABASE_URL` like the contract tests; skips itself without it.

use futures_util::future::join_all;
use hypha_cart::{AddLine, CreateCart, add_line, create_cart, set_cart_email};
use hypha_channels::{Channel, ChannelKind, ChannelRepo};
use hypha_checkout::{Checkout, CommandError, checkout};
use hypha_core::{
    Actor, CatalogId, ChannelId, Ctx, Currency, IdempotencyRequest, InventoryItemId, LocationId,
    MarketId, Money, Price, ProductId, RepoError, Scope, VariantId,
};
use hypha_customers::repo::keys as customer_keys;
use hypha_inventory::{InventoryItem, InventoryItemRepo, Location, LocationRepo};
use hypha_markets::{Catalog, CatalogContext, CatalogRepo, Market, MarketRepo};
use hypha_payments::{
    ManualProvider, NewPaymentMethod, PaymentMethod, PaymentMethodRepo, ProviderKey,
};
use hypha_products::{Handle, Product, ProductRepo, ProductStatus};
use hypha_store_postgres::{CORE, PgStore, migrate};
use hypha_testkit::{FixedClock, NoRateProviders, SequentialIds};
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

/// IDs from 1000 up, so they never collide with `SequentialIds` (which counts from 1).
fn market() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(1000))
}

fn warehouse() -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(1001))
}

fn mug() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(1002))
}

fn mug_item() -> InventoryItemId {
    InventoryItemId::from_uuid(Uuid::from_u128(1003))
}

fn online_store() -> ChannelId {
    ChannelId::from_uuid(Uuid::from_u128(1006))
}

async fn connect(url: &str, schema: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("connect to DATABASE_URL");
    tokio::spawn(connection);
    client
        .batch_execute(&format!("SET search_path TO {schema}"))
        .await
        .expect("set search_path");
    client
}

/// A fresh schema holding an EUR market for Croatia whose catalog has an active mug
/// (10.00) with `on_hand` units at the warehouse. Returns a store for setup and a plain
/// client for checking rows.
async fn shop(url: &str, schema: &str, on_hand: i64) -> (PgStore<Client>, Client) {
    let mut client = connect(url, "public").await;
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; \
             SET search_path TO {schema};"
        ))
        .await
        .expect("reset schema");
    migrate(&mut client, &[CORE]).await.expect("migrate");
    let store = PgStore::new(client, clock());

    let hr = hypha_core::CountryCode::new("HR").unwrap();
    let mut m = Market::new(market(), "Croatia".to_string(), vec![hr], Currency::EUR).unwrap();
    MarketRepo::insert(&store, &mut m).await.unwrap();

    let product_id = ProductId::from_uuid(Uuid::from_u128(1004));
    let price = Price::new(Money::new(1000, Currency::EUR).unwrap()).unwrap();
    let mut product = Product::new_simple(
        product_id,
        Handle::new("mug").unwrap(),
        "Mug".to_string(),
        mug(),
        price,
    )
    .unwrap();
    // The race is about stock: no shipping needed.
    product
        .update_variant(
            mug(),
            hypha_products::VariantPatch {
                requires_shipping: Some(false),
                ..hypha_products::VariantPatch::default()
            },
        )
        .unwrap();
    product.set_status(ProductStatus::Active).unwrap();
    ProductRepo::insert(&store, &mut product).await.unwrap();

    let catalog_id = CatalogId::from_uuid(Uuid::from_u128(1005));
    let mut catalog = Catalog::new(catalog_id, CatalogContext::Market(market()), None);
    CatalogRepo::insert(&store, &mut catalog).await.unwrap();
    CatalogRepo::add_products(&store, catalog_id, &[product_id])
        .await
        .unwrap();

    let mut channel = Channel::new(online_store(), "Web", ChannelKind::OnlineStore).unwrap();
    ChannelRepo::insert(&store, &mut channel).await.unwrap();
    ChannelRepo::publish(&store, online_store(), &[product_id])
        .await
        .unwrap();

    let mut location = Location::new(warehouse(), "Zagreb".to_string(), vec![market()], 0).unwrap();
    LocationRepo::insert(&store, &mut location).await.unwrap();
    let mut item = InventoryItem::new(mug_item(), mug(), true, false);
    item.set_on_hand(warehouse(), on_hand, None).unwrap();
    InventoryItemRepo::insert(&store, &mut item).await.unwrap();

    let mut method = PaymentMethod::new(NewPaymentMethod {
        id: cash_on_delivery(),
        provider: ProviderKey::manual(),
        name: "Cash on delivery".to_string(),
        instructions: None,
        markets: vec![],
        channels: vec![],
        enabled: true,
        position: 0,
        settings: serde_json::json!({}),
    })
    .unwrap();
    PaymentMethodRepo::insert(&store, &mut method)
        .await
        .unwrap();

    (store, connect(url, schema).await)
}

/// A cart for `email` holding one mug.
async fn cart(
    store: &PgStore<Client>,
    ids: &SequentialIds,
    clock: &FixedClock,
    email: &str,
) -> hypha_core::CartId {
    let ctx = Ctx::in_channel(Actor::System, Scope::ALL.to_vec(), online_store());
    let input = CreateCart {
        country: "HR".to_string(),
        idempotency: None,
    };
    let cart = create_cart(&ctx, store, ids, clock, input)
        .await
        .unwrap()
        .value
        .id;
    let line = AddLine {
        cart,
        variant: mug(),
        quantity: 1,
        idempotency: None,
    };
    add_line(&ctx, store, clock, line).await.unwrap();
    set_cart_email(&ctx, store, cart, Some(email.to_string()))
        .await
        .unwrap();
    hypha_checkout::set_payment_method(
        &ctx,
        store,
        &(ManualProvider,),
        cart,
        Some(cash_on_delivery()),
    )
    .await
    .unwrap();
    cart
}

fn cash_on_delivery() -> hypha_core::PaymentMethodId {
    hypha_core::PaymentMethodId::from_uuid(Uuid::from_u128(1006))
}

/// `n` stores, each on its own connection, so their transactions really overlap.
async fn buyers(url: &str, schema: &str, n: usize) -> Vec<PgStore<Client>> {
    let mut stores = Vec::new();
    for _ in 0..n {
        stores.push(PgStore::new(connect(url, schema).await, clock()));
    }
    stores
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

async fn mug_reservations(store: &PgStore<Client>) -> usize {
    InventoryItemRepo::get(store, mug_item())
        .await
        .unwrap()
        .unwrap()
        .reservations()
        .len()
}

#[tokio::test]
async fn carts_racing_for_the_last_units_never_oversell() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping the checkout race test");
        return;
    };
    const SCHEMA: &str = "checkout_race_stock";
    let (setup, rows) = shop(&url, SCHEMA, 3).await;
    let (ids, clock) = (SequentialIds::default(), FixedClock::default());

    // 8 carts, all for the same guest, each wanting 1 of the 3 mugs.
    let mut carts = Vec::new();
    for _ in 0..8 {
        carts.push(cart(&setup, &ids, &clock, "ana@example.hr").await);
    }
    let stores = buyers(&url, SCHEMA, 8).await;
    let ctx = Ctx::system();
    let results = join_all(stores.iter().zip(&carts).map(|(store, &cart)| {
        let input = Checkout {
            cart,
            expected_total: None,
            return_url: None,
            idempotency: None,
        };
        checkout(
            &ctx,
            store,
            &ids,
            &clock,
            &NoRateProviders,
            &(ManualProvider,),
            input,
        )
    }))
    .await;

    for r in &results {
        // Losers saw the mugs gone, or kept losing the race: never anything else.
        assert!(
            matches!(
                r,
                Ok(_)
                    | Err(CommandError::Unavailable { .. })
                    | Err(CommandError::Repo(RepoError::StaleVersion))
                    | Err(CommandError::Repo(RepoError::Duplicate(
                        customer_keys::CUSTOMER_EMAIL
                    )))
            ),
            "unexpected result {r:?}"
        );
    }
    let won = results.iter().filter(|r| r.is_ok()).count();
    assert!(won >= 1, "nobody checked out: {results:?}");
    assert!(won <= 3, "oversold: {won} carts got 3 mugs");
    assert_eq!(
        count(&rows, "SELECT count(*) FROM orders").await,
        won as i64
    );
    assert_eq!(
        mug_reservations(&setup).await,
        won,
        "one reservation per order"
    );
    assert_eq!(
        count(&rows, "SELECT count(DISTINCT number) FROM orders").await,
        won as i64,
        "order numbers are unique"
    );
    assert_eq!(
        count(
            &rows,
            "SELECT count(*) FROM carts WHERE order_id IS NOT NULL"
        )
        .await,
        won as i64,
        "exactly the winners' carts are completed"
    );
    assert_eq!(
        count(&rows, "SELECT count(*) FROM customers").await,
        1,
        "the guest became one customer, not one per order"
    );
    eprintln!("{won} of 8 carts checked out; results: {results:?}");
}

#[tokio::test]
async fn one_cart_checked_out_at_once_becomes_one_order() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping the checkout race test");
        return;
    };
    const SCHEMA: &str = "checkout_race_cart";
    let (setup, rows) = shop(&url, SCHEMA, 10).await;
    let (ids, clock) = (SequentialIds::default(), FixedClock::default());
    let cart = cart(&setup, &ids, &clock, "ana@example.hr").await;

    // A double-click without an idempotency key: 4 checkouts of the same cart.
    let stores = buyers(&url, SCHEMA, 4).await;
    let ctx = Ctx::system();
    let results = join_all(stores.iter().map(|store| {
        let input = Checkout {
            cart,
            expected_total: None,
            return_url: None,
            idempotency: None,
        };
        checkout(
            &ctx,
            store,
            &ids,
            &clock,
            &NoRateProviders,
            &(ManualProvider,),
            input,
        )
    }))
    .await;

    // One checkout places the order; the others are answered with it (design note
    // 0031: the shopper's browser and the provider's webhook may both complete a cart),
    // or ran out of retries.
    let placed: Vec<_> = results
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .filter(|p| !p.replayed)
        .collect();
    assert_eq!(placed.len(), 1, "exactly one checkout wins: {results:?}");
    let order = placed[0].value.order.id();
    for r in &results {
        assert!(
            match r {
                Ok(p) => p.value.order.id() == order,
                Err(CommandError::Repo(RepoError::StaleVersion)) => true,
                _ => false,
            },
            "unexpected result {r:?}"
        );
    }
    assert_eq!(count(&rows, "SELECT count(*) FROM orders").await, 1);
    assert_eq!(mug_reservations(&setup).await, 1, "reserved once");
}

#[tokio::test]
async fn retries_with_the_same_key_all_get_the_one_order() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping the checkout race test");
        return;
    };
    const SCHEMA: &str = "checkout_race_key";
    let (setup, rows) = shop(&url, SCHEMA, 10).await;
    let (ids, clock) = (SequentialIds::default(), FixedClock::default());
    let cart = cart(&setup, &ids, &clock, "ana@example.hr").await;

    // A client that times out and retries with the same key while the first request
    // is still running: every response must be that one order.
    let stores = buyers(&url, SCHEMA, 4).await;
    let ctx = Ctx::system();
    let results = join_all(stores.iter().map(|store| {
        let input = Checkout {
            cart,
            expected_total: None,
            return_url: None,
            idempotency: Some(IdempotencyRequest {
                key: "checkout-1".to_string(),
                fingerprint: [7; 32],
            }),
        };
        checkout(
            &ctx,
            store,
            &ids,
            &clock,
            &NoRateProviders,
            &(ManualProvider,),
            input,
        )
    }))
    .await;

    let placed: Vec<_> = results
        .iter()
        .map(|r| {
            r.as_ref()
                .unwrap_or_else(|e| panic!("got {e:?}: {results:?}"))
        })
        .collect();
    let fresh: Vec<_> = placed.iter().filter(|p| !p.replayed).collect();
    assert_eq!(fresh.len(), 1, "exactly one placed the order: {results:?}");
    let order = fresh[0].value.order.id();
    assert!(placed.iter().all(|p| p.value.order.id() == order));
    assert_eq!(count(&rows, "SELECT count(*) FROM orders").await, 1);
    assert_eq!(mug_reservations(&setup).await, 1, "reserved once");
}

#[tokio::test]
async fn adding_a_line_retried_with_the_same_key_adds_it_once() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping the checkout race test");
        return;
    };
    const SCHEMA: &str = "checkout_race_add_line";
    let (setup, _rows) = shop(&url, SCHEMA, 10).await;
    let (ids, clock) = (SequentialIds::default(), FixedClock::default());
    let cart = cart(&setup, &ids, &clock, "ana@example.hr").await;

    // The cart holds 1 mug; 4 copies of "add 2 more" with one key race to update it.
    let stores = buyers(&url, SCHEMA, 4).await;
    let ctx = Ctx::system();
    let results = join_all(stores.iter().map(|store| {
        let input = AddLine {
            cart,
            variant: mug(),
            quantity: 2,
            idempotency: Some(IdempotencyRequest {
                key: "add-1".to_string(),
                fingerprint: [9; 32],
            }),
        };
        add_line(&ctx, store, &clock, input)
    }))
    .await;

    let added: Vec<_> = results
        .iter()
        .map(|r| {
            r.as_ref()
                .unwrap_or_else(|e| panic!("got {e:?}: {results:?}"))
        })
        .collect();
    assert_eq!(
        added.iter().filter(|a| !a.replayed).count(),
        1,
        "exactly one added the line: {results:?}"
    );
    let stored = hypha_cart::CartRepo::get(&setup, cart)
        .await
        .unwrap()
        .unwrap();
    let quantities: Vec<u32> = stored
        .lines()
        .iter()
        .map(|l| l.quantity().value())
        .collect();
    assert_eq!(quantities, vec![3]);
}

/// Writes are stamped with a fixed time in these tests.
fn clock() -> std::sync::Arc<hypha_testkit::FixedClock> {
    std::sync::Arc::new(hypha_testkit::FixedClock::default())
}
