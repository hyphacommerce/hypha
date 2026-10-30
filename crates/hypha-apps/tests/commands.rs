//! App commands.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]
use hypha_apps::{
    AppError, AppKey, CommandError, CreateApp, SECRET_PREFIX, UpdateApp, WebhookInput, create_app,
    delete_app, get_app, get_event, list_apps, list_deliveries, open_secret, retry_delivery,
    rotate_app_secret, update_app,
};
use hypha_core::{
    Actor, AuthError, Ctx, DeliveryId, DeliveryOutcome, DeliveryStatus, EventId, HasBatch,
    OutboxRepo, Outcome, PageRequest, PendingEvent, Scope, StageEvents, Subject, UserId,
};
use hypha_testkit::{FixedClock, InMemoryAppRepo, InMemoryPorts, TestEnv};
use pollster::block_on;
use uuid::Uuid;

fn erp() -> CreateApp {
    CreateApp {
        key: "app_erp".to_string(),
        name: "ERP".to_string(),
        webhook: Some(WebhookInput {
            url: "https://erp.example/hooks".to_string(),
            timeout_ms: None,
        }),
        subscriptions: vec!["order.*".to_string()],
    }
}

fn reader() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::AppsRead],
    )
}

#[test]
fn an_app_gets_a_secret_shown_once_and_kept_sealed() {
    let apps = InMemoryAppRepo::default();
    let env = TestEnv::default();
    let (app, secret) = block_on(create_app(&Ctx::system(), &apps, &env, &env, erp())).unwrap();
    assert!(secret.starts_with(SECRET_PREFIX));
    assert_eq!(secret.len(), SECRET_PREFIX.len() + 64);
    assert_ne!(app.secret().as_bytes(), secret.as_bytes());
    assert_eq!(app.webhook().map(|w| w.timeout_ms), Some(10_000));
    let stored = block_on(get_app(&reader(), &apps, "app_erp")).unwrap();
    assert_eq!(open_secret(&env, &stored).as_deref(), Ok(secret.as_str()));

    let (_, rotated) = block_on(rotate_app_secret(
        &Ctx::system(),
        &apps,
        &env,
        &env,
        "app_erp",
    ))
    .unwrap();
    assert_ne!(rotated, secret);
    let stored = block_on(get_app(&reader(), &apps, "app_erp")).unwrap();
    assert_eq!(open_secret(&env, &stored).as_deref(), Ok(rotated.as_str()));
}

#[test]
fn a_key_is_taken_once() {
    let apps = InMemoryAppRepo::default();
    let env = TestEnv::default();
    block_on(create_app(&Ctx::system(), &apps, &env, &env, erp())).unwrap();
    assert_eq!(
        block_on(create_app(&Ctx::system(), &apps, &env, &env, erp())).map(|_| ()),
        Err(CommandError::AppExists(AppKey::new("app_erp").unwrap()))
    );
}

#[test]
fn bad_input_is_refused_before_anything_is_stored() {
    let apps = InMemoryAppRepo::default();
    let env = TestEnv::default();
    let cases = [
        (
            CreateApp {
                key: "erp".to_string(),
                ..erp()
            },
            "invalid_app_key",
        ),
        // Spores are built into the API, never registered.
        (
            CreateApp {
                key: "spore_erp".to_string(),
                ..erp()
            },
            "spore_key_reserved",
        ),
        (
            CreateApp {
                subscriptions: vec!["orders".to_string()],
                ..erp()
            },
            "invalid_subscription",
        ),
        (
            CreateApp {
                webhook: Some(WebhookInput {
                    url: "http://erp.example/hooks".to_string(),
                    timeout_ms: None,
                }),
                ..erp()
            },
            "invalid_callback_url",
        ),
    ];
    for (input, code) in cases {
        let result = block_on(create_app(&Ctx::system(), &apps, &env, &env, input));
        assert_eq!(
            result
                .map(|_| ())
                .map_err(|e| hypha_core::ErrorCode::code(&e)),
            Err(code)
        );
    }
    assert_eq!(block_on(list_apps(&reader(), &apps)), Ok(vec![]));
}

#[test]
fn an_update_changes_what_it_names() {
    let apps = InMemoryAppRepo::default();
    let env = TestEnv::default();
    block_on(create_app(&Ctx::system(), &apps, &env, &env, erp())).unwrap();
    let input = UpdateApp {
        enabled: Some(false),
        webhook: Some(None),
        subscriptions: Some(vec!["*".to_string(), "*".to_string()]),
        ..UpdateApp::default()
    };
    let app = block_on(update_app(&Ctx::system(), &apps, "app_erp", input)).unwrap();
    assert_eq!(
        (app.name(), app.enabled(), app.webhook()),
        ("ERP", false, None)
    );
    // Each pattern once.
    assert_eq!(app.subscriptions().len(), 1);
    assert!(!app.wants("order.paid"), "disabled, and no webhook");
    let input = UpdateApp {
        subscriptions: Some(vec!["Orders".to_string()]),
        ..UpdateApp::default()
    };
    assert_eq!(
        block_on(update_app(&Ctx::system(), &apps, "app_erp", input)),
        Err(CommandError::App(AppError::InvalidSubscription(
            "Orders".to_string()
        )))
    );
}

