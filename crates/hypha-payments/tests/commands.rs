#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]

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
    stripe.provider = "app_stripe".to_string();
    assert_eq!(
        block_on(create_payment_method(
            &Ctx::system(),
            &ports,
            &ids,
            &PROVIDERS,
            stripe
        )),
        Err(CommandError::Payment(PaymentError::UnknownProvider(
            ProviderKey::new("app_stripe").unwrap()
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
        hypha_core::CartId::from_uuid(Uuid::from_u128(52)),
        Some(OrderId::from_uuid(Uuid::from_u128(51))),
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

#[test]
fn a_method_with_an_authorized_payment_is_not_deleted_either() {
    let (ports, ids) = (InMemoryPorts::default(), SequentialIds::default());
    let id = create(&ports, &ids, cod(None));
    let method = block_on(get_payment_method(&Ctx::system(), &ports, id)).unwrap();
    let mut payment = Payment::new(NewPayment::by(
        &method,
        PaymentId::from_uuid(Uuid::from_u128(50)),
        hypha_core::CartId::from_uuid(Uuid::from_u128(52)),
        Some(OrderId::from_uuid(Uuid::from_u128(51))),
        Money::new(1000, Currency::EUR).unwrap(),
    ))
    .unwrap();
    payment.authorize(None).unwrap();
    block_on(ports.payments.insert(&mut payment)).unwrap();
    assert_eq!(
        block_on(delete_payment_method(&Ctx::system(), &ports, id)),
        Err(CommandError::PaymentMethodInUse(id))
    );
}

/// A provider under an app's key that accepts captures and voids and refuses refunds,
/// keeping what it was asked.
#[derive(Default)]
struct Gateway {
    asked: std::cell::RefCell<Vec<hypha_payments::ActionRequest>>,
}

impl hypha_payments::PaymentProvider for &Gateway {
    fn key(&self) -> ProviderKey {
        ProviderKey::new("app_gateway").unwrap()
    }

    fn check_settings(&self, _: &serde_json::Value) -> Result<(), PaymentError> {
        Ok(())
    }

    async fn start(
        &self,
        _: &hypha_payments::PaymentRequest,
    ) -> Result<hypha_payments::Started, PaymentError> {
        unreachable!("not started here")
    }

    async fn act(
        &self,
        request: &hypha_payments::ActionRequest,
    ) -> Result<hypha_payments::ActionAnswer, PaymentError> {
        self.asked.borrow_mut().push(request.clone());
        Ok(match request.action {
            hypha_payments::PaymentAction::Refund => hypha_payments::ActionAnswer::Refused {
                reason: "too late".to_string(),
            },
            _ => hypha_payments::ActionAnswer::Accepted {
                reference: Some("ch_1".to_string()),
            },
        })
    }
}

/// A payment of 10 EUR by a method of the gateway, moved by `prepare`, stored.
fn gateway_payment(ports: &InMemoryPorts, prepare: impl FnOnce(&mut Payment)) -> PaymentId {
    let id = PaymentId::from_uuid(Uuid::from_u128(60));
    let mut payment = Payment::new(NewPayment {
        id,
        cart: None,
        order: Some(OrderId::from_uuid(Uuid::from_u128(61))),
        method: None,
        method_name: None,
        provider: ProviderKey::new("app_gateway").unwrap(),
        amount: Money::new(1000, Currency::EUR).unwrap(),
        instructions: None,
    })
    .unwrap();
    prepare(&mut payment);
    block_on(ports.payments.insert(&mut payment)).unwrap();
    id
}

fn act(
    ports: &InMemoryPorts,
    gateway: &Gateway,
    payment: PaymentId,
    action: hypha_payments::PaymentAction,
    amount: Option<i64>,
) -> Result<hypha_payments::Acted, CommandError> {
    block_on(hypha_payments::act_on_payment(
        &Ctx::system(),
        ports,
        &SequentialIds::default(),
        &(gateway,),
        hypha_payments::ActOnPayment {
            payment,
            action,
            amount,
            idempotency_key: None,
        },
    ))
}

#[test]
fn an_accepted_capture_waits_for_the_providers_report() {
    use hypha_payments::{PaymentAction, PaymentStatus};
    let ports = InMemoryPorts::default();
    let gateway = Gateway::default();
    let id = gateway_payment(&ports, |p| {
        p.authorize(Some("pi_1".to_string())).unwrap();
    });
    let acted = act(&ports, &gateway, id, PaymentAction::Capture, None).unwrap();
    assert_eq!(acted.reference.as_deref(), Some("ch_1"));
    assert_eq!(acted.payment.status(), PaymentStatus::Authorized);
    let asked = gateway.asked.borrow();
    assert_eq!(asked[0].reference.as_deref(), Some("pi_1"));
    assert!(
        asked[0]
            .idempotency_key
            .starts_with(&format!("{id}:capture:")),
        "{}",
        asked[0].idempotency_key
    );
}

#[test]
fn each_attempt_without_a_key_gets_its_own_and_a_given_key_is_passed_on() {
    use hypha_payments::{ActOnPayment, PaymentAction};
    let ports = InMemoryPorts::default();
    let gateway = Gateway::default();
    let ids = SequentialIds::default();
    let id = gateway_payment(&ports, |p| {
        p.authorize(Some("pi_1".to_string())).unwrap();
    });
    let attempt = |key: Option<&str>| {
        block_on(hypha_payments::act_on_payment(
            &Ctx::system(),
            &ports,
            &ids,
            &(&gateway,),
            ActOnPayment {
                payment: id,
                action: PaymentAction::Void,
                amount: None,
                idempotency_key: key.map(str::to_string),
            },
        ))
        .unwrap()
    };
    attempt(None);
    attempt(None);
    attempt(Some("staff-click-1"));
    let keys: Vec<String> = gateway
        .asked
        .borrow()
        .iter()
        .map(|r| r.idempotency_key.clone())
        .collect();
    // A refused void retried by staff isn't the provider's replay of the refusal.
    assert_ne!(keys[0], keys[1]);
    assert!(keys[0].starts_with(&format!("{id}:void:")), "{}", keys[0]);
    assert_eq!(keys[2], "staff-click-1");
}

#[test]
fn a_payment_is_read_with_orders_read() {
    let ports = InMemoryPorts::default();
    let id = gateway_payment(&ports, |_| {});
    let payment = block_on(hypha_payments::get_payment(&Ctx::system(), &ports, id)).unwrap();
    assert_eq!(payment.id(), id);
    let missing = PaymentId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(hypha_payments::get_payment(&Ctx::system(), &ports, missing)),
        Err(CommandError::PaymentNotFound(missing))
    );
    let payments_only = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::PaymentsWrite],
    );
    assert_eq!(
        block_on(hypha_payments::get_payment(&payments_only, &ports, id)).map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::OrdersRead
        )))
    );
}

