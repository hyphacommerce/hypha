//! Erasing a customer's personal data and exporting it.

use hypha_auth::{IdentityRepo as _, SessionRepo as _};
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
            compare_at_price: None,
            price_list: None,
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

    // Ana has an account: an identity and a live session.
    let mut identity = hypha_auth::Identity::with_email_code(
        hypha_core::IdentityId::from_uuid(Uuid::from_u128(901)),
        hypha_auth::Principal::Customer(ana_id()),
        Email::new("ana@example.hr").unwrap(),
    );
    block_on(ports.identities.insert(&mut identity)).unwrap();
    let mut session = hypha_auth::Session::active(
        hypha_core::SessionId::from_uuid(Uuid::from_u128(902)),
        identity.id(),
        identity.principal(),
        hypha_auth::TokenHash::of("hypha_cs_test"),
        clock.now(),
        None,
        core::time::Duration::from_secs(3600),
    );
    block_on(ports.sessions.insert(&mut session)).unwrap();
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
    assert_eq!(
        block_on(erase_due(&ports, &clock, 10))
            .unwrap()
            .progress
            .done,
        0
    );
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

    // Her account went too: no identity, the session ended.
    {
        assert_eq!(
            block_on(
                ports
                    .identities
                    .get(hypha_core::IdentityId::from_uuid(Uuid::from_u128(901)))
            ),
            Ok(None)
        );
        let ended = block_on(
            ports
                .sessions
                .get(hypha_core::SessionId::from_uuid(Uuid::from_u128(902))),
        )
        .unwrap()
        .unwrap();
        assert!(!ended.is_active(clock.now()));
    }

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
    let run = block_on(erase_due(&ports, &later, 10)).unwrap();
    assert_eq!(run.progress.done, 1);
    assert!(!run.progress.more);
    assert!(run.failed.is_empty());
    assert!(
        block_on(ports.customers.get(ana_id()))
            .unwrap()
            .unwrap()
            .is_erased()
    );
    assert_eq!(
        block_on(erase_due(&ports, &later, 10))
            .unwrap()
            .progress
            .done,
        0
    );
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

fn ana_order() -> OrderId {
    OrderId::from_uuid(Uuid::from_u128(1001))
}

#[test]
fn erasing_redacts_the_orders_email_records() {
    use hypha_notifications::{
        NewNotification, Notification, NotificationRepo, NotificationStatus, QueuedEmail,
        TemplateKey,
    };
    let (ports, clock) = world();
    let id = hypha_core::NotificationId::from_uuid(Uuid::from_u128(3001));
    let mut queued = Notification::queued(
        NewNotification {
            id,
            template: TemplateKey::OrderConfirmation,
            to: Email::new("ana@example.hr").unwrap(),
            subject: "Order #1001 confirmed".to_string(),
            event: None,
            order: Some(ana_order()),
        },
        QueuedEmail {
            sender_name: None,
            reply_to: None,
            html: "<p>Thanks, Ana</p>".to_string(),
            text: "Thanks, Ana".to_string(),
        },
        clock.now(),
    );
    block_on(ports.notifications.insert(&mut queued)).unwrap();
    block_on(erase_customer(&ports, clock.now(), ana_id())).unwrap();
    let record = block_on(ports.notifications.get(id)).unwrap().unwrap();
    assert_eq!(record.to(), &Email::placeholder(ana_id().as_uuid()));
    assert_eq!(record.status(), NotificationStatus::Skipped);
    assert_eq!(record.email(), None);
}

#[test]
fn an_open_cart_with_a_pending_payment_is_kept_redacted() {
    use hypha_payments::{NewPayment, Payment, PaymentRepo, ProviderKey};
    let (ports, clock) = world();
    let open = CartId::from_uuid(Uuid::from_u128(6002));
    let payment_id = hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001));
    let mut payment = Payment::new(NewPayment {
        id: payment_id,
        cart: Some(open),
        order: None,
        method: None,
        method_name: None,
        provider: ProviderKey::manual(),
        amount: eur(500),
        instructions: None,
    })
    .unwrap();
    block_on(ports.payments.insert(&mut payment)).unwrap();
    block_on(erase_customer(&ports, clock.now(), ana_id())).unwrap();
    let cart = block_on(ports.carts.get(open)).unwrap().expect("kept");
    assert!(cart.email().unwrap().is_placeholder());
    assert_eq!(cart.shipping_address(), None);
    let payment = block_on(ports.payments.get(payment_id)).unwrap().unwrap();
    assert_eq!(payment.cart(), Some(open));
}

