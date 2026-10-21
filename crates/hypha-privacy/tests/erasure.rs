//! Erasing a customer's personal data and exporting it (design note 0032, §7.3, §7.4).

use std::time::Duration;

use hypha_cart::{Cart, CartRepo};
use hypha_core::{
    Actor, AuthError, CartId, Clock, CountryCode, Ctx, Currency, CustomerId, Email, MarketId,
    Money, OrderId, OrderLineId, Price, ProductId, Quantity, Scope, UserId, VariantId,
};
use hypha_customers::{
    Channel, ConsentChange, ConsentSource, ConsentState, Customer, CustomerError, CustomerRepo,
    OptInLevel,
};
use hypha_orders::{NewOrder, Order, OrderLine, OrderNumber, OrderRepo};
use hypha_privacy::{
    CommandError, PrivacySettings, cancel_erasure, erase_customer, erase_due, export_customer,
    request_erasure,
};
use hypha_testkit::{FixedClock, InMemoryPorts};
use pollster::block_on;
use uuid::Uuid;

fn ana_id() -> CustomerId {
    CustomerId::from_uuid(Uuid::from_u128(1))
}

fn eraser() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::CustomersErase, Scope::CustomersRead],
    )
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::EUR).unwrap()
}

fn address() -> hypha_core::Address {
    hypha_core::Address::new(hypha_core::AddressParts {
        first_name: Some("Ana".to_string()),
        last_name: "Horvat".to_string(),
        company: Some("Hypha d.o.o.".to_string()),
        line1: "Ilica 1".to_string(),
        line2: None,
        city: "Zagreb".to_string(),
        province: None,
        postal_code: Some("10000".to_string()),
        country: "HR".to_string(),
        phone: None,
    })
    .unwrap()
}

fn order_for(ana: CustomerId, n: u128, cart: CartId) -> Order {
    Order::place(NewOrder {
        id: OrderId::from_uuid(Uuid::from_u128(1000 + n)),
        number: OrderNumber::new(1000 + n as i64).unwrap(),
        placed_at: FixedClock::DEFAULT,
        cart,
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
        shipping_address: Some(address()),
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
    cart.set_shipping_address(Some(address())).unwrap();
    cart
}

/// Ana with a name, a phone, an address, an email consent, a placed order (and the cart
/// it came from) and an open cart.
fn world() -> (InMemoryPorts, FixedClock) {
    let ports = InMemoryPorts::default();
    let clock = FixedClock::default();
    let mut ana = Customer::new(ana_id(), Email::new("ana@example.hr").unwrap());
    ana.set_first_name(Some("Ana".to_string())).unwrap();
    ana.set_phone(Some("+385 91".to_string())).unwrap();
    ana.add_address(
        hypha_core::AddressId::from_uuid(Uuid::from_u128(77)),
        address(),
        true,
        false,
    )
    .unwrap();
    ana.set_marketing_consent(
        Channel::Email,
        ConsentChange {
            state: ConsentState::Subscribed,
            opt_in_level: OptInLevel::Single,
            consented_at: None,
        },
        ConsentSource::Checkout,
        clock.now(),
    )
    .unwrap();
    block_on(ports.customers.insert(&mut ana)).unwrap();

    let mut placed = cart_of(ana_id(), 1);
    placed
        .complete(OrderId::from_uuid(Uuid::from_u128(1001)))
        .unwrap();
    block_on(ports.carts.insert(&mut placed)).unwrap();
    let mut order = order_for(ana_id(), 1, placed.id());
    block_on(ports.orders.insert(&mut order)).unwrap();
    let mut open = cart_of(ana_id(), 2);
    block_on(ports.carts.insert(&mut open)).unwrap();
    (ports, clock)
}

#[test]
fn an_erasure_is_requested_after_the_window_and_can_be_cancelled() {
    let (ports, clock) = world();
    let settings = PrivacySettings {
        erasure_delay: Duration::from_secs(3 * 86_400),
    };
    let ana = block_on(request_erasure(
        &eraser(),
        &ports,
        &clock,
        &settings,
        ana_id(),
    ))
    .unwrap();
    assert_eq!(
        ana.erasure_due_at(),
        Some(clock.now().plus(Duration::from_secs(3 * 86_400)))
    );
    assert_eq!(
        block_on(request_erasure(
            &eraser(),
            &ports,
            &clock,
            &settings,
            ana_id()
        )),
        Err(CommandError::Customer(
            CustomerError::ErasureAlreadyRequested
        ))
    );
    // Not due yet: the job does nothing.
    assert_eq!(block_on(erase_due(&ports, &clock, 10)).unwrap().done, 0);
    let ana = block_on(cancel_erasure(&eraser(), &ports, ana_id())).unwrap();
    assert_eq!(ana.erasure_due_at(), None);
    assert_eq!(
        block_on(cancel_erasure(&eraser(), &ports, ana_id())),
        Err(CommandError::Customer(CustomerError::ErasureNotRequested))
    );
    let types: Vec<String> = ports
        .outbox
        .events()
        .iter()
        .map(|e| e.event_type.clone())
        .collect();
    assert!(types.contains(&"customer.erasure_requested".to_string()));
    assert!(types.contains(&"customer.erasure_cancelled".to_string()));

    let writer = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::CustomersWrite],
    );
    assert_eq!(
        block_on(request_erasure(
            &writer,
            &ports,
            &clock,
            &settings,
            ana_id()
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::CustomersErase
        )))
    );
}

