//! External apps (design note 0016, section 8): every field round trips, the key is
//! unique, versions hold.

use hypha_apps::{
    App, AppKey, AppRepo, Grant, HasApps, InstalledManifest, NewApp, NewSpore, Webhook, keys,
};
use hypha_core::{
    Actor, AppId, EventRoute, HasBatch, HasOutbox, OutboxRepo, PageRequest, PendingEvent,
    RepoError, Scope, Sealed, StageEvents, Subject, Timestamp, UserId, Version, Versioned,
};
use uuid::Uuid;

use super::scenario;

/// Runs every `AppRepo` scenario, each on a fresh repo from `make`.
pub async fn app_repo<R: AppRepo>(make: impl AsyncFn() -> R) {
    scenario("app: insert stores every field at version 1; the key is unique");
    round_trips(&make().await).await;
    scenario("app: update stores changes, a stale one fails; list by key; delete");
    updates(&make().await).await;
    scenario("app: an installed app's manifest, grant and uninstall round trip; by ID");
    installed(&make().await).await;
    scenario("app: a spore's app round trips, its grant approved by the host");
    spore(&make().await).await;
}

async fn spore<R: AppRepo>(repo: &R) {
    let mut app = App::spore(NewSpore {
        name: "stripe",
        id: id_of("spore_stripe"),
        title: "Stripe",
        secret: Sealed::from_bytes(vec![6; 40]),
        scopes: &[Scope::PaymentsWrite, Scope::OrdersWrite],
        at: Timestamp::from_unix_micros(1_800_000_000_000_000),
    })
    .unwrap();
    repo.insert(&mut app).await.unwrap();
    let stored = repo.get(&key("spore_stripe")).await.unwrap().unwrap();
    assert_eq!(stored, app);
    assert_eq!(stored.grant().map(|g| g.approved_by()), Some(Actor::System));
    assert_eq!(repo.get_by_id(app.id()).await, Ok(Some(app)));
}

/// A stable ID per key, so two apps never share one by accident.
pub(crate) fn id_of(k: &str) -> AppId {
    AppId::from_uuid(Uuid::from_u128(
        k.bytes()
            .fold(7u128, |a, b| a.wrapping_mul(31).wrapping_add(u128::from(b))),
    ))
}

/// App `k` installed from `crate::app_manifest`, granted `scopes`.
pub(crate) fn installed_app(k: &str, scopes: &[Scope]) -> App {
    let at = Timestamp::from_unix_micros(1_800_000_000_000_000);
    App::install(
        id_of(k),
        Sealed::from_bytes(vec![5; 40]),
        InstalledManifest {
            url: "https://app.example/hypha-app.json".to_string(),
            manifest: crate::app_manifest(k, "https://app.example"),
            fetched_at: at,
        },
        Grant::new(
            scopes,
            Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
            at,
        ),
    )
    .unwrap()
}

async fn installed<R: AppRepo>(repo: &R) {
    let mut app = installed_app("app_shop", &[Scope::OrdersRead, Scope::CustomerDataEmail]);
    repo.insert(&mut app).await.unwrap();
    assert_eq!(repo.get(&key("app_shop")).await, Ok(Some(app.clone())));
    assert_eq!(repo.get_by_id(app.id()).await, Ok(Some(app.clone())));
    assert_eq!(repo.get_by_id(id_of("app_none")).await, Ok(None));
    let stored = repo.get(&key("app_shop")).await.unwrap().unwrap();
    assert_eq!(
        stored.grant().map(|g| g.scopes().to_vec()),
        Some(vec![Scope::OrdersRead, Scope::CustomerDataEmail])
    );
    assert_eq!(
        stored.webhook().map(|w| w.url.as_str().to_string()),
        Some("https://app.example/hooks".to_string())
    );

    // Another key with the same ID breaks `APP_ID`.
    let mut twin = installed_app("app_twin", &[]);
    let mut snapshot = twin.snapshot();
    snapshot.id = app.id();
    twin = App::restore(snapshot).unwrap();
    assert_eq!(
        repo.insert(&mut twin).await,
        Err(RepoError::Duplicate(keys::APP_ID))
    );

    app.uninstall(Timestamp::from_unix_micros(1_800_000_100_000_000))
        .unwrap();
    repo.update(&mut app).await.unwrap();
    let stored = repo.get(&key("app_shop")).await.unwrap().unwrap();
    assert_eq!(stored, app);
    assert_eq!(stored.grant(), None);
    assert!(stored.uninstalled_at().is_some());
    assert!(stored.wants_event(&EventRoute {
        event_type: "app.uninstalled",
        subject_kind: "app",
        subject_id: app.id().as_uuid(),
    }));
}

