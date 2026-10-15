//! External apps (design note 0016, section 8): every field round trips, the key is
//! unique, versions hold.

use hypha_apps::{App, AppKey, AppRepo, HasApps, NewApp, Webhook, keys};
use hypha_core::{
    HasBatch, HasOutbox, OutboxRepo, PageRequest, PendingEvent, RepoError, Sealed, StageEvents,
    Subject, Timestamp, Version, Versioned,
};
use uuid::Uuid;

use super::scenario;

/// Runs every `AppRepo` scenario, each on a fresh repo from `make`.
pub async fn app_repo<R: AppRepo>(make: impl AsyncFn() -> R) {
    scenario("app: insert stores every field at version 1; the key is unique");
    round_trips(&make().await).await;
    scenario("app: update stores changes, a stale one fails; list by key; delete");
    updates(&make().await).await;
}

fn key(k: &str) -> AppKey {
    AppKey::new(k).unwrap()
}

fn app(k: &str) -> App {
    App::new(NewApp {
        key: key(k),
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
    let both = |_: &str| vec!["app:app_erp".to_string(), "app:app_crm".to_string()];
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
