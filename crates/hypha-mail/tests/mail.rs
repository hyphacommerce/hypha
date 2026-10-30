//! The mailer and the mail commands on the in-memory world.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]
use hypha_core::{
    Actor, Address, AddressParts, ApiKeyId, AssetId, AuthError, CartId, ChannelId, CountryCode,
    Ctx, Currency, CustomerId, Email, EventHandler, EventId, FulfillmentId, LocationId, MarketId,
    Money, OrderId, OrderLineId, PageLimit, PageRequest, PaymentId, Price, ProductId, Quantity,
    Scope, ShippingGroupId, ShippingOptionId, StoreId, StoredEvent, Timestamp, Timezone, UserId,
    VariantId, Versioned,
};
use hypha_fulfillment::{Fulfillment, FulfillmentLine, FulfillmentRepo, NewFulfillment, Tracking};
use hypha_mail::{
    CommandError, MailContext, MailEnv, Mailer, Sources, TemplateInput, UpdateSettings,
    order_context, preview, queue_event, render_for_sending, resend, save_template, send_due,
    send_staff, send_test, update_settings,
};
use hypha_media::{
    Asset, AssetRepo, AssetUsage, AssetUsageRepo, FileFacts, HasAssetUsages, MimeType, NewUpload,
    UsageOwner, UsageRole, Visibility,
};
use hypha_notifications::{
    EffectiveTemplate, EmailSender, NewTemplate, Notification, NotificationFilter,
    NotificationRepo, NotificationSettings, NotificationSettingsRepo, NotificationStatus,
    NotificationTemplateRepo, OutgoingEmail, RenderError, SendError, SendSettings, Sent, Template,
    TemplateError, TemplateKey, TemplatePart, TemplateSources, default_template, render,
};
use hypha_orders::{
    CancelReason, NewOrder, Order, OrderLine, OrderNumber, OrderRepo, OrderShippingLine,
};
use hypha_payments::{NewPayment, Payment, PaymentRepo, ProviderKey, newest_payment};
use hypha_stores::{Store, StoreRepo};
use hypha_testkit::{FixedClock, InMemoryEmailSender, InMemoryPorts, SequentialIds};
use hypha_users::{StaffEmail, User, UserRepo};
use pollster::block_on;
use std::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;

fn uuid(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn email(s: &str) -> Email {
    Email::new(s).unwrap()
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::EUR).unwrap()
}

fn store_id() -> StoreId {
    StoreId::from_uuid(uuid(900))
}

fn order_id() -> OrderId {
    OrderId::from_uuid(uuid(1000))
}

fn line_id(n: u128) -> OrderLineId {
    OrderLineId::from_uuid(uuid(1100 + n))
}

fn fulfillment_id() -> FulfillmentId {
    FulfillmentId::from_uuid(uuid(1200))
}

fn staff_user() -> UserId {
    UserId::from_uuid(uuid(1300))
}

/// The host side: a sender, the operator's address, links. `S` is the sender, the
/// in-memory one unless a test needs another.
struct TestEnv<S = InMemoryEmailSender> {
    sender: S,
    clock: FixedClock,
    ids: SequentialIds,
    from: Email,
    admin_url: Option<String>,
    /// How many times the send job was woken (`MailEnv::queued`).
    woken: AtomicUsize,
}

impl TestEnv {
    fn new() -> Self {
        TestEnv::with_sender(InMemoryEmailSender::default())
    }
}

impl<S> TestEnv<S> {
    fn with_sender(sender: S) -> Self {
        Self {
            sender,
            clock: FixedClock::default(),
            ids: SequentialIds::default(),
            from: email("shop@mail.example.com"),
            admin_url: Some("https://admin.example.com".to_string()),
            woken: AtomicUsize::new(0),
        }
    }
}

impl<S: EmailSender> MailEnv for TestEnv<S> {
    type Sender = S;
    type Clock = FixedClock;
    type Ids = SequentialIds;

    fn sender(&self) -> &S {
        &self.sender
    }

    fn clock(&self) -> &FixedClock {
        &self.clock
    }

    fn ids(&self) -> &SequentialIds {
        &self.ids
    }

    fn sender_address(&self) -> &Email {
        &self.from
    }

    fn store(&self) -> StoreId {
        store_id()
    }

    fn admin_url(&self) -> Option<&str> {
        self.admin_url.as_deref()
    }

    fn order_token(&self, order: OrderId) -> String {
        format!("tok_{order}")
    }

    async fn asset_url(&self, asset: &Asset) -> Option<String> {
        Some(format!("https://cdn.example.com/{}", asset.id()))
    }

    fn queued(&self) {
        self.woken.fetch_add(1, Ordering::SeqCst);
    }
}

impl<S> TestEnv<S> {
    fn woken(&self) -> usize {
        self.woken.load(Ordering::SeqCst)
    }

    /// Moves the send job's clock `secs` on.
    fn later(&self, secs: i64) {
        self.clock
            .set(hypha_core::Clock::now(&self.clock).plus_secs(secs));
    }
}

/// The `log` sender's name: test sends refuse it.
struct LogSender;

impl EmailSender for LogSender {
    fn name(&self) -> &'static str {
        "log"
    }

    async fn send(&self, _: &OutgoingEmail) -> Result<Sent, SendError> {
        Ok(Sent::default())
    }
}

fn line(n: u128, title: &str, variant: Option<&str>, quantity: u32, unit: i64) -> OrderLine {
    OrderLine {
        id: line_id(n),
        product: ProductId::from_uuid(uuid(2000 + n)),
        variant: VariantId::from_uuid(uuid(2100 + n)),
        quantity: Quantity::new(quantity).unwrap(),
        product_title: title.to_string(),
        variant_title: variant.map(str::to_string),
        sku: Some(format!("SKU-{n}")),
        unit_price: Price::new(eur(unit)).unwrap(),
        subtotal: eur(unit * i64::from(quantity)),
        discounts: Vec::new(),
        total: eur(unit * i64::from(quantity)),
        tax: None,
        fulfilled_quantity: 0,
        compare_at_price: None,
        price_list: None,
    }
}

