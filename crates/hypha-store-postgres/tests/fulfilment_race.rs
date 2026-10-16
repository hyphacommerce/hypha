//! Concurrent order actions against a real Postgres, each request on its own connection
//! (design note 0018, section 3): fulfilments racing for one line never ship more than
//! was ordered, and a fulfilment racing a cancellation leaves the order either shipped
//! or cancelled, with the stock to match.
//!
//! Needs `DATABASE_URL` like the contract tests; skips itself without it.

use futures_util::future::join_all;
use hypha_core::{
    CartId, CountryCode, Ctx, Currency, CustomerId, Email, InventoryItemId, LocationId, MarketId,
    Money, OrderId, OrderLineId, Price, ProductId, Quantity, RepoError, VariantId,
};
use hypha_fulfillment::FulfillmentRepo;
use hypha_inventory::{InventoryItem, InventoryItemRepo, Location, LocationRepo, ReservationRef};
use hypha_order_actions::{CancelOrder, CommandError, Fulfil, FulfilLine, cancel_order, fulfill};
use hypha_orders::{CancelReason, NewOrder, Order, OrderError, OrderLine, OrderNumber, OrderRepo};
use hypha_store_postgres::{CORE, PgStore, migrate};
use hypha_testkit::{FixedClock, SequentialIds};
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

/// IDs from 1000 up, so they never collide with `SequentialIds` (which counts from 1).
fn order_id() -> OrderId {
    OrderId::from_uuid(Uuid::from_u128(1000))
}

fn mugs() -> OrderLineId {
    OrderLineId::from_uuid(Uuid::from_u128(1001))
}

fn warehouse() -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(1002))
}

fn mug_item() -> InventoryItemId {
    InventoryItemId::from_uuid(Uuid::from_u128(1003))
}