#[test]
fn erasing_removes_the_person_and_keeps_the_orders() {
    let (ports, clock) = world();
    let erased = block_on(erase_customer(&ports, clock.now(), ana_id()))
        .unwrap()
        .expect("erased");
    assert_eq!(
        erased.orders,
        vec![OrderId::from_uuid(Uuid::from_u128(1001))]
    );

    let ana = block_on(ports.customers.get(ana_id())).unwrap().unwrap();
    assert!(ana.is_erased());
    assert!(ana.email().is_placeholder());
    assert_eq!(ana.first_name(), None);
    assert_eq!(ana.phone(), None);
    assert!(ana.addresses().is_empty());
    assert_eq!(
        ana.marketing_consent(Channel::Email).state,
        ConsentState::Redacted
    );
    assert_eq!(
        block_on(
            ports
                .customers
                .get_by_email(&Email::new("ana@example.hr").unwrap())
        )
        .unwrap(),
        None
    );

    let order = block_on(ports.orders.get(OrderId::from_uuid(Uuid::from_u128(1001))))
        .unwrap()
        .unwrap();
    assert!(order.email().is_placeholder());
    assert_eq!(order.shipping_address(), None);
    assert_eq!(order.total(), eur(500), "totals stay");
    assert_eq!(order.lines().len(), 1);

    // The placed cart is redacted, the open one deleted.
    let placed = block_on(ports.carts.get(CartId::from_uuid(Uuid::from_u128(6001))))
        .unwrap()
        .unwrap();
    assert!(placed.email().unwrap().is_placeholder());
    assert_eq!(placed.shipping_address(), None);
    assert_eq!(
        block_on(ports.carts.get(CartId::from_uuid(Uuid::from_u128(6002)))).unwrap(),
        None
    );

    let redacted = ports
        .outbox
        .events()
        .into_iter()
        .find(|e| e.event_type == "customer.redacted")
        .expect("customer.redacted recorded");
    assert_eq!(redacted.data["customer"], ana_id().to_string());
    assert_eq!(redacted.data["orders"][0], erased.orders[0].to_string());

    // Again: nothing to do.
    assert_eq!(
        block_on(erase_customer(&ports, clock.now(), ana_id())).unwrap(),
        None
    );
}

#[test]
fn the_job_erases_what_is_due() {
    let (ports, clock) = world();
    let settings = PrivacySettings {
        erasure_delay: Duration::from_secs(3600),
    };
    block_on(request_erasure(
        &eraser(),
        &ports,
        &clock,
        &settings,
        ana_id(),
    ))
    .unwrap();
    let later = FixedClock::at(clock.now().plus(Duration::from_secs(3601)));
    let progress = block_on(erase_due(&ports, &later, 10)).unwrap();
    assert_eq!(progress.done, 1);
    assert!(!progress.more);
    assert!(
        block_on(ports.customers.get(ana_id()))
            .unwrap()
            .unwrap()
            .is_erased()
    );
    assert_eq!(block_on(erase_due(&ports, &later, 10)).unwrap().done, 0);
}

#[test]
fn an_export_gathers_the_customer_and_records_the_request() {
    let (ports, _) = world();
    let export = block_on(export_customer(&eraser(), &ports, ana_id())).unwrap();
    assert_eq!(export.customer.id(), ana_id());
    assert_eq!(export.orders.len(), 1);
    assert!(export.payments.is_empty());
    assert!(
        ports
            .outbox
            .events()
            .iter()
            .any(|e| e.event_type == "customer.data_requested")
    );
    let nobody = Ctx::new(Actor::System, vec![]);
    assert!(matches!(
        block_on(export_customer(&nobody, &ports, ana_id())),
        Err(CommandError::Auth(_))
    ));
}