/// A store in Belgrade's time zone, order #1001 (two shirts, a mug, shipped to Ana by
/// standard delivery) with a pending bank transfer, and staff member 1300.
fn world() -> InMemoryPorts {
    let ports = InMemoryPorts::default();
    let mut store = Store::new(store_id(), "Fern & Field").unwrap();
    store.set_contact_email(Some(email("hello@fernandfield.example")));
    store.set_timezone(Timezone::new("Europe/Belgrade").ok());
    block_on(ports.stores.insert(&mut store)).unwrap();

    let address = Address::new(AddressParts {
        first_name: Some("Ana".to_string()),
        last_name: "Horvat".to_string(),
        company: None,
        line1: "Ilica 42".to_string(),
        line2: None,
        city: "Zagreb".to_string(),
        province: None,
        postal_code: Some("10000".to_string()),
        country: "HR".to_string(),
        phone: None,
    })
    .unwrap();
    let lines = vec![
        line(1, "Linen shirt", Some("M / Blue"), 2, 4500),
        line(2, "Ceramic mug", None, 1, 1800),
    ];
    let mut order = Order::place(NewOrder {
        id: order_id(),
        number: OrderNumber::new(1001).unwrap(),
        placed_at: FixedClock::DEFAULT,
        cart: CartId::from_uuid(uuid(3000)),
        channel: ChannelId::from_uuid(uuid(3100)),
        market: MarketId::from_uuid(uuid(3200)),
        currency: Currency::EUR,
        discounts: Vec::new(),
        discount_total: Money::zero(Currency::EUR),
        country: CountryCode::new("HR").unwrap(),
        taxes_included: true,
        customer: CustomerId::from_uuid(uuid(3300)),
        email: email("ana@example.hr"),
        shipping_lines: vec![OrderShippingLine {
            group: ShippingGroupId::from_uuid(uuid(3400)),
            lines: vec![line_id(1), line_id(2)],
            option: ShippingOptionId::from_uuid(uuid(3500)),
            code: "standard".to_string(),
            name: "Standard delivery".to_string(),
            provider: None,
            service: None,
            reference: None,
            discounts: Vec::new(),
            total: (Price::new(eur(490)).unwrap()).money(),
            amount: Price::new(eur(490)).unwrap(),
            estimate_days: None,
            tax: None,
        }],
        lines,
        shipping_address: Some(address),
        subtotal: eur(10800),
        shipping_total: eur(490),
        tax_total: eur(0),
        total: eur(11290),
    })
    .unwrap();
    block_on(ports.orders.insert(&mut order)).unwrap();

    let mut payment = Payment::new(NewPayment {
        id: PaymentId::from_uuid(uuid(4000)),
        cart: None,
        order: Some(order_id()),
        method: None,
        method_name: Some("Bank transfer".to_string()),
        provider: ProviderKey::manual(),
        amount: eur(11290),
        instructions: Some("IBAN: HR12 1001 0051 8630 0016 0\nReference: 1001".to_string()),
    })
    .unwrap();
    block_on(ports.payments.insert(&mut payment)).unwrap();

    let mut user = User::new(
        staff_user(),
        email("marko@fernandfield.example"),
        "Marko",
        hypha_core::RoleId::from_uuid(uuid(1400)),
        FixedClock::DEFAULT,
    )
    .unwrap();
    block_on(ports.users.insert(&mut user)).unwrap();
    ports
}

/// Ships the shirts with DHL tracking, recording `fulfillment.created`.
fn ship(ports: &InMemoryPorts, notify_customer: bool) {
    let mut fulfillment = Fulfillment::new(NewFulfillment {
        id: fulfillment_id(),
        order: order_id(),
        location: LocationId::from_uuid(uuid(5000)),
        lines: vec![FulfillmentLine {
            line: line_id(1),
            quantity: Quantity::new(2).unwrap(),
            stock_taken: true,
        }],
        tracking: vec![
            Tracking::new(Some("DHL"), "JD0001", Some("https://dhl.example/JD0001")).unwrap(),
        ],
        notify_customer,
    })
    .unwrap();
    block_on(ports.fulfillments.insert(&mut fulfillment)).unwrap();
}

fn cancel(ports: &InMemoryPorts, notify_customer: bool) {
    let mut order = block_on(ports.orders.get(order_id())).unwrap().unwrap();
    order
        .cancel(
            FixedClock::DEFAULT,
            CancelReason::Inventory,
            notify_customer,
        )
        .unwrap();
    block_on(ports.orders.update(&mut order)).unwrap();
}

/// The last recorded event of `event_type`.
fn event(ports: &InMemoryPorts, event_type: &str) -> StoredEvent {
    ports
        .outbox
        .events()
        .into_iter()
        .rev()
        .find(|e| e.event_type == event_type)
        .unwrap()
}

fn records(ports: &InMemoryPorts) -> Vec<Notification> {
    let page = PageRequest {
        after: None,
        limit: PageLimit::new(100).unwrap(),
    };
    let mut items = block_on(
        ports
            .notifications
            .list(NotificationFilter::default(), page),
    )
    .unwrap()
    .items;
    items.reverse();
    items
}

fn set_staff_recipients(ports: &InMemoryPorts, recipients: &[&str]) {
    let input = UpdateSettings {
        staff_recipients: Some(recipients.iter().map(|r| r.to_string()).collect()),
        ..UpdateSettings::default()
    };
    block_on(update_settings(&Ctx::system(), ports, input)).unwrap();
}

fn disable(ports: &InMemoryPorts, key: TemplateKey) {
    let mut template = Template::new(NewTemplate {
        key,
        locale: None,
        sources: None,
        enabled: false,
    })
    .unwrap();
    block_on(ports.notification_templates.insert(&mut template)).unwrap();
}

/// Runs the send job once on everything due.
fn send_job<S: EmailSender>(env: &TestEnv<S>, ports: &InMemoryPorts) -> Vec<Notification> {
    block_on(send_due(env, ports, &SendSettings::default()))
        .unwrap()
        .attempted
}

/// Queues what `event` calls for and runs the send job: what a dispatch step and the
/// send job it wakes do. The records as the job left them.
fn mail_event(
    env: &TestEnv,
    ports: &InMemoryPorts,
    event: &StoredEvent,
) -> Result<Vec<Notification>, CommandError> {
    let queued = block_on(queue_event(env, ports, event))?;
    send_job(env, ports);
    Ok(queued
        .iter()
        .map(|n| block_on(ports.notifications.get(n.id())).unwrap().unwrap())
        .collect())
}

fn mailer() -> Mailer<TestEnv> {
    Mailer::new(TestEnv::new())
}

fn handle(
    mailer: &Mailer<TestEnv>,
    ports: &InMemoryPorts,
    event: &StoredEvent,
) -> Result<(), String> {
    block_on(EventHandler::<InMemoryPorts>::handle(mailer, ports, event))
}

// Rendering the defaults.

#[test]
fn every_default_renders_against_its_sample() {
    let layout = default_template(TemplateKey::Layout).html;
    for key in TemplateKey::ALL.into_iter().filter(|k| !k.is_layout()) {
        let rendered = render(
            layout,
            &default_template(key).sources(),
            &MailContext::sample(key),
        );
        let rendered = rendered.unwrap_or_else(|e| panic!("{key}: {e}"));
        assert!(!rendered.subject.is_empty(), "{key}");
        assert!(rendered.html.contains("Fern &amp; Field"), "{key}");
        assert!(!rendered.text.contains("none"), "{key}: {}", rendered.text);
    }
}

#[test]
fn order_totals_add_up_and_list_the_discounts_they_include() {
    // The sample's subtotal and shipping are after its 10.80 discount: listing it as
    // "−10.80" under them would take it off twice.
    let layout = default_template(TemplateKey::Layout).html;
    for key in [
        TemplateKey::OrderConfirmation,
        TemplateKey::OrderCancelled,
        TemplateKey::StaffNewOrder,
    ] {
        let rendered = render(
            layout,
            &default_template(key).sources(),
            &MailContext::sample(key),
        )
        .unwrap();
        assert!(!rendered.text.contains('−'), "{key}: {}", rendered.text);
        assert!(!rendered.html.contains('−'), "{key}");
        assert!(
            rendered
                .text
                .contains("Discounts included above:\n- Welcome offer (WELCOME10): €10.80"),
            "{key}: {}",
            rendered.text
        );
    }
}

#[test]
fn every_default_renders_without_what_may_be_absent() {
    let layout = default_template(TemplateKey::Layout).html;
    for key in TemplateKey::ALL.into_iter().filter(|k| !k.is_layout()) {
        for context in MailContext::checks(key) {
            let rendered = render(layout, &default_template(key).sources(), &context);
            let rendered = rendered.unwrap_or_else(|e| panic!("{key}: {e}"));
            assert!(!rendered.text.contains("none"), "{key}: {}", rendered.text);
        }
    }
}