fn clock() -> std::sync::Arc<FixedClock> {
    std::sync::Arc::new(FixedClock::default())
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

/// A fresh schema with 10 mugs at the warehouse and order 1000 for 3 of them, reserved.
async fn shop(url: &str, schema: &str) -> PgStore<Client> {
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

    let market = MarketId::from_uuid(Uuid::from_u128(1004));
    let mut location = Location::new(warehouse(), "Zagreb".to_string(), vec![market], 0).unwrap();
    LocationRepo::insert(&store, &mut location).await.unwrap();

    let eur = |amount| Money::new(amount, Currency::Eur).unwrap();
    let variant = VariantId::from_uuid(Uuid::from_u128(1005));
    let mut order = Order::place(NewOrder {
        id: order_id(),
        number: OrderNumber::new(1001).unwrap(),
        placed_at: FixedClock::DEFAULT,
        cart: CartId::from_uuid(Uuid::from_u128(1006)),
        channel: hypha_core::ChannelId::from_uuid(Uuid::from_u128(500)),
        market,
        currency: Currency::Eur,
        country: CountryCode::new("HR").unwrap(),
        taxes_included: true,
        customer: CustomerId::from_uuid(Uuid::from_u128(1007)),
        email: Email::new("ana@example.hr").unwrap(),
        lines: vec![OrderLine {
            id: mugs(),
            product: ProductId::from_uuid(Uuid::from_u128(1008)),
            variant,
            quantity: Quantity::new(3).unwrap(),
            product_title: "Mug".to_string(),
            variant_title: None,
            sku: None,
            unit_price: Price::new(eur(1000)).unwrap(),
            total: eur(3000),
            tax: None,
            fulfilled_quantity: 0,
        }],
        shipping_address: None,
        shipping_lines: vec![],
        subtotal: eur(3000),
        shipping_total: eur(0),
        tax_total: eur(0),
        total: eur(3000),
    })
    .unwrap();
    OrderRepo::insert(&store, &mut order).await.unwrap();

    let mut item = InventoryItem::new(mug_item(), variant, true, false);
    item.set_on_hand(warehouse(), 10, None).unwrap();
    let reference = ReservationRef::new(&mugs().to_string()).unwrap();
    item.reserve(&reference, Quantity::new(3).unwrap(), &[location])
        .unwrap();
    InventoryItemRepo::insert(&store, &mut item).await.unwrap();
    store
}

/// `n` stores, each on its own connection, so their transactions really overlap.
async fn staff(url: &str, schema: &str, n: usize) -> Vec<PgStore<Client>> {
    let mut stores = Vec::new();
    for _ in 0..n {
        stores.push(PgStore::new(connect(url, schema).await, clock()));
    }
    stores
}

fn one_mug() -> Fulfil {
    Fulfil {
        order: order_id(),
        location: warehouse(),
        lines: vec![FulfilLine {
            line: mugs(),
            quantity: 1,
        }],
        tracking: vec![],
        idempotency: None,
    }
}

/// (fulfilled on the line, standing fulfilments, on hand, reserved for the line).
async fn state(store: &PgStore<Client>) -> (u32, usize, i64, i64) {
    let order = OrderRepo::get(store, order_id()).await.unwrap().unwrap();
    let standing = store
        .list_for_order(order_id())
        .await
        .unwrap()
        .iter()
        .filter(|f| f.stands())
        .count();
    let item = InventoryItemRepo::get(store, mug_item())
        .await
        .unwrap()
        .unwrap();
    (
        order.lines()[0].fulfilled_quantity,
        standing,
        item.on_hand(warehouse()),
        item.reserved(warehouse()),
    )
}

#[tokio::test]
async fn fulfilments_racing_for_one_line_never_ship_more_than_was_ordered() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping the fulfilment race test");
        return;
    };
    const SCHEMA: &str = "fulfilment_race_line";
    let setup = shop(&url, SCHEMA).await;
    let (ids, clock) = (SequentialIds::default(), FixedClock::default());

    // 6 people each ship 1 of the 3 mugs at the same moment.
    let stores = staff(&url, SCHEMA, 6).await;
    let ctx = Ctx::system();
    let results = join_all(
        stores
            .iter()
            .map(|store| fulfill(&ctx, store, &ids, &clock, one_mug())),
    )
    .await;

    for r in &results {
        // Losers saw nothing left to ship, or kept losing the race: never anything else.
        assert!(
            matches!(
                r,
                Ok(_)
                    | Err(CommandError::Order(OrderError::OverFulfilled { .. }))
                    | Err(CommandError::Repo(RepoError::StaleVersion))
            ),
            "unexpected result {r:?}"
        );
    }
    let won = results.iter().filter(|r| r.is_ok()).count();
    assert!((1..=3).contains(&won), "{won} fulfilments of 3 mugs");
    assert_eq!(
        state(&setup).await,
        (won as u32, won, 10 - won as i64, 3 - won as i64),
        "the order, the fulfilments and the stock agree"
    );
    eprintln!("{won} of 6 fulfilments won");
}

#[tokio::test]
async fn a_fulfilment_racing_a_cancellation_leaves_one_or_the_other() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping the fulfilment race test");
        return;
    };
    let (ids, clock) = (SequentialIds::default(), FixedClock::default());
    let ctx = Ctx::system();
    // A few rounds, so either side gets to win at least sometimes.
    for round in 0..4 {
        let schema = format!("fulfilment_race_cancel_{round}");
        let setup = shop(&url, &schema).await;
        let stores = staff(&url, &schema, 2).await;
        let cancel = CancelOrder {
            order: order_id(),
            reason: CancelReason::Customer,
        };
        let (shipped, cancelled) = tokio::join!(
            fulfill(&ctx, &stores[0], &ids, &clock, one_mug()),
            cancel_order(&ctx, &stores[1], &clock, cancel),
        );
        let order = OrderRepo::get(&setup, order_id()).await.unwrap().unwrap();
        match order.cancelled() {
            // Cancelled: nothing shipped, every unit back, the fulfilment refused.
            Some(_) => {
                assert_eq!(state(&setup).await, (0, 0, 10, 0));
                assert!(
                    matches!(shipped, Err(CommandError::Order(OrderError::Cancelled(_)))),
                    "{shipped:?}"
                );
            }
            // Shipped first: the cancellation was refused, one unit is gone.
            None => {
                assert_eq!(state(&setup).await, (1, 1, 9, 2));
                assert!(
                    matches!(cancelled, Err(CommandError::HasFulfillments { .. })),
                    "{cancelled:?}"
                );
            }
        }
    }
}