fn key(k: &str) -> AppKey {
    AppKey::new(k).unwrap()
}

fn app(k: &str) -> App {
    App::new(NewApp {
        key: key(k),
        id: id_of(k),
        name: "ERP".to_string(),
        secret: Sealed::from_bytes(vec![0, 1, 254, 255]),
        webhook: Some(Webhook::new("https://erp.example/hooks", Some(30_000)).unwrap()),
        subscriptions: vec!["order.*".to_string(), "product.created".to_string()],
    })
    .unwrap()
}

async fn round_trips<R: AppRepo>(repo: &R) {
    assert_eq!(repo.get(&key("app_erp")).await, Ok(None));
    let mut erp = app("app_erp");
    erp.set_enabled(false);
    repo.insert(&mut erp).await.unwrap();
    assert_eq!(erp.version(), Version::new(1));
    assert_eq!(repo.get(&key("app_erp")).await, Ok(Some(erp)));
    assert_eq!(
        repo.insert(&mut app("app_erp")).await,
        Err(RepoError::Duplicate(keys::APP_KEY))
    );
}

async fn updates<R: AppRepo>(repo: &R) {
    let mut zeta = app("app_zeta");
    repo.insert(&mut zeta).await.unwrap();
    let mut alpha = app("app_alpha");
    alpha.set_webhook(None);
    alpha.set_subscriptions(vec![]).unwrap();
    repo.insert(&mut alpha).await.unwrap();

    let stale = zeta.clone();
    zeta.rename("Zeta ERP").unwrap();
    zeta.set_webhook(None);
    zeta.set_subscriptions(vec!["*".to_string()]).unwrap();
    zeta.set_secret(Sealed::from_bytes(vec![9; 40]));
    repo.update(&mut zeta).await.unwrap();
    assert_eq!(zeta.version(), Version::new(2));
    assert_eq!(repo.get(&key("app_zeta")).await, Ok(Some(zeta.clone())));
    assert_eq!(
        repo.update(&mut stale.clone()).await,
        Err(RepoError::StaleVersion)
    );

    assert_eq!(repo.list().await, Ok(vec![alpha, zeta]));
    repo.delete(&key("app_zeta")).await.unwrap();
    assert_eq!(repo.get(&key("app_zeta")).await, Ok(None));
    assert_eq!(repo.delete(&key("app_zeta")).await, Ok(()));
}

/// Deleting an app deletes its deliveries, so an app made later under the same key
/// doesn't inherit the old one's queue or history.
pub async fn app_deliveries<S>(make: impl AsyncFn() -> S)
where
    S: HasApps + HasOutbox + HasBatch,
    S::Batch: StageEvents,
{
    scenario("app: deleting an app deletes its deliveries, and only its");
    deletes_deliveries(&make().await).await;
}

async fn deletes_deliveries<S>(store: &S)
where
    S: HasApps + HasOutbox + HasBatch,
    S::Batch: StageEvents,
{
    for k in ["app_erp", "app_crm"] {
        store.apps().insert(&mut app(k)).await.unwrap();
    }
    let mut batch = store.batch();
    batch.record_events(vec![PendingEvent {
        event_type: "order.placed",
        version: 1,
        subject: Subject::new("order", Uuid::from_u128(5)),
        data: serde_json::json!({}),
    }]);
    store.commit(batch).await.unwrap();
    let now = Timestamp::from_unix_micros(1_800_000_000_000_000);
    let both = |_: &EventRoute<'_>| vec!["app:app_erp".to_string(), "app:app_crm".to_string()];
    store.outbox().fan_out(10, now, &both).await.unwrap();

    store.apps().delete(&key("app_erp")).await.unwrap();
    let count = async |destination: &str| {
        let page = store
            .outbox()
            .deliveries_to(destination, None, PageRequest::default())
            .await
            .unwrap();
        page.items.len()
    };
    assert_eq!(count("app:app_erp").await, 0);
    assert_eq!(count("app:app_crm").await, 1);
}
