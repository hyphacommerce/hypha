use hypha_core::{
    Actor, AuthError, CartId, CountryCode, Ctx, Currency, CustomerId, Email, MarketId, Money,
    OrderId, OrderLineId, Page, PageRequest, Price, ProductId, Quantity, Scope, Timestamp, UserId,
    VariantId,
};
use hypha_orders::{
    CommandError, ListOrders, NewOrder, Order, OrderLine, OrderNumber, OrderRepo, get_order,
    get_placed_order, list_orders,
};
use hypha_testkit::InMemoryOrderRepo;
use pollster::block_on;
use uuid::Uuid;

fn order_id() -> OrderId {
    OrderId::from_uuid(Uuid::from_u128(1))
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::EUR).unwrap()
}

/// A repo holding one pending order: 2 mugs at 5.00, untaxed.
fn repo_with_order() -> InMemoryOrderRepo {
    let repo = InMemoryOrderRepo::default();
    let mut order = Order::place(NewOrder {
        id: order_id(),
        number: OrderNumber::new(1001).unwrap(),
        placed_at: Timestamp::from_unix_micros(1_767_225_600_000_000),
        cart: CartId::from_uuid(Uuid::from_u128(2)),
        channel: hypha_core::ChannelId::from_uuid(Uuid::from_u128(500)),
        market: MarketId::from_uuid(Uuid::from_u128(3)),
        currency: Currency::EUR,
        country: CountryCode::new("HR").unwrap(),
        taxes_included: false,
        customer: CustomerId::from_uuid(Uuid::from_u128(4)),
        email: Email::new("ana@example.hr").unwrap(),
        lines: vec![OrderLine {
            id: OrderLineId::from_uuid(Uuid::from_u128(5)),
            product: ProductId::from_uuid(Uuid::from_u128(6)),
            variant: VariantId::from_uuid(Uuid::from_u128(7)),
            quantity: Quantity::new(2).unwrap(),
            product_title: "Mug".to_string(),
            variant_title: None,
            sku: None,
            unit_price: Price::new(eur(500)).unwrap(),
            total: eur(1000),
            tax: None,
            fulfilled_quantity: 0,
        }],
        shipping_address: None,
        shipping_lines: vec![],
        subtotal: eur(1000),
        shipping_total: eur(0),
        tax_total: eur(0),
        total: eur(1000),
    })
    .unwrap();
    block_on(repo.insert(&mut order)).unwrap();
    repo
}

fn user_with(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(9))), scopes)
}

#[test]
fn get_order_returns_the_order() {
    let repo = repo_with_order();
    let order = block_on(get_order(&Ctx::system(), &repo, order_id())).unwrap();
    assert_eq!(order.number(), OrderNumber::new(1001).unwrap());
}

#[test]
fn get_order_reports_an_unknown_order() {
    let repo = InMemoryOrderRepo::default();
    assert_eq!(
        block_on(get_order(&Ctx::system(), &repo, order_id())),
        Err(CommandError::OrderNotFound(order_id()))
    );
}

#[test]
fn get_order_needs_orders_read() {
    let repo = repo_with_order();
    // Not `orders:write`: that implies `orders:read`.
    let ctx = user_with(vec![Scope::Checkout]);
    assert_eq!(
        block_on(get_order(&ctx, &repo, order_id())),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::OrdersRead
        )))
    );
}

#[test]
fn list_orders_returns_a_page() {
    let repo = repo_with_order();
    let page = block_on(list_orders(
        &Ctx::system(),
        &repo,
        ListOrders::default(),
        PageRequest::default(),
    ))
    .unwrap();
    let stored = block_on(repo.get(order_id())).unwrap().unwrap();
    assert_eq!(
        page,
        Page {
            items: vec![stored.summary()],
            has_more: false
        }
    );
}

#[test]
fn a_buyer_reads_a_placed_order_in_its_own_channel_only() {
    let repo = repo_with_order();
    let key = |channel: u128| {
        Ctx::in_channel(
            Actor::System,
            vec![Scope::Checkout],
            hypha_core::ChannelId::from_uuid(Uuid::from_u128(channel)),
        )
    };
    let order = block_on(get_placed_order(&key(500), &repo, order_id())).unwrap();
    assert_eq!(order.id(), order_id());
    assert_eq!(
        block_on(get_placed_order(&key(501), &repo, order_id())).map(|o| o.id()),
        Err(CommandError::OrderNotFound(order_id())),
        "another channel's order is as if absent"
    );
    assert_eq!(
        block_on(get_placed_order(
            &user_with(vec![Scope::OrdersRead]),
            &repo,
            order_id()
        ))
        .map(|o| o.id()),
        Err(CommandError::Auth(AuthError::MissingScope(Scope::Checkout)))
    );
}

#[test]
fn list_orders_needs_orders_read() {
    let repo = repo_with_order();
    let ctx = user_with(vec![Scope::Checkout]);
    assert_eq!(
        block_on(list_orders(
            &ctx,
            &repo,
            ListOrders::default(),
            PageRequest::default()
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::OrdersRead
        )))
    );
}
