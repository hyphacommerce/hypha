use hypha_core::{RepoError, StoreId, Version, Versioned};
use hypha_stores::{Store, StoreRepo, repo::keys};
use uuid::Uuid;

use super::scenario;

/// Runs every `StoreRepo` scenario, each on a fresh repo from `make`.
pub async fn store_repo<R: StoreRepo>(make: impl AsyncFn() -> R) {
    scenario("store: insert stores every field at version 1");
    round_trips(&make().await).await;
    scenario("store: update stores changes and bumps the version");
    update_round_trips(&make().await).await;
    scenario("store: unique ID, stale and missing updates");
    keys_and_stale(&make().await).await;
}

fn store(n: u128, name: &str) -> Store {
    Store::new(StoreId::from_uuid(Uuid::from_u128(n)), name).unwrap()
}

async fn round_trips<R: StoreRepo>(repo: &R) {
    assert_eq!(repo.list_all().await, Ok(vec![]));
    assert_eq!(
        repo.get(StoreId::from_uuid(Uuid::from_u128(99))).await,
        Ok(None)
    );

    // Non-ASCII must survive the round trip.
    let mut a = store(1, "Prodavnica Čačak");
    let mut b = store(2, "Acme");
    repo.insert(&mut a).await.unwrap();
    repo.insert(&mut b).await.unwrap();
    assert_eq!(a.version(), Version::new(1));
    assert_eq!(repo.get(a.id()).await, Ok(Some(a.clone())));

    let mut all = repo.list_all().await.unwrap();
    all.sort_by_key(|s| s.id().as_uuid());
    assert_eq!(all, vec![a, b]);
}

async fn update_round_trips<R: StoreRepo>(repo: &R) {
    let mut s = store(1, "Acme");
    repo.insert(&mut s).await.unwrap();
    s.rename("Acme EU").unwrap();
    repo.update(&mut s).await.unwrap();
    assert_eq!(s.version(), Version::new(2));
    assert_eq!(repo.get(s.id()).await, Ok(Some(s)));
}

async fn keys_and_stale<R: StoreRepo>(repo: &R) {
    let mut s = store(1, "Acme");
    repo.insert(&mut s).await.unwrap();
    assert_eq!(
        repo.insert(&mut store(1, "Other")).await,
        Err(RepoError::Duplicate(keys::STORE_ID))
    );

    let mut stale = s.clone();
    s.rename("Acme EU").unwrap();
    repo.update(&mut s).await.unwrap();
    stale.rename("Acme US").unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(s.id()).await, Ok(Some(s)));

    assert_eq!(
        repo.update(&mut store(9, "Missing")).await,
        Err(RepoError::StaleVersion)
    );
}
