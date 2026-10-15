//! External apps (design note 0016, section 8): every field round trips, the key is
//! unique, versions hold.

use hypha_apps::{App, AppKey, AppRepo, NewApp, Webhook, keys};
use hypha_core::{RepoError, Sealed, Version, Versioned};

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
