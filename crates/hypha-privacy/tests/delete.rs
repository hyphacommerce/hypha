//! Deleting a customer (design note 0032, CD7, §7.2).

use hypha_cart::{Cart, CartRepo};
use hypha_core::{
    Actor, AuthError, CartId, CountryCode, Ctx, Currency, CustomerId, Email, MarketId, Money,
    OrderId, OrderLineId, Price, ProductId, Quantity, Scope, UserId, VariantId,
};
use hypha_customers::{Customer, CustomerRepo};
use hypha_orders::{CancelReason, NewOrder, Order, OrderLine, OrderNumber, OrderRepo};
use hypha_privacy::{CommandError, delete_customer};
use hypha_testkit::{FixedClock, InMemoryPorts};
use pollster::block_on;
use uuid::Uuid;

fn ana_id() -> CustomerId {
    CustomerId::from_uuid(Uuid::from_u128(1))
}

fn staff() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::CustomersWrite],
    )
}

fn world() -> InMemoryPorts {
    let ports = InMemoryPorts::default();
    let mut ana = Customer::new(ana_id(), Email::new("ana@example.hr").unwrap());
    block_on(ports.customers.insert(&mut ana)).unwrap();
    ports
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::EUR).unwrap()
}

fn order_for(ana: CustomerId, n: u128) -> Order {
    Order::place(NewOrder {
        id: OrderId::from_uuid(Uuid::from_u128(1000 + n)),
        number: OrderNumber::new(1000 + n as i64).unwrap(),
        placed_at: FixedClock::DEFAULT,
        cart: CartId::from_uuid(Uuid::from_u128(6000 + n)),
        channel: hypha_core::ChannelId::from_uuid(Uuid::from_u128(500)),
        market: MarketId::from_uuid(Uuid::from_u128(5001)),
        currency: Currency::EUR,
        country: CountryCode::new("HR").unwrap(),
        taxes_included: true,
        customer: ana,
        email: Email::new("ana@example.hr").unwrap(),
        lines: vec![OrderLine {
            id: OrderLineId::from_uuid(Uuid::from_u128(2000 + n)),
            product: ProductId::from_uuid(Uuid::from_u128(100)),
            variant: VariantId::from_uuid(Uuid::from_u128(101)),
            quantity: Quantity::new(1).unwrap(),
            product_title: "Mug".to_string(),
            variant_title: None,
            sku: None,
            unit_price: Price::new(eur(500)).unwrap(),
            total: eur(500),
            tax: None,
            fulfilled_quantity: 0,
        }],
        shipping_address: None,
        shipping_lines: vec![],
        subtotal: eur(500),
        shipping_total: eur(0),
        tax_total: eur(0),
        total: eur(500),
    })
    .unwrap()
}

fn cart_of(ana: CustomerId, n: u128) -> Cart {
    let mut cart = Cart::new(
        CartId::from_uuid(Uuid::from_u128(6000 + n)),
        MarketId::from_uuid(Uuid::from_u128(5001)),
        CountryCode::new("HR").unwrap(),
        hypha_core::ChannelId::from_uuid(Uuid::from_u128(500)),
    );
    cart.set_customer(Some((ana, Email::new("ana@example.hr").unwrap())))
        .unwrap();
    cart
}

#[test]
fn deletes_a_customer_without_orders_and_unlinks_its_carts() {
    let ports = world();
    let mut cart = cart_of(ana_id(), 1);
    block_on(ports.carts.insert(&mut cart)).unwrap();

    assert_eq!(
        block_on(delete_customer(&staff(), &ports, ana_id())),
        Ok(())
    );
    assert_eq!(block_on(ports.customers.get(ana_id())), Ok(None));
    let unlinked = block_on(ports.carts.get(cart.id())).unwrap().unwrap();
    assert_eq!(unlinked.customer(), None);
    assert_eq!(
        unlinked.email().map(Email::as_str),
        Some("ana@example.hr"),
        "the cart keeps its email: it's the shopper's own"
    );
    let events = ports.outbox.events();
    let deleted = events
        .iter()
        .find(|e| e.event_type == "customer.deleted")
        .expect("customer.deleted recorded");
    assert_eq!(deleted.data["customer"], ana_id().to_string());
}

#[test]
fn a_customer_with_orders_is_refused_even_when_they_are_cancelled() {
    let ports = world();
    let mut order = order_for(ana_id(), 1);
    order
        .cancel(FixedClock::DEFAULT, CancelReason::Customer, false)
        .unwrap();
    block_on(ports.orders.insert(&mut order)).unwrap();
    assert_eq!(
        block_on(delete_customer(&staff(), &ports, ana_id())),
        Err(CommandError::CustomerHasOrders {
            customer: ana_id(),
            orders: 1
        })
    );
    assert!(block_on(ports.customers.get(ana_id())).unwrap().is_some());
}

#[test]
fn needs_customers_write_and_an_existing_customer() {
    let ports = world();
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::CustomersRead],
    );
    assert_eq!(
        block_on(delete_customer(&reader, &ports, ana_id())),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::CustomersWrite
        )))
    );
    let nobody = CustomerId::from_uuid(Uuid::from_u128(77));
    assert_eq!(
        block_on(delete_customer(&staff(), &ports, nobody)),
        Err(CommandError::CustomerNotFound(nobody))
    );
}