#[test]
fn staff_get_no_buyers_order_link() {
    let ports = world();
    let input = UpdateSettings {
        order_url: Some(Some(
            "https://shop.example.com/order/{order}?token={token}".to_string(),
        )),
        ..UpdateSettings::default()
    };
    block_on(update_settings(&Ctx::system(), &ports, input)).unwrap();
    let env = TestEnv::new();
    let branding = block_on(hypha_mail::branding(&env, &ports)).unwrap();
    let order = block_on(ports.orders.get(order_id())).unwrap().unwrap();
    let context = |key| {
        let context = block_on(order_context(
            &env, &ports, &branding, key, &order, None, None,
        ))
        .unwrap();
        serde_json::to_value(context).unwrap()
    };
    let link = format!(
        "https://shop.example.com/order/{}?token=tok_{}",
        order_id(),
        order_id()
    );
    assert_eq!(
        context(TemplateKey::OrderConfirmation)["order"]["url"],
        link
    );
    let staff = context(TemplateKey::StaffNewOrder);
    assert_eq!(staff["order"]["url"], serde_json::Value::Null);
    assert!(!staff.to_string().contains("tok_"), "{staff}");
    let sample = serde_json::to_value(MailContext::sample(TemplateKey::StaffNewOrder)).unwrap();
    assert_eq!(sample["order"]["url"], serde_json::Value::Null);
}

#[test]
fn every_default_renders_against_a_real_order() {
    let ports = world();
    // Recorded straight in the repo, the shipment leaves the order unfulfilled, so it
    // can be cancelled too: one order with every template's data.
    ship(&ports, true);
    cancel(&ports, true);
    let env = TestEnv::new();
    let branding = block_on(hypha_mail::branding(&env, &ports)).unwrap();
    let order = block_on(ports.orders.get(order_id())).unwrap().unwrap();
    let payment = block_on(newest_payment(&ports, order_id())).unwrap();
    for key in [
        TemplateKey::OrderConfirmation,
        TemplateKey::OrderShipped,
        TemplateKey::OrderTrackingUpdated,
        TemplateKey::OrderCancelled,
        TemplateKey::StaffNewOrder,
    ] {
        let context = block_on(order_context(
            &env,
            &ports,
            &branding,
            key,
            &order,
            payment.as_ref(),
            None,
        ))
        .unwrap();
        let sources = Sources {
            layout: EffectiveTemplate::of(TemplateKey::Layout, None),
            template: EffectiveTemplate::of(key, None),
        };
        let rendering =
            render_for_sending(&sources, &context).unwrap_or_else(|e| panic!("{key}: {e}"));
        assert_eq!(rendering.note, None);
        let rendered = rendering.rendered;
        assert!(
            rendered.subject.contains("#1001"),
            "{key}: {}",
            rendered.subject
        );
        assert!(!rendered.text.contains("none"), "{key}: {}", rendered.text);
    }
}

#[test]
fn the_confirmation_has_the_orders_details() {
    let ports = world();
    let env = TestEnv::new();
    let sent = mail_event(&env, &ports, &event(&ports, "order.placed")).unwrap();
    assert_eq!(sent.len(), 1);
    let emails = env.sender.sent();
    let email = &emails[0];
    assert_eq!(email.to, self::email("ana@example.hr"));
    assert_eq!(
        email.from.to_string(),
        r#""Fern & Field" <shop@mail.example.com>"#
    );
    assert_eq!(email.subject, "Order #1001 confirmed");
    // The bank details keep their lines; times are Belgrade's (UTC+1 on 1 Jan).
    assert!(
        email
            .text
            .contains("IBAN: HR12 1001 0051 8630 0016 0\nReference: 1001"),
        "{}",
        email.text
    );
    assert!(
        email.text.contains("placed 1 Jan 2026, 01:00"),
        "{}",
        email.text
    );
    assert!(email.text.contains("2 × Linen shirt (M / Blue): €90.00"));
    assert!(email.text.contains("Total: €112.90"));
    assert!(email.text.contains("Shipping method: Standard delivery"));
    assert!(email.text.contains("Ana Horvat"));
    assert!(email.text.contains("Croatia"));
    assert_eq!(email.id, sent[0].id());
}

#[test]
fn an_order_link_is_made_from_the_settings_pattern_and_the_token() {
    let ports = world();
    let input = UpdateSettings {
        order_url: Some(Some(
            "https://shop.example.com/order/{order}?token={token}".to_string(),
        )),
        ..UpdateSettings::default()
    };
    block_on(update_settings(&Ctx::system(), &ports, input)).unwrap();
    let env = TestEnv::new();
    mail_event(&env, &ports, &event(&ports, "order.placed")).unwrap();
    let text = &env.sender.sent()[0].text;
    let link = format!(
        "https://shop.example.com/order/{}?token=tok_{}",
        order_id(),
        order_id()
    );
    assert!(text.contains(&link), "{text}");
}

// The handler.

#[test]
fn the_mailer_wants_the_four_order_events() {
    let mailer = mailer();
    let wants = |t| EventHandler::<InMemoryPorts>::wants(&mailer, t);
    assert_eq!(EventHandler::<InMemoryPorts>::name(&mailer), "mailer");
    for t in [
        "order.placed",
        "order.cancelled",
        "fulfillment.created",
        "fulfillment.updated",
    ] {
        assert!(wants(t), "{t}");
    }
    assert!(!wants("order.paid"));
    assert!(!wants("fulfillment.cancelled"));
}

#[test]
fn the_handler_only_queues_and_wakes_the_send_job() {
    let ports = world();
    set_staff_recipients(&ports, &["ops@fernandfield.example"]);
    let mailer = mailer();
    assert_eq!(
        handle(&mailer, &ports, &event(&ports, "order.placed")),
        Ok(())
    );
    assert!(
        mailer.env().sender.sent().is_empty(),
        "no network in the handler"
    );
    assert_eq!(mailer.env().woken(), 1);
    let records = records(&ports);
    assert_eq!(records.len(), 2);
    for r in &records {
        assert_eq!(r.status(), NotificationStatus::Queued);
        assert_eq!(r.attempts(), 0);
        assert_eq!(r.next_attempt_at(), Some(FixedClock::DEFAULT));
        assert!(r.email().is_some_and(|e| e.text.contains("#1001")));
    }
    assert_eq!(records[0].subject(), "Order #1001 confirmed");
    let sent = send_job(mailer.env(), &ports);
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().all(|r| r.status() == NotificationStatus::Sent
        && r.email().is_none()
        && r.attempts() == 1));
    assert_eq!(mailer.env().sender.sent().len(), 2);
}

#[test]
fn a_second_run_queues_nothing_more() {
    let ports = world();
    set_staff_recipients(&ports, &["ops@fernandfield.example"]);
    let mailer = mailer();
    let placed = event(&ports, "order.placed");
    assert_eq!(handle(&mailer, &ports, &placed), Ok(()));
    assert_eq!(handle(&mailer, &ports, &placed), Ok(()));
    assert_eq!(mailer.env().woken(), 1, "the second run queued nothing");
    send_job(mailer.env(), &ports);
    // And after they're sent, another run of the same event still sends nothing.
    assert_eq!(handle(&mailer, &ports, &placed), Ok(()));
    send_job(mailer.env(), &ports);
    assert_eq!(mailer.env().sender.sent().len(), 2);
    let records = records(&ports);
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|r| r.status() == NotificationStatus::Sent
                && r.event() == Some(EventId::from_uuid(placed.id))
                && r.order() == Some(order_id())
                && r.attempts() == 1)
    );
}

