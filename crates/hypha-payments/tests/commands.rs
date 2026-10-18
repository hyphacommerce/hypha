use hypha_core::{
    Actor, AuthError, Ctx, Currency, MarketId, Money, OrderId, PaymentId, PaymentMethodId, Scope,
    UserId,
};
use hypha_payments::{
    CommandError, CreatePaymentMethod, ManualProvider, NewPayment, Payment, PaymentError,
    PaymentRepo, ProviderKey, UpdatePaymentMethod, create_payment_method, delete_payment_method,
    get_payment_method, list_payment_methods, update_payment_method,
};
use hypha_testkit::{InMemoryPorts, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

const PROVIDERS: (ManualProvider,) = (ManualProvider,);

fn cod(position: Option<u32>) -> CreatePaymentMethod {
    CreatePaymentMethod {
        provider: "manual".to_string(),
        name: "Cash on delivery".to_string(),
        instructions: Some("Pay the courier.".to_string()),
        markets: vec![],
        channels: vec![],
        enabled: true,
        position,
        settings: None,
    }
}

fn create(
    ports: &InMemoryPorts,
    ids: &SequentialIds,
    input: CreatePaymentMethod,
) -> PaymentMethodId {
    block_on(create_payment_method(
        &Ctx::system(),
        ports,
        ids,
        &PROVIDERS,
        input,
    ))
    .unwrap()
    .id()
}

#[test]
fn new_methods_go_after_the_others() {
    let (ports, ids) = (InMemoryPorts::default(), SequentialIds::default());
    let first = create(&ports, &ids, cod(None));
    let second = create(&ports, &ids, cod(Some(7)));
    let third = create(&ports, &ids, cod(None));
    let listed = block_on(list_payment_methods(&Ctx::system(), &ports)).unwrap();
    let order: Vec<(PaymentMethodId, u32)> =
        listed.iter().map(|m| (m.id(), m.position())).collect();
    assert_eq!(order, [(first, 0), (second, 7), (third, 8)]);
}

#[test]
fn a_method_needs_a_provider_this_build_has() {
    let (ports, ids) = (InMemoryPorts::default(), SequentialIds::default());
    let mut stripe = cod(None);
    stripe.provider = "spore_stripe".to_string();
    assert_eq!(
        block_on(create_payment_method(
            &Ctx::system(),
            &ports,
            &ids,
            &PROVIDERS,
            stripe
        )),
        Err(CommandError::Payment(PaymentError::UnknownProvider(
            ProviderKey::new("spore_stripe").unwrap()
        )))
    );
    let mut bad = cod(None);
    bad.provider = "paypal".to_string();
    assert_eq!(
        block_on(create_payment_method(
            &Ctx::system(),
            &ports,
            &ids,
            &PROVIDERS,
            bad
        )),
        Err(CommandError::Payment(PaymentError::InvalidProviderKey(
            "paypal".to_string()
        )))
    );
}

#[test]
fn the_provider_checks_the_settings() {
    let (ports, ids) = (InMemoryPorts::default(), SequentialIds::default());
    let mut with_settings = cod(None);
    with_settings.settings = Some(serde_json::json!({"secret": "x"}));
    assert!(matches!(
        block_on(create_payment_method(
            &Ctx::system(),
            &ports,
            &ids,
            &PROVIDERS,
            with_settings
        )),
        Err(CommandError::Payment(PaymentError::InvalidSettings { .. }))
    ));
    let id = create(&ports, &ids, cod(None));
    let patch = UpdatePaymentMethod {
        settings: Some(serde_json::json!([1])),
        ..UpdatePaymentMethod::default()
    };
    assert!(matches!(
        block_on(update_payment_method(
            &Ctx::system(),
            &ports,
            &PROVIDERS,
            id,
            patch
        )),
        Err(CommandError::Payment(PaymentError::InvalidSettings { .. }))
    ));
}

#[test]
fn update_changes_only_what_it_names() {
    let (ports, ids) = (InMemoryPorts::default(), SequentialIds::default());
    let id = create(&ports, &ids, cod(None));
    let market = MarketId::from_uuid(Uuid::from_u128(77));
    let patch = UpdatePaymentMethod {
        instructions: Some(None),
        markets: Some(vec![market]),
        enabled: Some(false),
        position: Some(3),
        ..UpdatePaymentMethod::default()
    };
    let m = block_on(update_payment_method(
        &Ctx::system(),
        &ports,
        &PROVIDERS,
        id,
        patch,
    ))
    .unwrap();
    assert_eq!(m.name(), "Cash on delivery");
    assert_eq!(m.instructions(), None);
    assert_eq!(m.markets(), [market]);
    assert!(!m.enabled());
    assert_eq!(m.position(), 3);
    assert_eq!(
        block_on(get_payment_method(&Ctx::system(), &ports, id)),
        Ok(m)
    );
}

#[test]
fn an_unknown_method_is_not_found() {
    let ports = InMemoryPorts::default();
    let id = PaymentMethodId::from_uuid(Uuid::from_u128(9));
    assert_eq!(
        block_on(get_payment_method(&Ctx::system(), &ports, id)),
        Err(CommandError::PaymentMethodNotFound(id))
    );
    assert_eq!(
        block_on(delete_payment_method(&Ctx::system(), &ports, id)),
        Err(CommandError::PaymentMethodNotFound(id))
    );
}

#[test]
fn a_method_with_a_pending_payment_is_disabled_not_deleted() {
    let (ports, ids) = (InMemoryPorts::default(), SequentialIds::default());
    let id = create(&ports, &ids, cod(None));
    let method = block_on(get_payment_method(&Ctx::system(), &ports, id)).unwrap();
    let mut payment = Payment::new(NewPayment::by(
        &method,
        PaymentId::from_uuid(Uuid::from_u128(50)),
        OrderId::from_uuid(Uuid::from_u128(51)),
        Money::new(1000, Currency::EUR).unwrap(),
    ))
    .unwrap();
    block_on(ports.payments.insert(&mut payment)).unwrap();
    assert_eq!(
        block_on(delete_payment_method(&Ctx::system(), &ports, id)),
        Err(CommandError::PaymentMethodInUse(id))
    );

    payment
        .pay(hypha_core::Timestamp::UNIX_EPOCH, None)
        .unwrap();
    block_on(ports.payments.update(&mut payment)).unwrap();
    assert_eq!(
        block_on(delete_payment_method(&Ctx::system(), &ports, id)),
        Ok(())
    );
    assert_eq!(
        block_on(get_payment_method(&Ctx::system(), &ports, id)),
        Err(CommandError::PaymentMethodNotFound(id))
    );
    // The payment keeps what it copied.
    let kept = block_on(ports.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(kept.method_name(), Some("Cash on delivery"));
}

#[test]
fn methods_need_the_payments_scopes() {
    let (ports, ids) = (InMemoryPorts::default(), SequentialIds::default());
    let staff = |scopes| Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), scopes);
    assert_eq!(
        block_on(list_payment_methods(
            &staff(vec![Scope::OrdersWrite]),
            &ports
        ))
        .map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PaymentsRead
        )))
    );
    // `payments:write` implies `payments:read`.
    assert!(
        block_on(list_payment_methods(
            &staff(vec![Scope::PaymentsWrite]),
            &ports
        ))
        .is_ok()
    );
    assert_eq!(
        block_on(create_payment_method(
            &staff(vec![Scope::PaymentsRead]),
            &ports,
            &ids,
            &PROVIDERS,
            cod(None)
        ))
        .map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PaymentsWrite
        )))
    );
}