#[test]
fn erasing_removes_personal_field_values_and_keeps_the_rest() {
    use hypha_core::{CustomFieldDefinitionId, HasBatch};
    use hypha_custom_fields::{
        Access, Capabilities, CustomFieldDefinition, DefinitionRepo, DefinitionSpec, FieldType,
        FieldValue, HasCustomFieldDefinitions, HasCustomFieldValues, Namespace, Owner, OwnerType,
        RawValidations, StageCustomFieldValues, ValueRepo, ValueRow,
    };
    let (ports, clock) = world();
    let field = |n: u128, key: &str, personal: bool| {
        let mut definition = CustomFieldDefinition::new(
            CustomFieldDefinitionId::from_uuid(Uuid::from_u128(n)),
            Namespace::parse("custom").unwrap(),
            DefinitionSpec {
                owner_type: OwnerType::Customer,
                key: key.to_string(),
                field_type: FieldType::parse("text").unwrap(),
                name: key.to_string(),
                description: None,
                validations: RawValidations::default(),
                access: Access::default(),
                private: false,
                personal,
                capabilities: Capabilities::default(),
            },
            n as u32,
        )
        .unwrap();
        block_on(ports.custom_field_definitions().insert(&mut definition)).unwrap();
        definition
    };
    let birthday = field(1, "birthday", true);
    let tier = field(2, "tier", false);
    let owner = Owner::customer(ana_id());
    let mut batch = ports.batch();
    for (definition, text) in [(&birthday, "1990-01-01"), (&tier, "gold")] {
        let value = FieldValue::parse(
            definition.field_type(),
            definition.validations(),
            &serde_json::json!(text),
        )
        .unwrap();
        batch.set_custom_field_value(ValueRow::new(definition, &owner, &value), None);
    }
    block_on(ports.commit(batch)).unwrap();

    block_on(erase_customer(&ports, clock.now(), ana_id())).unwrap();
    let kept = block_on(ports.custom_field_values().get_many(&[owner.id()], None)).unwrap();
    let kept: Vec<_> = kept.iter().map(|v| v.definition).collect();
    assert_eq!(kept, [tier.id()]);
}

#[test]
fn the_job_erases_the_others_when_one_erasure_fails() {
    let (ports, clock) = world();
    let bo_id = CustomerId::from_uuid(Uuid::from_u128(2));
    let mut bo = Customer::new(bo_id, Email::new("bo@example.hr").unwrap());
    block_on(ports.customers.insert(&mut bo)).unwrap();
    let settings = PrivacySettings {
        erasure_delay: Duration::from_secs(3600),
    };
    for id in [ana_id(), bo_id] {
        block_on(request_erasure(&eraser(), &ports, &clock, &settings, id)).unwrap();
    }
    // Ana's order changes after her erasure loads it: her batch is stale. Bo has no
    // orders, so the stale copy is served to Ana's erasure alone.
    let stale = block_on(ports.orders.get(ana_order())).unwrap().unwrap();
    let mut changed = stale.clone();
    block_on(ports.orders.update(&mut changed)).unwrap();
    ports.orders.serve_next_get(stale);

    let later = FixedClock::at(clock.now().plus(Duration::from_secs(3601)));
    let run = block_on(erase_due(&ports, &later, 10)).unwrap();
    assert_eq!(run.progress.done, 1);
    assert_eq!(
        run.failed,
        [(
            ana_id(),
            CommandError::Repo(hypha_core::RepoError::StaleVersion)
        )]
    );
    let erased = |id| {
        block_on(ports.customers.get(id))
            .unwrap()
            .unwrap()
            .is_erased()
    };
    assert!(erased(bo_id));
    assert!(!erased(ana_id()));
}