#[test]
fn staff_get_the_new_order_email_each() {
    let ports = world();
    set_staff_recipients(
        &ports,
        &["ops@fernandfield.example", "boss@fernandfield.example"],
    );
    let env = TestEnv::new();
    mail_event(&env, &ports, &event(&ports, "order.placed")).unwrap();
    let sent = env.sender.sent();
    let to: Vec<&str> = sent.iter().map(|e| e.to.as_str()).collect();
    assert_eq!(
        to,
        [
            "ana@example.hr",
            "ops@fernandfield.example",
            "boss@fernandfield.example"
        ]
    );
    assert_eq!(sent[1].subject, "New order #1001: €112.90");
    assert_eq!(sent[2].subject, sent[1].subject);
    let admin = format!("https://admin.example.com/orders/{}", order_id());
    assert!(sent[1].text.contains(&admin), "{}", sent[1].text);
}

#[test]
fn without_staff_recipients_only_the_buyer_is_emailed() {
    let ports = world();
    let env = TestEnv::new();
    mail_event(&env, &ports, &event(&ports, "order.placed")).unwrap();
    let records = records(&ports);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].template(), TemplateKey::OrderConfirmation);
}

#[test]
fn an_erased_buyer_is_skipped_while_staff_are_still_emailed() {
    let ports = world();
    set_staff_recipients(&ports, &["ops@fernandfield.example"]);
    let placeholder = Email::placeholder(uuid(3300));
    let mut order = block_on(ports.orders.get(order_id())).unwrap().unwrap();
    assert!(order.redact(placeholder.clone()));
    block_on(ports.orders.update(&mut order)).unwrap();
    let env = TestEnv::new();
    let records = mail_event(&env, &ports, &event(&ports, "order.placed")).unwrap();
    let buyer = records
        .iter()
        .find(|r| r.template() == TemplateKey::OrderConfirmation)
        .unwrap();
    assert_eq!(
        (buyer.status(), buyer.to(), buyer.error()),
        (
            NotificationStatus::Skipped,
            &placeholder,
            Some("the customer was erased")
        )
    );
    let sent = env.sender.sent();
    let to: Vec<&str> = sent.iter().map(|e| e.to.as_str()).collect();
    assert_eq!(to, ["ops@fernandfield.example"]);
}

#[test]
fn a_login_link_percent_encodes_the_email_and_the_code() {
    assert_eq!(
        hypha_mail::context::login_link(Some("https://shop.example"), "a+b@x.com", "12 3"),
        "https://shop.example/account/login?email=a%2Bb%40x.com&code=12%203"
    );
}

#[test]
fn without_a_shop_url_a_login_link_is_empty() {
    assert_eq!(
        hypha_mail::context::login_link(None, "ana@example.hr", "123456"),
        ""
    );
}

#[test]
fn a_template_turned_off_is_recorded_as_skipped_without_rendering() {
    let ports = world();
    // Off, and broken: it isn't rendered, so it doesn't matter.
    let sources = TemplateSources::new(
        TemplateKey::OrderConfirmation,
        "{{ order.nonexistent }}".to_string(),
        r#"{% extends "layout" %}{% block content %}{{ nope.nope }}{% endblock %}"#.to_string(),
        None,
    )
    .unwrap();
    let mut template = Template::new(NewTemplate {
        key: TemplateKey::OrderConfirmation,
        locale: None,
        sources: Some(sources),
        enabled: false,
    })
    .unwrap();
    block_on(ports.notification_templates.insert(&mut template)).unwrap();
    let env = TestEnv::new();
    mail_event(&env, &ports, &event(&ports, "order.placed")).unwrap();
    assert!(env.sender.sent().is_empty());
    assert_eq!(env.woken(), 0);
    let records = records(&ports);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status(), NotificationStatus::Skipped);
    assert_eq!(records[0].error(), Some("the template is turned off"));
    assert_eq!(records[0].subject(), "");
    // And a second run doesn't make another.
    mail_event(&env, &ports, &event(&ports, "order.placed")).unwrap();
    assert_eq!(self::records(&ports).len(), 1);
}

#[test]
fn a_transient_failure_is_queued_again_and_sent_once_later() {
    let ports = world();
    let mailer = mailer();
    let env = mailer.env();
    let placed = event(&ports, "order.placed");
    assert_eq!(handle(&mailer, &ports, &placed), Ok(()));
    env.sender
        .fail_next(SendError::Transient("timed out".to_string()));
    let first = send_job(env, &ports);
    assert_eq!(first[0].status(), NotificationStatus::Queued);
    assert_eq!(first[0].error(), Some("timed out"));
    assert_eq!(first[0].provider(), Some("memory"));
    assert_eq!(first[0].attempts(), 1);
    // The schedule's first wait: a minute.
    assert_eq!(
        first[0].next_attempt_at(),
        Some(FixedClock::DEFAULT.plus_secs(60))
    );
    // The event retried (another handler failed, say) changes nothing.
    assert_eq!(handle(&mailer, &ports, &placed), Ok(()));
    assert!(send_job(env, &ports).is_empty(), "not due yet");
    env.later(60);
    let sent = send_job(env, &ports);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].id(), first[0].id(), "the same record, tried again");
    assert_eq!(sent[0].status(), NotificationStatus::Sent);
    assert_eq!(sent[0].attempts(), 2);
    assert_eq!(sent[0].error(), None);
    assert_eq!(sent[0].provider_message_id(), Some("memory_1"));
    assert_eq!(sent[0].sent_at(), Some(FixedClock::DEFAULT.plus_secs(60)));
    assert_eq!(env.sender.sent().len(), 1);
    assert_eq!(env.sender.sent()[0].id, sent[0].id(), "the idempotency key");
    assert_eq!(records(&ports).len(), 1);
}

#[test]
fn transient_failures_fail_for_good_when_the_schedule_ends() {
    let ports = world();
    let env = TestEnv::new();
    block_on(queue_event(&env, &ports, &event(&ports, "order.placed"))).unwrap();
    let settings = SendSettings {
        retry: vec![std::time::Duration::from_secs(10)],
        ..SendSettings::default()
    };
    let run = |env: &TestEnv| {
        block_on(send_due(env, &ports, &settings))
            .unwrap()
            .attempted
    };
    env.sender
        .fail_next(SendError::Transient("421".to_string()));
    env.sender
        .fail_next(SendError::Transient("421 again".to_string()));
    assert_eq!(run(&env)[0].status(), NotificationStatus::Queued);
    env.later(10);
    let last = run(&env);
    assert_eq!(last[0].status(), NotificationStatus::Failed);
    assert_eq!(last[0].error(), Some("421 again"));
    assert_eq!(last[0].attempts(), 2);
    assert_eq!(last[0].email(), None);
    env.later(3600);
    assert!(run(&env).is_empty());
}

#[test]
fn a_permanent_refusal_is_final_even_when_the_event_runs_again() {
    let ports = world();
    let mailer = mailer();
    let placed = event(&ports, "order.placed");
    assert_eq!(handle(&mailer, &ports, &placed), Ok(()));
    mailer
        .env()
        .sender
        .fail_next(SendError::Permanent("address refused".to_string()));
    send_job(mailer.env(), &ports);
    // Another destination's failure retries the event: this message stays failed.
    assert_eq!(handle(&mailer, &ports, &placed), Ok(()));
    mailer.env().later(24 * 3600);
    assert!(send_job(mailer.env(), &ports).is_empty());
    let records = records(&ports);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status(), NotificationStatus::Failed);
    assert_eq!(records[0].error(), Some("address refused"));
    assert!(mailer.env().sender.sent().is_empty());
}

