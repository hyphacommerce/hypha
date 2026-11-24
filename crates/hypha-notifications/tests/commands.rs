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
    Actor, AuthError, Ctx, Email, EventId, Language, NotificationId, OrderId, PageLimit,
    PageRequest, Scope, UserId,
};
use hypha_notifications::{
    CommandError, NewNotification, NewTemplate, Notification, NotificationFilter, NotificationRepo,
    NotificationSettings, NotificationSettingsRepo, NotificationStatus, NotificationTemplateRepo,
    Template, TemplateError, TemplateKey, TemplateLanguages, TemplatePart, TemplateSources,
    default_template, default_template_in, get_notification, get_notification_settings,
    get_notification_template, list_notification_templates, list_notifications,
    revert_notification_template, set_notification_template_enabled,
};
use hypha_testkit::InMemoryPorts;
use pollster::block_on;
use uuid::Uuid;

fn en() -> Language {
    Language::new("en").unwrap()
}

/// An English store that also has Serbian and German.
fn langs() -> TemplateLanguages<'static> {
    static OTHERS: std::sync::LazyLock<Vec<Language>> = std::sync::LazyLock::new(|| {
        ["sr-Latn", "de"]
            .into_iter()
            .map(|t| Language::new(t).unwrap())
            .collect()
    });
    static DEFAULT: std::sync::LazyLock<Language> = std::sync::LazyLock::new(en);
    TemplateLanguages {
        default: &DEFAULT,
        others: &OTHERS,
    }
}

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
    let listed = block_on(list_notification_templates(&ctx(), &ports, &en())).unwrap();
    let keys: Vec<TemplateKey> = listed.iter().map(|t| t.template.key).collect();
    assert_eq!(keys, TemplateKey::ALL);
    for t in listed.iter().map(|l| &l.template) {
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
    let default = block_on(get_notification_template(
        &ctx(),
        &ports,
        key,
        None,
        langs(),
    ))
    .unwrap();
    assert_eq!(default.sources, default_template(key).sources());
    assert!(!default.customized);
    store_edit(&ports, key, true);
    let mine = block_on(get_notification_template(
        &ctx(),
        &ports,
        key,
        None,
        langs(),
    ))
    .unwrap();
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
    let t = block_on(get_notification_template(
        &ctx(),
        &ports,
        key,
        None,
        langs(),
    ))
    .unwrap();
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
        &en(),
    ))
    .unwrap();
    assert!(!off.enabled && !off.customized);
    assert_eq!(off.sources, default_template(key).sources());
    assert_eq!(
        stored(&ports, key).map(|t| t.sources().is_none()),
        Some(true)
    );
    let on = block_on(set_notification_template_enabled(
        &ctx(),
        &ports,
        key,
        true,
        &en(),
    ))
    .unwrap();
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
        &en(),
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
                false,
                &en()
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
    let reverted = block_on(revert_notification_template(
        &ctx(),
        &ports,
        key,
        None,
        langs(),
    ))
    .unwrap();
    assert!(!reverted.customized && !reverted.enabled);
    assert_eq!(reverted.sources, default_template(key).sources());
    assert_eq!(
        stored(&ports, key).map(|t| t.sources().is_none()),
        Some(true)
    );

    let on = TemplateKey::OrderConfirmation;
    store_edit(&ports, on, true);
    block_on(revert_notification_template(
        &ctx(),
        &ports,
        on,
        None,
        langs(),
    ))
    .unwrap();
    assert_eq!(stored(&ports, on), None);
}

#[test]
fn reverting_a_default_changes_nothing() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::Layout;
    let reverted = block_on(revert_notification_template(
        &ctx(),
        &ports,
        key,
        None,
        langs(),
    ))
    .unwrap();
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
        purchase_order: None,
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
        block_on(list_notification_templates(&nobody, &ports, &en())).map(|_| ()),
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
            TemplateKey::OrderShipped,
            None,
            langs()
        ))
        .map(|_| ()),
        missing(Scope::NotificationsWrite)
    );
    assert_eq!(
        block_on(set_notification_template_enabled(
            &reader,
            &ports,
            TemplateKey::OrderShipped,
            false,
            &en()
        ))
        .map(|_| ()),
        missing(Scope::NotificationsWrite)
    );
    // Write implies read.
    let writer = staff(vec![Scope::NotificationsWrite]);
    assert!(block_on(get_notification_settings(&writer, &ports)).is_ok());
}

fn lang(tag: &str) -> Language {
    Language::new(tag).unwrap()
}

/// Stores `key` for `locale` with sources whose subject names it.
fn store_in(ports: &InMemoryPorts, key: TemplateKey, locale: Option<&str>) -> TemplateSources {
    let sources = TemplateSources::new(
        key,
        format!("{}: {{{{ order.number }}}}", locale.unwrap_or("default")),
        r#"{% extends "layout" %}{% block content %}<p>Mine</p>{% endblock %}"#.to_string(),
        None,
    )
    .unwrap();
    let mut template = Template::new(NewTemplate {
        key,
        locale: locale.map(lang),
        sources: Some(sources.clone()),
        enabled: true,
    })
    .unwrap();
    block_on(ports.notification_templates.insert(&mut template)).unwrap();
    sources
}