#[test]
fn acting_on_a_payment_needs_orders_write() {
    use hypha_payments::{ActOnPayment, PaymentAction};
    let ports = InMemoryPorts::default();
    let gateway = Gateway::default();
    let id = gateway_payment(&ports, |p| {
        p.authorize(Some("pi_1".to_string())).unwrap();
    });
    let payments_only = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::PaymentsWrite, Scope::OrdersRead],
    );
    let acted = block_on(hypha_payments::act_on_payment(
        &payments_only,
        &ports,
        &SequentialIds::default(),
        &(&gateway,),
        ActOnPayment {
            payment: id,
            action: PaymentAction::Void,
            amount: None,
            idempotency_key: None,
        },
    ));
    assert_eq!(
        acted.map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::OrdersWrite
        )))
    );
    assert!(
        gateway.asked.borrow().is_empty(),
        "the provider isn't asked"
    );
}

#[test]
fn an_action_must_fit_the_payments_status() {
    use hypha_payments::{PaymentAction, PaymentStatus};
    let ports = InMemoryPorts::default();
    let gateway = Gateway::default();
    let id = gateway_payment(&ports, |_| {});
    // Nothing collected yet: nothing to refund, and the provider isn't asked.
    assert_eq!(
        act(&ports, &gateway, id, PaymentAction::Refund, None),
        Err(CommandError::Payment(PaymentError::InvalidTransition {
            payment: id,
            from: PaymentStatus::Pending,
            to: PaymentStatus::Refunded,
        }))
    );
    assert!(gateway.asked.borrow().is_empty());
}

/// A pending payment's authorization may not have been reported yet (a late or lost
/// provider webhook): capture asks the provider, which knows whether there's money held.
#[test]
fn a_pending_payment_can_be_captured_the_provider_decides() {
    use hypha_payments::PaymentAction;
    let ports = InMemoryPorts::default();
    let gateway = Gateway::default();
    let id = gateway_payment(&ports, |_| {});
    assert!(act(&ports, &gateway, id, PaymentAction::Capture, None).is_ok());
    assert_eq!(gateway.asked.borrow().len(), 1);
}

#[test]
fn a_refused_refund_says_why_and_one_too_big_isnt_asked() {
    use hypha_payments::{PaymentAction, PaymentStatus};
    let ports = InMemoryPorts::default();
    let gateway = Gateway::default();
    let at = hypha_core::Timestamp::from_unix_micros(1);
    let id = gateway_payment(&ports, |p| {
        p.pay(at, None).unwrap();
    });
    assert_eq!(
        act(&ports, &gateway, id, PaymentAction::Refund, Some(1001)),
        Err(CommandError::Payment(PaymentError::InvalidRefund {
            payment: id,
            amount: 1001,
            paid: 1000,
            refunded: 0,
            reported: PaymentStatus::Refunded,
        }))
    );
    assert!(gateway.asked.borrow().is_empty());
    assert_eq!(
        act(&ports, &gateway, id, PaymentAction::Refund, Some(300)),
        Err(CommandError::PaymentRefused {
            payment: id,
            reason: "too late".to_string()
        })
    );
    assert_eq!(
        gateway.asked.borrow()[0].amount,
        Some(Money::new(300, Currency::EUR).unwrap())
    );
}

#[test]
fn manual_payments_take_no_actions() {
    use hypha_payments::PaymentAction;
    let ports = InMemoryPorts::default();
    let id = PaymentId::from_uuid(Uuid::from_u128(70));
    let mut payment = Payment::new(NewPayment::recorded_by_hand(
        id,
        OrderId::from_uuid(Uuid::from_u128(71)),
        Money::new(1000, Currency::EUR).unwrap(),
    ))
    .unwrap();
    block_on(ports.payments.insert(&mut payment)).unwrap();
    assert_eq!(
        block_on(hypha_payments::act_on_payment(
            &Ctx::system(),
            &ports,
            &SequentialIds::default(),
            &PROVIDERS,
            hypha_payments::ActOnPayment {
                payment: id,
                action: PaymentAction::Void,
                amount: None,
                idempotency_key: None,
            },
        )),
        Err(CommandError::Payment(PaymentError::ActionUnsupported {
            provider: ProviderKey::manual(),
            action: PaymentAction::Void,
        }))
    );
}