#[test]
fn one_messages_failure_doesnt_hold_up_the_others() {
    let ports = world();
    set_staff_recipients(&ports, &["ops@fernandfield.example"]);
    let env = TestEnv::new();
    block_on(queue_event(&env, &ports, &event(&ports, "order.placed"))).unwrap();
    env.sender
        .fail_next(SendError::Transient("rate limited".to_string()));
    let run = send_job(&env, &ports);
    let statuses: Vec<NotificationStatus> = run.iter().map(Notification::status).collect();
    assert_eq!(
        statuses,
        [NotificationStatus::Queued, NotificationStatus::Sent]
    );
    let to: Vec<String> = env.sender.sent().iter().map(|e| e.to.to_string()).collect();
    assert_eq!(to, ["ops@fernandfield.example"]);
}

#[test]
fn a_message_held_by_a_running_lease_isnt_sent_again() {
    let ports = world();
    let env = TestEnv::new();
    block_on(queue_event(&env, &ports, &event(&ports, "order.placed"))).unwrap();
    // One run leased it and is still sending; another starts.
    let settings = SendSettings::default();
    let held = block_on(hypha_mail::lease_due(&ports, &env.clock, &settings)).unwrap();
    assert_eq!(held.len(), 1);
    env.later(30);
    assert!(send_job(&env, &ports).is_empty());
    assert!(env.sender.sent().is_empty());
    // The first finishes; its record stands.
    let attempt = block_on(hypha_mail::send_leased(&env, &held[0]));
    let done = block_on(hypha_mail::record_attempt(
        &ports,
        &env.clock,
        &settings,
        held[0].clone(),
        attempt,
    ))
    .unwrap();
    assert_eq!(done.status(), NotificationStatus::Sent);
    assert_eq!(env.sender.sent().len(), 1);
}

#[test]
fn a_lease_that_ran_out_is_taken_again_with_the_same_id() {
    let ports = world();
    let env = TestEnv::new();
    block_on(queue_event(&env, &ports, &event(&ports, "order.placed"))).unwrap();
    let settings = SendSettings::default();
    // A runner leased it and stopped (crashed, cut off) without recording.
    let lost = block_on(hypha_mail::lease_due(&ports, &env.clock, &settings)).unwrap();
    env.later(settings.lease.as_secs() as i64);
    let sent = send_job(&env, &ports);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].id(), lost[0].id());
    assert_eq!(sent[0].attempts(), 2);
    assert_eq!(sent[0].status(), NotificationStatus::Sent);
    // The provider gets the record's ID as its idempotency key either way.
    assert_eq!(env.sender.sent()[0].id, lost[0].id());
    // The lost runner's late record is stale, not a second outcome.
    let late = block_on(hypha_mail::record_attempt(
        &ports,
        &env.clock,
        &settings,
        lost[0].clone(),
        hypha_mail::Attempt {
            provider: "memory",
            result: Ok(Sent::default()),
        },
    ));
    assert_eq!(
        late.map(drop),
        Err(CommandError::Repo(hypha_core::RepoError::StaleVersion))
    );
}

#[test]
fn a_send_that_cant_finish_within_the_lease_isnt_started() {
    let ports = world();
    let env = TestEnv::new();
    block_on(queue_event(&env, &ports, &event(&ports, "order.placed"))).unwrap();
    let short = SendSettings {
        lease: std::time::Duration::from_secs(10),
        ..SendSettings::default()
    };
    let run = block_on(send_due(&env, &ports, &short)).unwrap();
    assert_eq!((run.attempted.len(), run.left), (0, 1));
    assert!(env.sender.sent().is_empty());
}

#[test]
fn shipping_emails_go_only_when_staff_chose_to_tell_the_buyer() {
    let ports = world();
    ship(&ports, false);
    let env = TestEnv::new();
    let quiet = event(&ports, "fulfillment.created");
    assert_eq!(mail_event(&env, &ports, &quiet), Ok(vec![]));

    let ports = world();
    ship(&ports, true);
    let sent = mail_event(&env, &ports, &event(&ports, "fulfillment.created")).unwrap();
    assert_eq!(sent[0].template(), TemplateKey::OrderShipped);
    let email = &env.sender.sent()[0];
    assert_eq!(email.subject, "Your order #1001 is on its way");
    assert!(
        email
            .text
            .contains("DHL: JD0001 (https://dhl.example/JD0001)"),
        "{}",
        email.text
    );
    assert!(email.text.contains("- 2 × Linen shirt (M / Blue)"));
    assert!(!email.text.contains("Ceramic mug"), "only what shipped");
}

#[test]
fn a_tracking_update_names_the_new_tracking() {
    let ports = world();
    ship(&ports, false);
    let mut fulfillment = block_on(ports.fulfillments.get(fulfillment_id()))
        .unwrap()
        .unwrap();
    let tracking = vec![Tracking::new(Some("GLS"), "GLS42", None).unwrap()];
    fulfillment.set_tracking(tracking, true).unwrap();
    block_on(ports.fulfillments.update(&mut fulfillment)).unwrap();
    let env = TestEnv::new();
    mail_event(&env, &ports, &event(&ports, "fulfillment.updated")).unwrap();
    let email = &env.sender.sent()[0];
    assert_eq!(email.subject, "Tracking updated for order #1001");
    assert!(email.text.contains("GLS: GLS42"), "{}", email.text);
}

#[test]
fn a_cancellation_email_goes_only_when_asked_with_its_reason() {
    let ports = world();
    cancel(&ports, false);
    let env = TestEnv::new();
    let quiet = event(&ports, "order.cancelled");
    assert_eq!(mail_event(&env, &ports, &quiet), Ok(vec![]));

    let ports = world();
    cancel(&ports, true);
    mail_event(&env, &ports, &event(&ports, "order.cancelled")).unwrap();
    let email = &env.sender.sent()[0];
    assert_eq!(email.subject, "Order #1001 cancelled");
    assert!(
        email.text.contains("Reason: Items unavailable."),
        "{}",
        email.text
    );
}

#[test]
fn events_from_before_notify_customer_send_nothing() {
    let ports = world();
    cancel(&ports, true);
    let mut old = event(&ports, "order.cancelled");
    old.data = serde_json::json!({ "order": order_id(), "reason": "inventory" });
    let env = TestEnv::new();
    assert_eq!(mail_event(&env, &ports, &old), Ok(vec![]));
}

#[test]
fn an_event_with_a_bad_payload_is_an_error() {
    let ports = world();
    let mut bad = event(&ports, "order.placed");
    bad.data = serde_json::json!({ "order": 7 });
    let result = handle(&mailer(), &ports, &bad);
    assert!(
        result
            .as_ref()
            .is_err_and(|e| e.contains("unexpected payload")),
        "{result:?}"
    );
}

#[test]
fn a_customized_template_that_breaks_falls_back_to_the_default() {
    let ports = world();
    // Stored before a rule it now breaks: names a variable the context lacks.
    let sources = TemplateSources::new(
        TemplateKey::OrderConfirmation,
        "Thanks {{ order.nonexistent }}".to_string(),
        r#"{% extends "layout" %}{% block content %}x{% endblock %}"#.to_string(),
        None,
    )
    .unwrap();
    let mut template = Template::new(NewTemplate {
        key: TemplateKey::OrderConfirmation,
        locale: None,
        sources: Some(sources),
        enabled: true,
    })
    .unwrap();
    block_on(ports.notification_templates.insert(&mut template)).unwrap();
    let env = TestEnv::new();
    let sent = mail_event(&env, &ports, &event(&ports, "order.placed")).unwrap();
    assert_eq!(env.sender.sent()[0].subject, "Order #1001 confirmed");
    let note = sent[0].note().unwrap_or_default();
    assert!(
        note.starts_with("sent with the default template: ") && note.contains("subject"),
        "{note}"
    );
    assert_eq!(sent[0].status(), NotificationStatus::Sent);
}

// Settings.

