#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]

use hypha_core::{
    Actor, AuthError, Ctx, Email, EventId, NotificationId, OrderId, PageLimit, PageRequest, Scope,
    UserId,
};
use hypha_notifications::{
    CommandError, NewNotification, NewTemplate, Notification, NotificationFilter, NotificationRepo,
    NotificationSettings, NotificationSettingsRepo, NotificationStatus, NotificationTemplateRepo,
    Template, TemplateError, TemplateKey, TemplatePart, TemplateSources, default_template,
    get_notification, get_notification_settings, get_notification_template,
    list_notification_templates, list_notifications, revert_notification_template,
    set_notification_template_enabled,
};
use hypha_testkit::InMemoryPorts;
use pollster::block_on;
use uuid::Uuid;

fn ctx() -> Ctx {
    Ctx::system()
}

fn staff(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), scopes)
}

fn edited(key: TemplateKey) -> TemplateSources {
    TemplateSources::new(
        key,
        "Shipped: {{ order.number }}".to_string(),
        r#"{% extends "layout" %}{% block content %}<p>Mine</p>{% endblock %}"#.to_string(),
        None,
    )
    .unwrap()
}

/// Stores `key` as the merchant edited it.
fn store_edit(ports: &InMemoryPorts, key: TemplateKey, enabled: bool) {
    let mut template = Template::new(NewTemplate {
        key,
        locale: None,
        sources: Some(edited(key)),
        enabled,
    })
    .unwrap();
    block_on(ports.notification_templates.insert(&mut template)).unwrap();
}

fn stored(ports: &InMemoryPorts, key: TemplateKey) -> Option<Template> {
    block_on(ports.notification_templates.get(key, None)).unwrap()
}

#[test]
fn settings_are_the_defaults_until_saved() {
    let ports = InMemoryPorts::default();
    assert_eq!(
        block_on(get_notification_settings(&ctx(), &ports)),
        Ok(NotificationSettings::default())
    );
    let mut saved = NotificationSettings::default();
    saved
        .set_footer(Some("Ilica 1, Zagreb".to_string()))
        .unwrap();
    block_on(ports.notification_settings.insert(&mut saved)).unwrap();
    assert_eq!(
        block_on(get_notification_settings(&ctx(), &ports)),
        Ok(saved)
    );
}

#[test]
fn every_template_is_listed_default_or_edited() {
    let ports = InMemoryPorts::default();
    store_edit(&ports, TemplateKey::OrderShipped, false);
    let listed = block_on(list_notification_templates(&ctx(), &ports)).unwrap();
    let keys: Vec<TemplateKey> = listed.iter().map(|t| t.key).collect();
    assert_eq!(keys, TemplateKey::ALL);
    for t in &listed {
        if t.key == TemplateKey::OrderShipped {
            assert!(t.customized && !t.enabled);
            assert_eq!(t.sources, edited(TemplateKey::OrderShipped));
        } else {
            assert!(!t.customized && t.enabled, "{}", t.key);
            assert_eq!(t.sources, default_template(t.key).sources());
        }
    }
}

#[test]
fn one_template_is_its_sources_in_effect() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::OrderConfirmation;
    let default = block_on(get_notification_template(&ctx(), &ports, key)).unwrap();
    assert_eq!(default.sources, default_template(key).sources());
    assert!(!default.customized);
    store_edit(&ports, key, true);
    let mine = block_on(get_notification_template(&ctx(), &ports, key)).unwrap();
    assert_eq!(mine.sources, edited(key));
    assert!(mine.customized);
    assert_eq!(mine.sending_sources(), edited(key));
}

#[test]
fn a_stored_template_that_no_longer_parses_sends_the_default() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::OrderShipped;
    store_edit(&ports, key, true);
    // As if a newer minijinja refused what an older one stored.
    let mut broken = stored(&ports, key).unwrap().snapshot();
    broken.html = Some("{% extends \"layout\" %}{% block content %}{% for %}".to_string());
    let mut broken = Template::restore(broken).unwrap();
    block_on(ports.notification_templates.update(&mut broken)).unwrap();
    let t = block_on(get_notification_template(&ctx(), &ports, key)).unwrap();
    assert_eq!(t.problem.as_ref().map(|p| p.part), Some(TemplatePart::Html));
    assert_eq!(t.sending_sources(), default_template(key).sources());
}

#[test]
fn turning_a_default_off_stores_no_sources_and_on_again_stores_nothing() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::OrderShipped;
    let off = block_on(set_notification_template_enabled(
        &ctx(),
        &ports,
        key,
        false,
    ))
    .unwrap();
    assert!(!off.enabled && !off.customized);
    assert_eq!(off.sources, default_template(key).sources());
    assert_eq!(
        stored(&ports, key).map(|t| t.sources().is_none()),
        Some(true)
    );
    let on = block_on(set_notification_template_enabled(&ctx(), &ports, key, true)).unwrap();
    assert!(on.enabled);
    assert_eq!(stored(&ports, key), None);
}

#[test]
fn turning_an_edited_template_off_keeps_its_sources() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::StaffNewOrder;
    store_edit(&ports, key, true);
    let off = block_on(set_notification_template_enabled(
        &ctx(),
        &ports,
        key,
        false,
    ))
    .unwrap();
    assert!(!off.enabled && off.customized);
    assert_eq!(off.sources, edited(key));
}

#[test]
fn invites_resets_and_the_layout_stay_on() {
    let ports = InMemoryPorts::default();
    for key in [
        TemplateKey::StaffInvite,
        TemplateKey::StaffPasswordReset,
        TemplateKey::Layout,
    ] {
        assert_eq!(
            block_on(set_notification_template_enabled(
                &ctx(),
                &ports,
                key,
                false
            )),
            Err(CommandError::Template(TemplateError::Required(key)))
        );
        assert_eq!(stored(&ports, key), None);
    }
}

