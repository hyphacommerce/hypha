//! Notifications (design note 0028): stored templates by (key, locale), the store's
//! one settings record (also in a batch with the logo's asset usage), and the record of
//! messages with its (event, template, recipient) key.

use hypha_core::{
    AssetId, Email, EventId, HasBatch, Language, NotificationId, OrderId, PageLimit, PageRequest,
    RepoError, Timestamp, Version, Versioned,
};
use hypha_media::{
    AssetRepo, AssetUsage, AssetUsageRepo, HasAssetUsages, HasAssets, MimeType, StageAssetUsages,
    UsageOwner, UsageRole, repo::keys as media_keys,
};
use hypha_notifications::{
    Colour, HasNotificationSettings, NewNotification, NewTemplate, Notification,
    NotificationFilter, NotificationRepo, NotificationSettings, NotificationSettingsRepo,
    NotificationStatus, NotificationTemplateRepo, OrderUrl, StageNotificationSettings, Template,
    TemplateKey, TemplateSources, keys,
};
use uuid::Uuid;

use super::{
    media::{aid, ready},
    scenario,
};

/// Runs every `NotificationTemplateRepo` scenario, each on a fresh repo from `make`.
pub async fn notification_template_repo<R: NotificationTemplateRepo>(make: impl AsyncFn() -> R) {
    scenario("notification template: insert stores every field at version 1");
    template_round_trips(&make().await).await;
    scenario("notification template: update stores every field; stale and missing fail");
    template_updates(&make().await).await;
    scenario("notification template: unique (key, locale)");
    template_unique_key(&make().await).await;
    scenario("notification template: listed by key, then locale");
    templates_listed_in_order(&make().await).await;
    scenario("notification template: delete at its version; stale and missing fail");
    template_deletes(&make().await).await;
}

/// Runs every `NotificationSettingsRepo` scenario, each on a fresh repo from `make`.
pub async fn notification_settings_repo<R: NotificationSettingsRepo>(make: impl AsyncFn() -> R) {
    scenario("notification settings: none until inserted, then every field");
    settings_round_trip(&make().await).await;
    scenario("notification settings: one per store");
    settings_once(&make().await).await;
    scenario("notification settings: update stores every field; stale fails");
    settings_update(&make().await).await;
}

/// Runs every `NotificationRepo` scenario, each on a fresh repo from `make`.
pub async fn notification_repo<R: NotificationRepo>(make: impl AsyncFn() -> R) {
    scenario("notification: insert stores every field at version 1");
    notification_round_trips(&make().await).await;
    scenario("notification: update stores every field; stale and missing fail");
    notification_updates(&make().await).await;
    scenario("notification: unique ID");
    notification_unique_id(&make().await).await;
    scenario("notification: one per (event, template, recipient) where the event is set");
    notification_unique_message(&make().await).await;
    scenario("notification: found by (event, template, recipient)");
    notification_found(&make().await).await;
    scenario("notification: listed newest first, filtered, in pages");
    notifications_listed(&make().await).await;
}

/// Runs the batch scenarios for notification settings: the settings and their logo's
/// asset usage commit together, or not at all.
pub async fn notification_settings_batch<S>(make: impl AsyncFn() -> S)
where
    S: HasBatch + HasNotificationSettings + HasAssets + HasAssetUsages,
    S::Batch: StageNotificationSettings + StageAssetUsages,
{
    scenario("notification settings batch: inserts and updates with the logo's usage");
    settings_with_logo(&make().await).await;
    scenario("notification settings batch: a missing logo writes no settings");
    missing_logo_writes_nothing(&make().await).await;
    scenario("notification settings batch: stale settings write no usage");
    stale_settings_write_no_usage(&make().await).await;
    scenario("notification settings batch: inserting twice fails");
    settings_inserted_twice(&make().await).await;
}

fn email(s: &str) -> Email {
    Email::new(s).unwrap()
}

fn at(micros: i64) -> Timestamp {
    Timestamp::from_unix_micros(1_767_225_600_000_000 + micros)
}