fn add_asset(
    ports: &InMemoryPorts,
    n: u128,
    mime_type: MimeType,
    visibility: Visibility,
    ready: bool,
) {
    let mut asset = Asset::new_upload(NewUpload {
        id: AssetId::from_uuid(uuid(n)),
        filename: format!("{n}.bin"),
        mime_type,
        byte_size: 100,
        checksum: None,
        visibility,
        now: FixedClock::DEFAULT,
    })
    .unwrap();
    if ready {
        asset
            .mark_ready(FileFacts::default(), FixedClock::DEFAULT)
            .unwrap();
    }
    block_on(ports.assets.insert(&mut asset)).unwrap();
}

fn usages(ports: &InMemoryPorts, asset: u128) -> Vec<AssetUsage> {
    let page = PageRequest {
        after: None,
        limit: PageLimit::new(100).unwrap(),
    };
    block_on(
        ports
            .asset_usages()
            .list_usages(AssetId::from_uuid(uuid(asset)), page),
    )
    .unwrap()
    .items
}

fn set_logo(
    ports: &InMemoryPorts,
    logo: Option<u128>,
) -> Result<NotificationSettings, CommandError> {
    let input = UpdateSettings {
        logo: Some(logo.map(|n| AssetId::from_uuid(uuid(n)))),
        ..UpdateSettings::default()
    };
    block_on(update_settings(&Ctx::system(), ports, input))
}

#[test]
fn the_logo_is_an_asset_in_use_until_it_changes() {
    let ports = world();
    add_asset(&ports, 1, MimeType::Png, Visibility::Public, true);
    add_asset(&ports, 2, MimeType::Png, Visibility::Public, true);
    let settings = set_logo(&ports, Some(1)).unwrap();
    assert_eq!(settings.logo(), Some(AssetId::from_uuid(uuid(1))));
    assert_eq!(
        usages(&ports, 1),
        [AssetUsage {
            asset: AssetId::from_uuid(uuid(1)),
            owner: UsageOwner::notification_settings(),
            role: UsageRole::logo(),
        }]
    );
    set_logo(&ports, Some(2)).unwrap();
    assert!(usages(&ports, 1).is_empty());
    assert_eq!(usages(&ports, 2).len(), 1);
    set_logo(&ports, None).unwrap();
    assert!(usages(&ports, 2).is_empty());
    let stored = block_on(ports.notification_settings.get())
        .unwrap()
        .unwrap();
    assert_eq!(stored.logo(), None);
}

#[test]
fn the_logo_is_in_every_email() {
    let ports = world();
    add_asset(&ports, 1, MimeType::Png, Visibility::Public, true);
    set_logo(&ports, Some(1)).unwrap();
    let env = TestEnv::new();
    mail_event(&env, &ports, &event(&ports, "order.placed")).unwrap();
    let html = &env.sender.sent()[0].html;
    // As it is: only `& < > " '` are escaped (`render::format_value`).
    let url = format!(
        "src=\"https://cdn.example.com/{}\"",
        AssetId::from_uuid(uuid(1))
    );
    assert!(html.contains(&url), "{html}");
}

#[test]
fn the_logo_must_be_a_ready_public_image() {
    let ports = world();
    add_asset(&ports, 3, MimeType::Mp4, Visibility::Public, true);
    add_asset(&ports, 4, MimeType::Png, Visibility::Private, true);
    add_asset(&ports, 5, MimeType::Png, Visibility::Public, false);
    let aid = |n| AssetId::from_uuid(uuid(n));
    assert_eq!(
        set_logo(&ports, Some(9)),
        Err(CommandError::AssetNotFound(aid(9)))
    );
    assert_eq!(
        set_logo(&ports, Some(3)),
        Err(CommandError::AssetKindNotAllowed {
            asset: aid(3),
            kind: hypha_media::AssetKind::Video
        })
    );
    assert_eq!(
        set_logo(&ports, Some(4)),
        Err(CommandError::AssetNotPublic(aid(4)))
    );
    assert_eq!(
        set_logo(&ports, Some(5)),
        Err(CommandError::AssetNotReady(aid(5)))
    );
    assert_eq!(block_on(ports.notification_settings.get()), Ok(None));
}

#[test]
fn settings_are_checked_and_merged() {
    let ports = world();
    let input = UpdateSettings {
        accent: Some("#FF0000".to_string()),
        sender_name: Some(Some("Fern".to_string())),
        reply_to: Some(Some("help@fernandfield.example".to_string())),
        ..UpdateSettings::default()
    };
    block_on(update_settings(&Ctx::system(), &ports, input)).unwrap();
    let input = UpdateSettings {
        footer: Some(Some("Ilica 1".to_string())),
        ..UpdateSettings::default()
    };
    let settings = block_on(update_settings(&Ctx::system(), &ports, input)).unwrap();
    assert_eq!(settings.accent().as_str(), "#ff0000");
    assert_eq!(settings.sender_name(), Some("Fern"));
    assert_eq!(settings.footer(), Some("Ilica 1"));
    assert_eq!(settings.version(), hypha_core::Version::new(2));
    let bad = UpdateSettings {
        accent: Some("red".to_string()),
        ..UpdateSettings::default()
    };
    assert_eq!(
        block_on(update_settings(&Ctx::system(), &ports, bad)),
        Err(CommandError::Settings(
            hypha_notifications::SettingsError::InvalidColour("red".to_string())
        ))
    );
    let env = TestEnv::new();
    mail_event(&env, &ports, &event(&ports, "order.placed")).unwrap();
    let email = &env.sender.sent()[0];
    assert_eq!(email.from.to_string(), r#""Fern" <shop@mail.example.com>"#);
    assert_eq!(
        email.reply_to,
        Some(self::email("help@fernandfield.example"))
    );
}

#[test]
fn settings_need_notifications_write() {
    let ports = world();
    let ctx = Ctx::new(Actor::User(staff_user()), vec![Scope::NotificationsRead]);
    assert_eq!(
        block_on(update_settings(&ctx, &ports, UpdateSettings::default())),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::NotificationsWrite
        )))
    );
}

// Saving templates.

fn input(subject: &str, html: &str) -> TemplateInput {
    TemplateInput {
        subject: subject.to_string(),
        html: html.to_string(),
        text: None,
    }
}

fn save(
    ports: &InMemoryPorts,
    key: TemplateKey,
    input: TemplateInput,
    enabled: bool,
) -> Result<EffectiveTemplate, CommandError> {
    block_on(save_template(&Ctx::system(), ports, key, input, enabled))
}

#[test]
fn a_saved_template_is_used_from_then_on() {
    let ports = world();
    let saved = save(
        &ports,
        TemplateKey::OrderConfirmation,
        input(
            "Got it: {{ order.number }}",
            r#"{% extends "layout" %}{% block content %}<p>Hi {{ customer.name }}</p>{% endblock %}"#,
        ),
        true,
    )
    .unwrap();
    assert!(saved.customized && saved.enabled);
    let env = TestEnv::new();
    mail_event(&env, &ports, &event(&ports, "order.placed")).unwrap();
    let email = &env.sender.sent()[0];
    assert_eq!(email.subject, "Got it: #1001");
    assert!(email.html.contains("<p>Hi Ana Horvat</p>"));
}

#[test]
fn a_template_that_doesnt_parse_is_refused_with_its_line() {
    let ports = world();
    let result = save(
        &ports,
        TemplateKey::OrderShipped,
        input(
            "Ok",
            "{% extends \"layout\" %}\n{% block content %}\n{% if %}\n{% endblock %}",
        ),
        true,
    );
    let Err(CommandError::Template(TemplateError::Invalid(problem))) = &result else {
        panic!("expected template_invalid, got {result:?}");
    };
    assert_eq!(problem.part, TemplatePart::Html);
    assert_eq!(problem.line, Some(3));
    assert_eq!(result.as_ref().unwrap_err().problem(), Some(problem));
    assert_eq!(block_on(ports.notification_templates.list()), Ok(vec![]));
}