#[test]
fn reverting_drops_the_edit_but_keeps_off_off() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::OrderCancelled;
    store_edit(&ports, key, false);
    let reverted = block_on(revert_notification_template(&ctx(), &ports, key)).unwrap();
    assert!(!reverted.customized && !reverted.enabled);
    assert_eq!(reverted.sources, default_template(key).sources());
    assert_eq!(
        stored(&ports, key).map(|t| t.sources().is_none()),
        Some(true)
    );

    let on = TemplateKey::OrderConfirmation;
    store_edit(&ports, on, true);
    block_on(revert_notification_template(&ctx(), &ports, on)).unwrap();
    assert_eq!(stored(&ports, on), None);
}

#[test]
fn reverting_a_default_changes_nothing() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::Layout;
    let reverted = block_on(revert_notification_template(&ctx(), &ports, key)).unwrap();
    assert!(!reverted.customized && reverted.enabled);
    assert_eq!(stored(&ports, key), None);
}

fn nid(n: u128) -> NotificationId {
    NotificationId::from_uuid(Uuid::from_u128(n))
}

fn record(ports: &InMemoryPorts, n: u128, order: Option<u128>) {
    let mut message = Notification::sending(NewNotification {
        id: nid(n),
        template: TemplateKey::OrderConfirmation,
        to: Email::new("ana@example.com").unwrap(),
        subject: format!("Order {n}"),
        event: Some(EventId::from_uuid(Uuid::from_u128(100 + n))),
        order: order.map(|o| OrderId::from_uuid(Uuid::from_u128(o))),
    });
    block_on(ports.notifications.insert(&mut message)).unwrap();
}

#[test]
fn an_orders_messages_newest_first() {
    let ports = InMemoryPorts::default();
    record(&ports, 1, Some(7));
    record(&ports, 2, Some(8));
    record(&ports, 3, Some(7));
    let filter = NotificationFilter {
        order: Some(OrderId::from_uuid(Uuid::from_u128(7))),
        ..NotificationFilter::default()
    };
    let page = block_on(list_notifications(
        &ctx(),
        &ports,
        filter,
        PageRequest {
            after: None,
            limit: PageLimit::new(10).unwrap(),
        },
    ))
    .unwrap();
    let ids: Vec<NotificationId> = page.items.iter().map(Notification::id).collect();
    assert_eq!(ids, [nid(3), nid(1)]);
    assert!(!page.has_more);
    assert!(
        page.items
            .iter()
            .all(|n| n.status() == NotificationStatus::Sending)
    );
}

#[test]
fn a_message_is_found_by_its_id() {
    let ports = InMemoryPorts::default();
    record(&ports, 1, None);
    assert_eq!(
        block_on(get_notification(&ctx(), &ports, nid(1))).map(|n| n.id()),
        Ok(nid(1))
    );
    assert_eq!(
        block_on(get_notification(&ctx(), &ports, nid(2))),
        Err(CommandError::NotificationNotFound(nid(2)))
    );
}

#[test]
fn reading_messages_needs_orders_read_too() {
    let ports = InMemoryPorts::default();
    record(&ports, 1, Some(7));
    let settings_only = staff(vec![Scope::NotificationsWrite]);
    let missing = Err(CommandError::Auth(AuthError::MissingScope(
        Scope::OrdersRead,
    )));
    assert_eq!(
        block_on(get_notification(&settings_only, &ports, nid(1))).map(|_| ()),
        missing
    );
    let page = PageRequest {
        after: None,
        limit: PageLimit::new(10).unwrap(),
    };
    assert_eq!(
        block_on(list_notifications(
            &settings_only,
            &ports,
            NotificationFilter::default(),
            page,
        ))
        .map(|_| ()),
        missing
    );
    let both = staff(vec![Scope::NotificationsRead, Scope::OrdersRead]);
    assert_eq!(
        block_on(get_notification(&both, &ports, nid(1))).map(|n| n.id()),
        Ok(nid(1))
    );
}

#[test]
fn reading_needs_notifications_read_and_changing_needs_write() {
    let ports = InMemoryPorts::default();
    let nobody = staff(vec![Scope::OrdersRead]);
    let missing = |scope| Err(CommandError::Auth(AuthError::MissingScope(scope)));
    assert_eq!(
        block_on(get_notification_settings(&nobody, &ports)).map(|_| ()),
        missing(Scope::NotificationsRead)
    );
    assert_eq!(
        block_on(list_notification_templates(&nobody, &ports)).map(|_| ()),
        missing(Scope::NotificationsRead)
    );
    assert_eq!(
        block_on(get_notification(&nobody, &ports, nid(1))).map(|_| ()),
        missing(Scope::NotificationsRead)
    );
    let reader = staff(vec![Scope::NotificationsRead]);
    assert_eq!(
        block_on(revert_notification_template(
            &reader,
            &ports,
            TemplateKey::OrderShipped
        ))
        .map(|_| ()),
        missing(Scope::NotificationsWrite)
    );
    assert_eq!(
        block_on(set_notification_template_enabled(
            &reader,
            &ports,
            TemplateKey::OrderShipped,
            false
        ))
        .map(|_| ()),
        missing(Scope::NotificationsWrite)
    );
    // Write implies read.
    let writer = staff(vec![Scope::NotificationsWrite]);
    assert!(block_on(get_notification_settings(&writer, &ports)).is_ok());
}
