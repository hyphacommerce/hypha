use hypha_core::{
    Actor, AuthError, CartId, CountryCode, Ctx, Currency, CustomerId, Email, MarketId, Money,
    OrderId, OrderLineId, Page, PageRequest, Price, ProductId, Quantity, Scope, Timestamp, UserId,
    VariantId, Version, Versioned,
};
use hypha_orders::{
    CancelReason, CommandError, FinancialStatus, NewOrder, Order, OrderError, OrderLine,
    OrderNumber, OrderRepo, OrderSummary, get_order, list_orders, mark_order_paid,
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
fn mark_order_paid_stores_the_payment() {
    let repo = repo_with_order();
    let paid = block_on(mark_order_paid(&Ctx::system(), &repo, order_id())).unwrap();
    assert_eq!(paid.financial_status(), FinancialStatus::Paid);
    assert_eq!(block_on(repo.get(order_id())), Ok(Some(paid)));
}

#[test]
fn marking_a_paid_order_again_writes_nothing() {
    let repo = repo_with_order();
    block_on(mark_order_paid(&Ctx::system(), &repo, order_id())).unwrap();
    let again = block_on(mark_order_paid(&Ctx::system(), &repo, order_id())).unwrap();
    assert_eq!(again.version(), Version::new(2));
    // Placed, then paid once.
    assert_eq!(repo.outbox().types(), ["order.placed", "order.paid"]);
}

#[test]
fn mark_order_paid_needs_orders_write() {
    let repo = repo_with_order();
    let ctx = user_with(vec![Scope::OrdersRead]);
    assert_eq!(
        block_on(mark_order_paid(&ctx, &repo, order_id())),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::OrdersWrite
        )))
    );
}

/// An order repo whose first `get` returns `stale`: the order as this request loaded it,
/// before another request changed it.
struct LoadedBeforeAChange {
    repo: InMemoryOrderRepo,
    stale: std::cell::RefCell<Option<Order>>,
}

impl OrderRepo for LoadedBeforeAChange {
    async fn get(&self, id: OrderId) -> Result<Option<Order>, hypha_core::RepoError> {
        // Taken in its own statement: a temporary in a `match` scrutinee lives until the
        // `match` ends, so the borrow would be held across the `.await` below.
        let stale = self.stale.borrow_mut().take();
        match stale {
            Some(stale) => Ok(Some(stale)),
            None => self.repo.get(id).await,
        }
    }
    async fn get_by_number(
        &self,
        number: OrderNumber,
    ) -> Result<Option<Order>, hypha_core::RepoError> {
        self.repo.get_by_number(number).await
    }
    async fn list(
        &self,
        page: PageRequest<OrderId>,
    ) -> Result<Page<OrderSummary>, hypha_core::RepoError> {
        self.repo.list(page).await
    }
    async fn insert(&self, order: &mut Order) -> Result<(), hypha_core::RepoError> {
        self.repo.insert(order).await
    }
    async fn update(&self, order: &mut Order) -> Result<(), hypha_core::RepoError> {
        self.repo.update(order).await
    }
    async fn next_number(&self) -> Result<OrderNumber, hypha_core::RepoError> {
        self.repo.next_number().await
    }
}

#[test]
fn marking_an_order_paid_twice_at_once_succeeds_both_times() {
    let repo = repo_with_order();
    let stale = block_on(repo.get(order_id())).unwrap();
    // The other request wins and marks it paid first.
    block_on(mark_order_paid(&Ctx::system(), &repo, order_id())).unwrap();

    let racing = LoadedBeforeAChange {
        repo,
        stale: std::cell::RefCell::new(stale),
    };
    let order = block_on(mark_order_paid(&Ctx::system(), &racing, order_id())).unwrap();
    assert_eq!(order.financial_status(), FinancialStatus::Paid);
    assert_eq!(order.version(), Version::new(2));
}

#[test]
fn list_orders_returns_a_page() {
    let repo = repo_with_order();
    let page = block_on(list_orders(&Ctx::system(), &repo, PageRequest::default())).unwrap();
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
fn list_orders_needs_orders_read() {
    let repo = repo_with_order();
    let ctx = user_with(vec![Scope::Checkout]);
    assert_eq!(
        block_on(list_orders(&ctx, &repo, PageRequest::default())),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::OrdersRead
        )))
    );
}

#[test]
fn a_cancelled_order_cant_be_marked_paid() {
    let repo = repo_with_order();
    let mut order = block_on(repo.get(order_id())).unwrap().unwrap();
    order
        .cancel(
            Timestamp::from_unix_micros(1_767_312_000_000_000),
            CancelReason::Fraud,
        )
        .unwrap();
    block_on(repo.update(&mut order)).unwrap();
    assert_eq!(
        block_on(mark_order_paid(&Ctx::system(), &repo, order_id())),
        Err(CommandError::Order(OrderError::Cancelled(order_id())))
    );
}