#[test]
fn a_template_naming_a_missing_variable_is_refused_with_its_line() {
    let ports = world();
    let result = save(
        &ports,
        TemplateKey::OrderShipped,
        input(
            "Ok",
            "{% extends \"layout\" %}\n{% block content %}\n<p>{{ order.numbr }}</p>\n{% endblock %}",
        ),
        true,
    );
    let Err(CommandError::Render(RenderError::Invalid(problem))) = &result else {
        panic!("expected template_invalid, got {result:?}");
    };
    assert_eq!(problem.part, TemplatePart::Html);
    assert_eq!(problem.line, Some(3));
    assert_eq!(
        hypha_core::ErrorCode::code(result.as_ref().unwrap_err()),
        "template_invalid"
    );
    // The staff templates have no order.
    let staff = save(
        &ports,
        TemplateKey::StaffInvite,
        input("Join {{ order.number }}", "<p>{{ link }}</p>"),
        true,
    );
    assert!(
        matches!(&staff, Err(CommandError::Render(RenderError::Invalid(p))) if p.part == TemplatePart::Subject),
        "{staff:?}"
    );
}

#[test]
fn a_template_that_needs_an_optional_value_is_refused() {
    let ports = world();
    // Fine on the sample, which has a payment; a cash order without one would break it.
    let result = save(
        &ports,
        TemplateKey::OrderConfirmation,
        input(
            "Ok",
            "{% extends \"layout\" %}\n{% block content %}\n<p>{{ order.payment.method|upper }}</p>\n{% endblock %}",
        ),
        true,
    );
    let Err(CommandError::Render(RenderError::Invalid(problem))) = &result else {
        panic!("expected template_invalid, got {result:?}");
    };
    assert_eq!((problem.part, problem.line), (TemplatePart::Html, Some(3)));
    // Tracking without a URL, likewise.
    let result = save(
        &ports,
        TemplateKey::OrderShipped,
        input(
            "Ok",
            "{% extends \"layout\" %}{% block content %}{% for t in fulfillment.tracking %}{{ t.url.upper() }}{% endfor %}{% endblock %}",
        ),
        true,
    );
    assert!(
        matches!(&result, Err(CommandError::Render(RenderError::Invalid(p))) if p.part == TemplatePart::Html),
        "{result:?}"
    );
    // Guarded, it's saved.
    let guarded = save(
        &ports,
        TemplateKey::OrderConfirmation,
        input(
            "Ok",
            "{% extends \"layout\" %}{% block content %}{% if order.payment %}{{ order.payment.method|upper }}{% endif %}{% endblock %}",
        ),
        true,
    );
    assert!(guarded.is_ok(), "{guarded:?}");
}

#[test]
fn a_layout_that_breaks_a_message_is_refused() {
    let ports = world();
    let layout = TemplateInput {
        subject: String::new(),
        html: "<html>\n{{ order.number }}\n{% block content %}{% endblock %}</html>".to_string(),
        text: None,
    };
    // The order templates have an order; the staff ones don't.
    let result = save(&ports, TemplateKey::Layout, layout, true);
    let Err(CommandError::Render(RenderError::Invalid(problem))) = &result else {
        panic!("expected template_invalid, got {result:?}");
    };
    assert_eq!(problem.part, TemplatePart::Layout);
    assert_eq!(problem.line, Some(2));

    let good = TemplateInput {
        subject: String::new(),
        html: "<html>{{ shop.name }}{% block content %}{% endblock %}</html>".to_string(),
        text: None,
    };
    assert!(save(&ports, TemplateKey::Layout, good, true).is_ok());
}

#[test]
fn templates_that_cant_be_turned_off_stay_on() {
    let ports = world();
    let d = default_template(TemplateKey::StaffInvite);
    let input = TemplateInput {
        subject: d.subject.to_string(),
        html: d.html.to_string(),
        text: d.text.map(str::to_string),
    };
    assert_eq!(
        save(&ports, TemplateKey::StaffInvite, input, false),
        Err(CommandError::Template(TemplateError::Required(
            TemplateKey::StaffInvite
        )))
    );
}

#[test]
fn saving_again_updates_the_stored_template() {
    let ports = world();
    let html = r#"{% extends "layout" %}{% block content %}x{% endblock %}"#;
    save(&ports, TemplateKey::OrderShipped, input("One", html), true).unwrap();
    let saved = save(&ports, TemplateKey::OrderShipped, input("Two", html), false).unwrap();
    assert_eq!(saved.sources.subject(), "Two");
    assert!(!saved.enabled);
    let stored = block_on(
        ports
            .notification_templates
            .get(TemplateKey::OrderShipped, None),
    )
    .unwrap()
    .unwrap();
    assert_eq!(stored.version(), hypha_core::Version::new(2));
}

#[test]
fn saving_needs_notifications_write() {
    let ctx = Ctx::new(Actor::User(staff_user()), vec![Scope::NotificationsRead]);
    assert_eq!(
        block_on(save_template(
            &ctx,
            &world(),
            TemplateKey::OrderShipped,
            input("x", "y"),
            true
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::NotificationsWrite
        )))
    );
}

// Previews and test sends.

fn default_input(key: TemplateKey) -> TemplateInput {
    let d = default_template(key);
    TemplateInput {
        subject: d.subject.to_string(),
        html: d.html.to_string(),
        text: d.text.map(str::to_string),
    }
}

#[test]
fn a_preview_renders_the_sample_with_the_stores_branding() {
    let ports = world();
    let env = TestEnv::new();
    let rendered = block_on(preview(
        &Ctx::system(),
        &env,
        &ports,
        TemplateKey::OrderConfirmation,
        input(
            "{{ order.number }} at {{ shop.name }}",
            r#"{% extends "layout" %}{% block content %}{{ order.payment.method }}{% endblock %}"#,
        ),
        None,
    ))
    .unwrap();
    assert_eq!(rendered.subject, "#1001 at Fern & Field");
    assert!(rendered.html.contains("Bank transfer"));
    // The sample's order, the store's name.
    assert!(rendered.html.contains("Fern &amp; Field"));
}

#[test]
fn a_preview_can_render_a_real_order() {
    let ports = world();
    let env = TestEnv::new();
    let ctx = Ctx::new(
        Actor::User(staff_user()),
        vec![Scope::NotificationsRead, Scope::OrdersRead],
    );
    let rendered = block_on(preview(
        &ctx,
        &env,
        &ports,
        TemplateKey::OrderConfirmation,
        input(
            "{{ order.email }}",
            r#"{% extends "layout" %}{% block content %}x{% endblock %}"#,
        ),
        Some(order_id()),
    ))
    .unwrap();
    assert_eq!(rendered.subject, "ana@example.hr");
    let no_orders = Ctx::new(Actor::User(staff_user()), vec![Scope::NotificationsRead]);
    assert_eq!(
        block_on(preview(
            &no_orders,
            &env,
            &ports,
            TemplateKey::OrderConfirmation,
            default_input(TemplateKey::OrderConfirmation),
            Some(order_id()),
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::OrdersRead
        )))
    );
}

#[test]
fn a_layout_previews_around_the_confirmation() {
    let ports = world();
    let env = TestEnv::new();
    let rendered = block_on(preview(
        &Ctx::system(),
        &env,
        &ports,
        TemplateKey::Layout,
        TemplateInput {
            subject: String::new(),
            html: "<main>FRAME{% block content %}{% endblock %}</main>".to_string(),
            text: None,
        },
        None,
    ))
    .unwrap();
    assert!(rendered.html.starts_with("<main>FRAME"));
    assert!(rendered.html.contains("Thanks for your order"));
}