fn sources(key: TemplateKey, n: u32) -> TemplateSources {
    TemplateSources::new(
        key,
        format!("Order {{{{ order.number }}}} shipped ({n})"),
        format!(
            "{{% extends \"layout\" %}}{{% block content %}}<p>Tracking: {{{{ fulfillment.tracking }}}}</p>\n<p>{n} — é</p>{{% endblock %}}"
        ),
        Some(format!("Your order {{{{ order.number }}}} shipped.\n\n{n}")),
    )
    .unwrap()
}

fn template(key: TemplateKey, locale: Option<&str>, n: u32) -> Template {
    Template::new(NewTemplate {
        key,
        locale: locale.map(|l| Language::new(l).unwrap()),
        sources: Some(sources(key, n)),
        enabled: true,
    })
    .unwrap()
}

/// Turned off without edits: no sources.
fn bare_template(key: TemplateKey) -> Template {
    Template::new(NewTemplate {
        key,
        locale: None,
        sources: None,
        enabled: false,
    })
    .unwrap()
}

async fn template_round_trips<R: NotificationTemplateRepo>(repo: &R) {
    let mut full = template(TemplateKey::OrderShipped, None, 1);
    repo.insert(&mut full).await.unwrap();
    assert_eq!(full.version(), Version::new(1));
    assert_eq!(
        repo.get(TemplateKey::OrderShipped, None).await,
        Ok(Some(full))
    );
    let mut bare = bare_template(TemplateKey::StaffNewOrder);
    repo.insert(&mut bare).await.unwrap();
    assert_eq!(
        repo.get(TemplateKey::StaffNewOrder, None).await,
        Ok(Some(bare))
    );
    let mut layout = Template::new(NewTemplate {
        key: TemplateKey::Layout,
        locale: None,
        sources: Some(
            TemplateSources::new(
                TemplateKey::Layout,
                String::new(),
                "<html>{% block content %}{% endblock %}</html>".to_string(),
                None,
            )
            .unwrap(),
        ),
        enabled: true,
    })
    .unwrap();
    repo.insert(&mut layout).await.unwrap();
    assert_eq!(repo.get(TemplateKey::Layout, None).await, Ok(Some(layout)));
    let mut translated = template(TemplateKey::OrderShipped, Some("sr-Latn"), 2);
    repo.insert(&mut translated).await.unwrap();
    let sr = Language::new("sr-Latn").unwrap();
    assert_eq!(
        repo.get(TemplateKey::OrderShipped, Some(&sr)).await,
        Ok(Some(translated))
    );
    assert_eq!(repo.get(TemplateKey::OrderCancelled, None).await, Ok(None));
    let en = Language::new("en").unwrap();
    assert_eq!(
        repo.get(TemplateKey::OrderShipped, Some(&en)).await,
        Ok(None)
    );
}

async fn template_updates<R: NotificationTemplateRepo>(repo: &R) {
    let mut t = template(TemplateKey::OrderConfirmation, None, 1);
    repo.insert(&mut t).await.unwrap();
    let stale = t.clone();
    t.set_sources(Some(sources(TemplateKey::OrderConfirmation, 2)));
    t.set_enabled(false).unwrap();
    repo.update(&mut t).await.unwrap();
    assert_eq!(t.version(), Version::new(2));
    assert_eq!(
        repo.get(TemplateKey::OrderConfirmation, None).await,
        Ok(Some(t.clone()))
    );
    // Back to the default's sources, still off: the columns go back to NULL.
    t.set_sources(None);
    repo.update(&mut t).await.unwrap();
    assert_eq!(
        repo.get(TemplateKey::OrderConfirmation, None).await,
        Ok(Some(t))
    );

    let mut stale = stale;
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    let mut missing = template(TemplateKey::OrderCancelled, None, 1);
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

async fn template_unique_key<R: NotificationTemplateRepo>(repo: &R) {
    repo.insert(&mut template(TemplateKey::OrderShipped, None, 1))
        .await
        .unwrap();
    assert_eq!(
        repo.insert(&mut template(TemplateKey::OrderShipped, None, 2))
            .await,
        Err(RepoError::Duplicate(keys::TEMPLATE_KEY))
    );
    // The same key in another language is another template.
    repo.insert(&mut template(TemplateKey::OrderShipped, Some("de"), 3))
        .await
        .unwrap();
    assert_eq!(
        repo.insert(&mut template(TemplateKey::OrderShipped, Some("de"), 4))
            .await,
        Err(RepoError::Duplicate(keys::TEMPLATE_KEY))
    );
}

async fn templates_listed_in_order<R: NotificationTemplateRepo>(repo: &R) {
    assert_eq!(repo.list().await, Ok(vec![]));
    let mut staff = bare_template(TemplateKey::StaffNewOrder);
    let mut shipped_de = template(TemplateKey::OrderShipped, Some("de"), 1);
    let mut shipped = template(TemplateKey::OrderShipped, None, 2);
    let mut confirmation = template(TemplateKey::OrderConfirmation, None, 3);
    for t in [&mut staff, &mut shipped_de, &mut shipped, &mut confirmation] {
        repo.insert(t).await.unwrap();
    }
    let listed: Vec<(TemplateKey, Option<String>)> = repo
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|t| (t.key(), t.locale().map(|l| l.as_str().to_string())))
        .collect();
    assert_eq!(
        listed,
        vec![
            (TemplateKey::OrderConfirmation, None),
            (TemplateKey::OrderShipped, None),
            (TemplateKey::OrderShipped, Some("de".to_string())),
            (TemplateKey::StaffNewOrder, None),
        ]
    );
}