#[test]
fn reading_needs_apps_read_and_writing_apps_write() {
    let apps = InMemoryAppRepo::default();
    let env = TestEnv::default();
    assert_eq!(
        block_on(create_app(&reader(), &apps, &env, &env, erp())).map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::AppsWrite
        )))
    );
    let nobody = Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), vec![]);
    assert_eq!(
        block_on(list_apps(&nobody, &apps)),
        Err(CommandError::Auth(AuthError::MissingScope(Scope::AppsRead)))
    );
}

#[test]
fn a_deleted_app_is_gone() {
    let apps = InMemoryAppRepo::default();
    let env = TestEnv::default();
    block_on(create_app(&Ctx::system(), &apps, &env, &env, erp())).unwrap();
    block_on(delete_app(&Ctx::system(), &apps, "app_erp")).unwrap();
    assert_eq!(
        block_on(get_app(&reader(), &apps, "app_erp")).map(|_| ()),
        Err(CommandError::AppNotFound("app_erp".to_string()))
    );
}

/// `app_erp`, and one event delivered to it (failed for good).
fn delivered() -> (InMemoryPorts, TestEnv) {
    let ports = InMemoryPorts::default();
    let env = TestEnv::default();
    block_on(create_app(&Ctx::system(), &ports.apps, &env, &env, erp())).unwrap();
    let mut batch = ports.batch();
    batch.record_events(vec![PendingEvent {
        event_type: "order.placed",
        version: 1,
        subject: Subject::new("order", Uuid::from_u128(5)),
        data: serde_json::json!({}),
    }]);
    block_on(ports.commit(batch)).unwrap();
    let now = ports.outbox.now();
    block_on(
        ports
            .outbox
            .fan_out(10, now, &|_| vec!["app:app_erp".to_string()]),
    )
    .unwrap();
    let leased = block_on(ports.outbox.lease_due(now, now.plus_secs(60), 10)).unwrap();
    let failed = DeliveryOutcome::Failed {
        error: "answered HTTP 500".to_string(),
    };
    block_on(
        ports
            .outbox
            .record_outcomes(&[Outcome::of(&leased[0], failed)]),
    )
    .unwrap();
    (ports, env)
}

#[test]
fn an_apps_deliveries_are_listed_and_retried() {
    let (ports, _) = delivered();
    let page = block_on(list_deliveries(
        &reader(),
        &ports,
        "app_erp",
        Some(DeliveryStatus::Failed),
        PageRequest::default(),
    ))
    .unwrap();
    assert_eq!(page.items.len(), 1);
    let delivery = DeliveryId::from_uuid(page.items[0].id);

    let clock = FixedClock::default();
    clock.set(FixedClock::DEFAULT.plus_secs(600));
    let writer = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::AppsWrite],
    );
    let retried = block_on(retry_delivery(&writer, &ports, &clock, "app_erp", delivery)).unwrap();
    assert_eq!(
        (retried.status, retried.attempts, retried.next_attempt_at),
        (
            DeliveryStatus::Pending,
            1,
            FixedClock::DEFAULT.plus_secs(600)
        )
    );

    let event = EventId::from_uuid(retried.event_id);
    let stored = block_on(get_event(&reader(), &ports.outbox, event)).unwrap();
    assert_eq!(stored.event_type, "order.placed");
}

#[test]
fn deliveries_are_found_under_their_own_app_only() {
    let (ports, env) = delivered();
    let other = CreateApp {
        key: "app_crm".to_string(),
        ..erp()
    };
    block_on(create_app(&Ctx::system(), &ports.apps, &env, &env, other)).unwrap();
    let page = block_on(list_deliveries(
        &reader(),
        &ports,
        "app_erp",
        None,
        PageRequest::default(),
    ))
    .unwrap();
    let delivery = DeliveryId::from_uuid(page.items[0].id);
    let clock = FixedClock::default();
    assert_eq!(
        block_on(retry_delivery(
            &Ctx::system(),
            &ports,
            &clock,
            "app_crm",
            delivery
        )),
        Err(CommandError::DeliveryNotFound {
            app: AppKey::new("app_crm").unwrap(),
            delivery
        })
    );
    assert_eq!(
        block_on(list_deliveries(
            &reader(),
            &ports,
            "app_none",
            None,
            PageRequest::default()
        ))
        .map(|_| ()),
        Err(CommandError::AppNotFound("app_none".to_string()))
    );
    let missing = EventId::from_uuid(Uuid::from_u128(424242));
    assert_eq!(
        block_on(get_event(&reader(), &ports.outbox, missing)).map(|_| ()),
        Err(CommandError::EventNotFound(missing))
    );
}

#[test]
fn a_key_that_cant_be_one_is_no_app() {
    let apps = InMemoryAppRepo::default();
    assert_eq!(
        block_on(get_app(&reader(), &apps, "erp")).map(|_| ()),
        Err(CommandError::AppNotFound("erp".to_string()))
    );
}