#[test]
fn a_test_goes_to_the_staff_members_own_address() {
    let ports = world();
    let env = TestEnv::new();
    let ctx = Ctx::new(Actor::User(staff_user()), vec![Scope::NotificationsWrite]);
    let to = block_on(send_test(
        &ctx,
        &env,
        &ports,
        TemplateKey::OrderShipped,
        default_input(TemplateKey::OrderShipped),
        None,
    ))
    .unwrap();
    assert_eq!(to, email("marko@fernandfield.example"));
    let sent = env.sender.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, to);
    assert_eq!(sent[0].subject, "[Test] Your order #1001 is on its way");
    assert!(records(&ports).is_empty(), "tests aren't recorded");
}

#[test]
fn a_test_needs_a_real_sender_and_a_staff_member() {
    let ports = world();
    let logging = TestEnv::with_sender(LogSender);
    let staff = Ctx::new(Actor::User(staff_user()), vec![Scope::NotificationsWrite]);
    let key = TemplateKey::OrderConfirmation;
    assert_eq!(
        block_on(send_test(
            &staff,
            &logging,
            &ports,
            key,
            default_input(key),
            None
        )),
        Err(CommandError::EmailNotConfigured)
    );
    let api_key = Ctx::new(
        Actor::ApiKey(ApiKeyId::from_uuid(uuid(1))),
        vec![Scope::NotificationsWrite],
    );
    let env = TestEnv::new();
    assert_eq!(
        block_on(send_test(
            &api_key,
            &env,
            &ports,
            key,
            default_input(key),
            None
        )),
        Err(CommandError::StaffOnly)
    );
    let failing = TestEnv::new();
    failing
        .sender
        .fail_next(SendError::Permanent("unverified sender".to_string()));
    assert_eq!(
        block_on(send_test(
            &staff,
            &failing,
            &ports,
            key,
            default_input(key),
            None
        )),
        Err(CommandError::Send(SendError::Permanent(
            "unverified sender".to_string()
        )))
    );
}

// Resending.

#[test]
fn a_resend_is_a_new_message_without_an_event() {
    let ports = world();
    ship(&ports, true);
    let env = TestEnv::new();
    let first = mail_event(&env, &ports, &event(&ports, "fulfillment.created"))
        .unwrap()
        .remove(0);
    // Even after the template was turned off: staff asked.
    disable(&ports, TemplateKey::OrderShipped);
    let woken = env.woken();
    let again = block_on(resend(&Ctx::system(), &env, &ports, first.id())).unwrap();
    assert_ne!(again.id(), first.id());
    assert_eq!(again.event(), None);
    assert_eq!(again.order(), Some(order_id()));
    assert_eq!(again.template(), TemplateKey::OrderShipped);
    assert_eq!(again.status(), NotificationStatus::Queued);
    assert_eq!(env.woken(), woken + 1);
    assert_eq!(env.sender.sent().len(), 1, "the send job sends it");
    let done = send_job(&env, &ports);
    assert_eq!(done[0].id(), again.id());
    assert_eq!(done[0].status(), NotificationStatus::Sent);
    let sent = env.sender.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].subject, sent[0].subject);
    assert!(sent[1].text.contains("JD0001"));
}

#[test]
fn staff_emails_cant_be_resent() {
    let ports = world();
    let env = TestEnv::new();
    let invite = block_on(send_staff(&env, &ports, invite_email())).unwrap();
    assert_eq!(
        block_on(resend(&Ctx::system(), &env, &ports, invite.id())),
        Err(CommandError::NotResendable {
            template: TemplateKey::StaffInvite
        })
    );
    let unknown = hypha_core::NotificationId::from_uuid(uuid(77));
    assert_eq!(
        block_on(resend(&Ctx::system(), &env, &ports, unknown)),
        Err(CommandError::NotificationNotFound(unknown))
    );
}

// Staff flows.

fn invite_email() -> StaffEmail {
    StaffEmail::Invite {
        to: email("new@fernandfield.example"),
        token: "hypha_inv_abc".to_string(),
        expires_at: Timestamp::from_unix_micros(1_767_484_800_000_000),
    }
}

#[test]
fn an_invite_is_sent_with_its_link_and_recorded() {
    let ports = world();
    let env = TestEnv::new();
    let record = block_on(send_staff(&env, &ports, invite_email())).unwrap();
    assert_eq!(record.template(), TemplateKey::StaffInvite);
    assert_eq!(record.event(), None);
    assert_eq!(record.order(), None);
    assert_eq!(record.status(), NotificationStatus::Sent);
    let email = &env.sender.sent()[0];
    assert_eq!(email.subject, "You're invited to Fern & Field");
    assert!(
        email
            .text
            .contains("https://admin.example.com/invite?token=hypha_inv_abc"),
        "{}",
        email.text
    );
    // 4 Jan 2026 00:00 UTC is 01:00 in Belgrade.
    assert!(email.text.contains("4 Jan 2026, 01:00"), "{}", email.text);
}

#[test]
fn a_reset_without_an_admin_url_shows_the_bare_token() {
    let ports = world();
    let mut env = TestEnv::new();
    env.admin_url = None;
    let reset = StaffEmail::PasswordReset {
        to: email("marko@fernandfield.example"),
        token: "hypha_pwr_xyz".to_string(),
        expires_at: FixedClock::DEFAULT,
    };
    block_on(send_staff(&env, &ports, reset)).unwrap();
    let email = &env.sender.sent()[0];
    assert_eq!(email.subject, "Reset your Fern & Field password");
    assert!(email.text.contains("token hypha_pwr_xyz"), "{}", email.text);
}

#[test]
fn a_staff_email_that_fails_is_the_flows_error() {
    let ports = world();
    let env = TestEnv::new();
    env.sender
        .fail_next(SendError::Permanent("address refused".to_string()));
    assert_eq!(
        block_on(send_staff(&env, &ports, invite_email())),
        Err(hypha_core::NotifyError("address refused".to_string()))
    );
    env.sender
        .fail_next(SendError::Transient("timed out".to_string()));
    assert_eq!(
        block_on(send_staff(&env, &ports, invite_email())),
        Err(hypha_core::NotifyError("timed out".to_string()))
    );
    let records = records(&ports);
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|r| r.status() == NotificationStatus::Failed)
    );
}

/// Hosts spawn these on multi-threaded runtimes: on `Send` ports and environment, the
/// futures must be `Send` too (nothing `!Send` held across an `.await`).
#[test]
fn the_futures_are_send_on_send_ports() {
    fn send<F: Future + Send>(_: F) {}
    let ports = world();
    let env = TestEnv::new();
    let mailer = mailer();
    let placed = event(&ports, "order.placed");
    let ctx = Ctx::system();
    let key = TemplateKey::OrderConfirmation;
    send(EventHandler::<InMemoryPorts>::handle(
        &mailer, &ports, &placed,
    ));
    send(send_due(&env, &ports, &SendSettings::default()));
    send(update_settings(&ctx, &ports, UpdateSettings::default()));
    send(save_template(&ctx, &ports, key, default_input(key), true));
    send(preview(&ctx, &env, &ports, key, default_input(key), None));
    send(send_test(&ctx, &env, &ports, key, default_input(key), None));
    send(resend(
        &ctx,
        &env,
        &ports,
        hypha_core::NotificationId::from_uuid(uuid(1)),
    ));
    send(send_staff(&env, &ports, invite_email()));
}