async fn template_deletes<R: NotificationTemplateRepo>(repo: &R) {
    let mut t = template(TemplateKey::OrderShipped, None, 1);
    repo.insert(&mut t).await.unwrap();
    let mut other_language = template(TemplateKey::OrderShipped, Some("de"), 2);
    repo.insert(&mut other_language).await.unwrap();
    let stale = t.clone();
    t.set_enabled(false).unwrap();
    repo.update(&mut t).await.unwrap();
    assert_eq!(repo.delete(&stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.delete(&t).await, Ok(()));
    assert_eq!(repo.get(TemplateKey::OrderShipped, None).await, Ok(None));
    assert_eq!(repo.delete(&t).await, Err(RepoError::StaleVersion));
    // Only that language's row went.
    let de = Language::new("de").unwrap();
    assert_eq!(
        repo.get(TemplateKey::OrderShipped, Some(&de)).await,
        Ok(Some(other_language))
    );
}

/// Every field set.
fn full_settings() -> NotificationSettings {
    let mut s = NotificationSettings::default();
    s.set_logo(Some(aid(1)));
    s.set_accent(Colour::new("#0a7c4f").unwrap());
    s.set_footer(Some("Kafa d.o.o.\nIlica 1, Zagreb".to_string()))
        .unwrap();
    s.set_sender_name(Some("Kafa, \"the\" shop".to_string()))
        .unwrap();
    s.set_reply_to(Some(email("help@kafa.example")));
    s.set_staff_recipients(vec![email("b@kafa.example"), email("a@kafa.example")])
        .unwrap();
    s.set_order_url(Some(
        OrderUrl::new("https://shop.kafa.example/order/{order}?token={token}").unwrap(),
    ));
    s
}

async fn settings_round_trip<R: NotificationSettingsRepo>(repo: &R) {
    assert_eq!(repo.get().await, Ok(None));
    let mut s = full_settings();
    repo.insert(&mut s).await.unwrap();
    assert_eq!(s.version(), Version::new(1));
    assert_eq!(repo.get().await, Ok(Some(s)));
}

async fn settings_once<R: NotificationSettingsRepo>(repo: &R) {
    repo.insert(&mut NotificationSettings::default())
        .await
        .unwrap();
    assert_eq!(
        repo.insert(&mut full_settings()).await,
        Err(RepoError::Duplicate(keys::SETTINGS))
    );
}

async fn settings_update<R: NotificationSettingsRepo>(repo: &R) {
    let mut s = full_settings();
    repo.insert(&mut s).await.unwrap();
    let stale = s.clone();
    // Every optional field cleared: a mapping bug would keep the old values.
    let mut cleared = s.clone();
    cleared.set_logo(None);
    cleared.set_footer(None).unwrap();
    cleared.set_sender_name(None).unwrap();
    cleared.set_reply_to(None);
    cleared.set_staff_recipients(vec![]).unwrap();
    cleared.set_order_url(None);
    repo.update(&mut cleared).await.unwrap();
    assert_eq!(cleared.version(), Version::new(2));
    assert_eq!(repo.get().await, Ok(Some(cleared)));
    let mut stale = stale;
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
}

fn nid(n: u128) -> NotificationId {
    NotificationId::from_uuid(Uuid::from_u128(n))
}

fn eid(n: u128) -> EventId {
    EventId::from_uuid(Uuid::from_u128(500 + n))
}

fn oid(n: u128) -> OrderId {
    OrderId::from_uuid(Uuid::from_u128(900 + n))
}

fn message(
    n: u128,
    template: TemplateKey,
    to: &str,
    event: Option<u128>,
    order: Option<u128>,
) -> Notification {
    Notification::sending(NewNotification {
        id: nid(n),
        template,
        to: email(to),
        subject: format!("Order #{n} — confirmed"),
        event: event.map(eid),
        order: order.map(oid),
    })
}

async fn notification_round_trips<R: NotificationRepo>(repo: &R) {
    let mut sent = message(
        1,
        TemplateKey::OrderConfirmation,
        "ana@example.com",
        Some(1),
        Some(1),
    );
    sent.sent("resend", Some("re_123".to_string()), at(5))
        .unwrap();
    repo.insert(&mut sent).await.unwrap();
    assert_eq!(sent.version(), Version::new(1));
    assert_eq!(repo.get(nid(1)).await, Ok(Some(sent)));

    let mut failed = message(2, TemplateKey::StaffInvite, "new@example.com", None, None);
    failed.failed("smtp", "550 mailbox unavailable").unwrap();
    repo.insert(&mut failed).await.unwrap();
    assert_eq!(repo.get(nid(2)).await, Ok(Some(failed)));

    let mut skipped = Notification::skipped(
        NewNotification {
            id: nid(3),
            template: TemplateKey::OrderShipped,
            to: email("ana@example.com"),
            subject: "Shipped".to_string(),
            event: Some(eid(3)),
            order: Some(oid(1)),
        },
        "the template is turned off",
    );
    repo.insert(&mut skipped).await.unwrap();
    assert_eq!(repo.get(nid(3)).await, Ok(Some(skipped)));
    assert_eq!(repo.get(nid(99)).await, Ok(None));
}

async fn notification_updates<R: NotificationRepo>(repo: &R) {
    let mut n = message(
        1,
        TemplateKey::OrderShipped,
        "ana@example.com",
        Some(1),
        Some(1),
    );
    repo.insert(&mut n).await.unwrap();
    let stale = n.clone();
    n.failed("resend", "timed out").unwrap();
    repo.update(&mut n).await.unwrap();
    assert_eq!(n.version(), Version::new(2));
    assert_eq!(repo.get(nid(1)).await, Ok(Some(n.clone())));
    n.retry().unwrap();
    n.sent("resend", Some("re_9".to_string()), at(9)).unwrap();
    repo.update(&mut n).await.unwrap();
    assert_eq!(repo.get(nid(1)).await, Ok(Some(n)));

    let mut stale = stale;
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    let mut missing = message(2, TemplateKey::OrderShipped, "ana@example.com", None, None);
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

async fn notification_unique_id<R: NotificationRepo>(repo: &R) {
    repo.insert(&mut message(
        1,
        TemplateKey::OrderShipped,
        "a@example.com",
        None,
        None,
    ))
    .await
    .unwrap();
    assert_eq!(
        repo.insert(&mut message(
            1,
            TemplateKey::OrderCancelled,
            "b@example.com",
            None,
            None
        ))
        .await,
        Err(RepoError::Duplicate(keys::NOTIFICATION_ID))
    );
}

async fn notification_unique_message<R: NotificationRepo>(repo: &R) {
    let to = "ana@example.com";
    repo.insert(&mut message(
        1,
        TemplateKey::OrderConfirmation,
        to,
        Some(1),
        Some(1),
    ))
    .await
    .unwrap();
    assert_eq!(
        repo.insert(&mut message(
            2,
            TemplateKey::OrderConfirmation,
            to,
            Some(1),
            Some(1)
        ))
        .await,
        Err(RepoError::Duplicate(keys::NOTIFICATION_MESSAGE))
    );
    // Another template, recipient or event is another message.
    for (n, template, to, event) in [
        (3, TemplateKey::StaffNewOrder, to, 1),
        (4, TemplateKey::OrderConfirmation, "staff@example.com", 1),
        (5, TemplateKey::OrderConfirmation, to, 2),
    ] {
        repo.insert(&mut message(n, template, to, Some(event), Some(1)))
            .await
            .unwrap();
    }
    // Without an event (resends, direct sends), as many as wanted.
    for n in [6, 7] {
        repo.insert(&mut message(
            n,
            TemplateKey::OrderConfirmation,
            to,
            None,
            Some(1),
        ))
        .await
        .unwrap();
    }
}

async fn notification_found<R: NotificationRepo>(repo: &R) {
    let to = email("ana@example.com");
    let mut n = message(1, TemplateKey::OrderShipped, to.as_str(), Some(1), Some(1));
    repo.insert(&mut n).await.unwrap();
    repo.insert(&mut message(
        2,
        TemplateKey::OrderShipped,
        to.as_str(),
        None,
        Some(1),
    ))
    .await
    .unwrap();
    assert_eq!(
        repo.find(eid(1), TemplateKey::OrderShipped, &to).await,
        Ok(Some(n))
    );
    for (event, template, to) in [
        (2, TemplateKey::OrderShipped, to.clone()),
        (1, TemplateKey::OrderCancelled, to.clone()),
        (1, TemplateKey::OrderShipped, email("other@example.com")),
    ] {
        assert_eq!(repo.find(eid(event), template, &to).await, Ok(None));
    }
}

async fn notifications_listed<R: NotificationRepo>(repo: &R) {
    // IDs in creation order: 1 oldest.
    let mut failed = message(
        4,
        TemplateKey::OrderShipped,
        "a@example.com",
        Some(4),
        Some(1),
    );
    failed.failed("smtp", "refused").unwrap();
    for mut n in [
        message(
            1,
            TemplateKey::OrderConfirmation,
            "a@example.com",
            Some(1),
            Some(1),
        ),
        message(
            2,
            TemplateKey::StaffNewOrder,
            "s@example.com",
            Some(1),
            Some(1),
        ),
        message(
            3,
            TemplateKey::OrderConfirmation,
            "b@example.com",
            Some(3),
            Some(2),
        ),
        failed,
        message(5, TemplateKey::StaffInvite, "new@example.com", None, None),
    ] {
        repo.insert(&mut n).await.unwrap();
    }
    let listed = async |filter: NotificationFilter, after: Option<u128>, limit: u32| {
        let page = repo
            .list(
                filter,
                PageRequest {
                    after: after.map(nid),
                    limit: PageLimit::new(limit).unwrap(),
                },
            )
            .await
            .unwrap();
        let ids: Vec<NotificationId> = page.items.iter().map(Notification::id).collect();
        (ids, page.has_more)
    };
    let all = NotificationFilter::default();
    assert_eq!(listed(all, None, 2).await, (vec![nid(5), nid(4)], true));
    assert_eq!(listed(all, Some(4), 2).await, (vec![nid(3), nid(2)], true));
    assert_eq!(listed(all, Some(2), 2).await, (vec![nid(1)], false));
    let order = NotificationFilter {
        order: Some(oid(1)),
        ..NotificationFilter::default()
    };
    assert_eq!(
        listed(order, None, 10).await,
        (vec![nid(4), nid(2), nid(1)], false)
    );
    let confirmations = NotificationFilter {
        template: Some(TemplateKey::OrderConfirmation),
        ..NotificationFilter::default()
    };
    assert_eq!(
        listed(confirmations, None, 10).await,
        (vec![nid(3), nid(1)], false)
    );
    let failed = NotificationFilter {
        status: Some(NotificationStatus::Failed),
        ..NotificationFilter::default()
    };
    assert_eq!(listed(failed, None, 10).await, (vec![nid(4)], false));
    let together = NotificationFilter {
        order: Some(oid(1)),
        template: Some(TemplateKey::OrderConfirmation),
        status: Some(NotificationStatus::Sending),
    };
    assert_eq!(listed(together, None, 10).await, (vec![nid(1)], false));
}

async fn logo_usages<S: HasAssetUsages>(ports: &S, asset: AssetId) -> Vec<AssetUsage> {
    let page = PageRequest {
        after: None,
        limit: PageLimit::new(10).unwrap(),
    };
    ports
        .asset_usages()
        .list_usages(asset, page)
        .await
        .unwrap()
        .items
}

fn logo_usage(asset: AssetId) -> AssetUsage {
    AssetUsage {
        asset,
        owner: UsageOwner::notification_settings(),
        role: UsageRole::logo(),
    }
}

/// Stages `settings` (inserted when `new`) with exactly `logo`'s usage, and commits.
async fn commit_settings<S>(
    ports: &S,
    settings: &mut NotificationSettings,
    new: bool,
) -> Result<(), RepoError>
where
    S: HasBatch,
    S::Batch: StageNotificationSettings + StageAssetUsages,
{
    let usages: Vec<(AssetId, UsageRole)> = settings
        .logo()
        .map(|logo| (logo, UsageRole::logo()))
        .into_iter()
        .collect();
    let mut batch = ports.batch();
    if new {
        batch.insert_notification_settings(settings);
    } else {
        batch.update_notification_settings(settings);
    }
    batch.replace_asset_usages(&UsageOwner::notification_settings(), &usages);
    ports.commit(batch).await
}

async fn settings_with_logo<S>(ports: &S)
where
    S: HasBatch + HasNotificationSettings + HasAssets + HasAssetUsages,
    S::Batch: StageNotificationSettings + StageAssetUsages,
{
    for n in [1, 2] {
        ports
            .assets()
            .insert(&mut ready(n, MimeType::Png))
            .await
            .unwrap();
    }
    let mut s = full_settings();
    commit_settings(ports, &mut s, true).await.unwrap();
    assert_eq!(s.version(), Version::new(1));
    assert_eq!(
        ports.notification_settings().get().await,
        Ok(Some(s.clone()))
    );
    assert_eq!(logo_usages(ports, aid(1)).await, vec![logo_usage(aid(1))]);

    s.set_logo(Some(aid(2)));
    commit_settings(ports, &mut s, false).await.unwrap();
    assert_eq!(s.version(), Version::new(2));
    assert_eq!(ports.notification_settings().get().await, Ok(Some(s)));
    assert_eq!(logo_usages(ports, aid(1)).await, vec![]);
    assert_eq!(logo_usages(ports, aid(2)).await, vec![logo_usage(aid(2))]);
}

async fn missing_logo_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasNotificationSettings + HasAssets + HasAssetUsages,
    S::Batch: StageNotificationSettings + StageAssetUsages,
{
    let mut s = full_settings();
    assert_eq!(
        commit_settings(ports, &mut s, true).await,
        Err(RepoError::Reference(media_keys::USAGE_ASSET))
    );
    assert_eq!(ports.notification_settings().get().await, Ok(None));
}

async fn stale_settings_write_no_usage<S>(ports: &S)
where
    S: HasBatch + HasNotificationSettings + HasAssets + HasAssetUsages,
    S::Batch: StageNotificationSettings + StageAssetUsages,
{
    ports
        .assets()
        .insert(&mut ready(1, MimeType::Png))
        .await
        .unwrap();
    let mut s = NotificationSettings::default();
    ports.notification_settings().insert(&mut s).await.unwrap();
    let mut stale = s.clone();
    s.set_footer(Some("Moved".to_string())).unwrap();
    ports.notification_settings().update(&mut s).await.unwrap();
    stale.set_logo(Some(aid(1)));
    assert_eq!(
        commit_settings(ports, &mut stale, false).await,
        Err(RepoError::StaleVersion)
    );
    assert_eq!(logo_usages(ports, aid(1)).await, vec![]);
    assert_eq!(ports.notification_settings().get().await, Ok(Some(s)));
}

async fn settings_inserted_twice<S>(ports: &S)
where
    S: HasBatch + HasNotificationSettings + HasAssets + HasAssetUsages,
    S::Batch: StageNotificationSettings + StageAssetUsages,
{
    ports
        .notification_settings()
        .insert(&mut NotificationSettings::default())
        .await
        .unwrap();
    let mut again = NotificationSettings::default();
    assert_eq!(
        commit_settings(ports, &mut again, true).await,
        Err(RepoError::Duplicate(keys::SETTINGS))
    );
}