#[test]
fn the_chain_tries_the_merchants_then_the_builds_by_region_then_the_default_language() {
    use hypha_notifications::{Step, lookup_chain};
    let m = |tag: Option<&str>| Step::Merchant(tag.map(lang));
    let d = |tag: &str| Step::Default(lang(tag));
    assert_eq!(
        lookup_chain(&lang("de-AT"), &en()),
        vec![
            m(Some("de-AT")),
            m(Some("de")),
            d("de-AT"),
            d("de"),
            m(None),
            d("en")
        ]
    );
    // The default language is the `''` row, never one keyed by it.
    assert_eq!(
        lookup_chain(&lang("de-AT"), &lang("de")),
        vec![m(Some("de-AT")), m(None), d("de-AT"), d("de"), d("en")]
    );
    assert_eq!(lookup_chain(&en(), &en()), vec![m(None), d("en")]);
    // A script isn't a region: Serbian in Latin script never falls back to Cyrillic.
    assert_eq!(
        lookup_chain(&lang("sr-Latn"), &en()),
        vec![m(Some("sr-Latn")), d("sr-Latn"), m(None), d("en")]
    );
}

#[test]
fn an_email_in_a_regional_language_takes_the_merchants_language_before_the_builds() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::OrderConfirmation;
    let de = store_in(&ports, key, Some("de"));
    store_in(&ports, key, None);
    let at = block_on(hypha_notifications::template_in_effect(
        &ports,
        key,
        Some(&lang("de-AT")),
        &en(),
    ))
    .unwrap();
    assert_eq!(at.sending_sources(), de);
    assert_eq!(at.language(), &lang("de"));
    assert!(!at.customized, "nothing saved for de-AT itself");
    let mine = store_in(&ports, key, Some("de-AT"));
    let at = block_on(hypha_notifications::template_in_effect(
        &ports,
        key,
        Some(&lang("de-AT")),
        &en(),
    ))
    .unwrap();
    assert_eq!((at.sending_sources(), at.customized), (mine, true));
}

#[test]
fn the_builds_language_comes_before_the_merchants_default_language() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::OrderConfirmation;
    store_in(&ports, key, None);
    let serbian = block_on(hypha_notifications::template_in_effect(
        &ports,
        key,
        Some(&lang("sr-Latn")),
        &en(),
    ))
    .unwrap();
    let built = default_template_in(key, &lang("sr-Latn")).unwrap();
    assert_eq!(serbian.sending_sources(), built.sources());
    assert_eq!(serbian.language(), &lang("sr-Latn"));
    // A language the build has nothing in reads the merchant's default language.
    let german = block_on(hypha_notifications::template_in_effect(
        &ports,
        key,
        Some(&lang("de")),
        &en(),
    ))
    .unwrap();
    assert_eq!(german.language(), &en());
    assert!(german.sends_merchants());
}

#[test]
fn a_store_in_another_language_sends_the_builds_defaults_in_it() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::OrderShipped;
    let serbian = lang("sr-Latn");
    let t = block_on(hypha_notifications::template_in_effect(
        &ports, key, None, &serbian,
    ))
    .unwrap();
    assert_eq!(
        t.sending_sources(),
        default_template_in(key, &serbian).unwrap().sources()
    );
    // Staff emails have English defaults only.
    let staff = block_on(hypha_notifications::template_in_effect(
        &ports,
        TemplateKey::StaffNewOrder,
        None,
        &serbian,
    ))
    .unwrap();
    assert_eq!(staff.language(), &en());
}

#[test]
fn a_languages_template_is_read_reverted_and_listed_apart_from_the_default() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::OrderShipped;
    let mine = store_in(&ports, key, Some("sr-Latn"));
    let sr = lang("sr-Latn");
    let got = block_on(get_notification_template(
        &ctx(),
        &ports,
        key,
        Some(&sr),
        langs(),
    ))
    .unwrap();
    assert_eq!(
        (got.sources, got.customized, got.locale),
        (mine, true, Some(sr.clone()))
    );
    let listed = block_on(list_notification_templates(&ctx(), &ports, &en())).unwrap();
    let shipped = listed.iter().find(|l| l.template.key == key).unwrap();
    assert_eq!(shipped.languages, vec![sr.clone()]);
    assert!(!shipped.template.customized);
    let reverted = block_on(revert_notification_template(
        &ctx(),
        &ports,
        key,
        Some(&sr),
        langs(),
    ))
    .unwrap();
    assert!(!reverted.customized);
    assert_eq!(
        reverted.sources,
        default_template_in(key, &sr).unwrap().sources()
    );
    assert_eq!(
        block_on(ports.notification_templates.list()).unwrap(),
        vec![]
    );
}

#[test]
fn the_default_language_named_is_the_default_row_and_others_must_be_the_stores() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::OrderShipped;
    let got = block_on(get_notification_template(
        &ctx(),
        &ports,
        key,
        Some(&en()),
        langs(),
    ))
    .unwrap();
    assert_eq!(got.locale, None);
    assert_eq!(
        block_on(get_notification_template(
            &ctx(),
            &ports,
            key,
            Some(&lang("fr")),
            langs()
        ))
        .map(|_| ()),
        Err(CommandError::LanguageNotListed(lang("fr")))
    );
}

#[test]
fn whether_a_template_is_sent_is_the_default_rows_for_every_language() {
    let ports = InMemoryPorts::default();
    let key = TemplateKey::OrderShipped;
    store_in(&ports, key, Some("sr-Latn"));
    block_on(set_notification_template_enabled(
        &ctx(),
        &ports,
        key,
        false,
        &en(),
    ))
    .unwrap();
    let sr = block_on(get_notification_template(
        &ctx(),
        &ports,
        key,
        Some(&lang("sr-Latn")),
        langs(),
    ))
    .unwrap();
    assert!(!sr.enabled);
}
