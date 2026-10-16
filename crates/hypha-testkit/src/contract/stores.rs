use hypha_core::{
    Address, AddressParts, Email, Language, RepoError, StoreId, Timezone, Version, Versioned,
};
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
    scenario("store: every detail is set, stored and cleared");
    details_round_trip(&make().await).await;
}

async fn details_round_trip<R: StoreRepo>(repo: &R) {
    let mut s = store(1, "Acme");
    repo.insert(&mut s).await.unwrap();
    s.set_contact_email(Some(Email::new("hi@acme.example").unwrap()));
    s.set_description(Some("Mugs, cups\nand plates.")).unwrap();
    s.set_legal_name(Some("Acme d.o.o.")).unwrap();
    let address = Address::new(AddressParts {
        first_name: Some("Ana".to_string()),
        last_name: "Horvat".to_string(),
        company: Some("Acme d.o.o.".to_string()),
        line1: "Ilica 1".to_string(),
        line2: Some("2. kat".to_string()),
        city: "Zagreb".to_string(),
        province: Some("Grad Zagreb".to_string()),
        postal_code: Some("10000".to_string()),
        country: "HR".to_string(),
        phone: Some("+385 1 000 000".to_string()),
    })
    .unwrap();
    s.set_legal_address(Some(address));
    s.set_timezone(Some(Timezone::new("Europe/Zagreb").unwrap()));
    s.set_default_language(Language::new("sr-Latn-RS").unwrap());
    repo.update(&mut s).await.unwrap();
    assert_eq!(repo.get(s.id()).await, Ok(Some(s.clone())));

    s.set_contact_email(None);
    s.set_description(None).unwrap();
    s.set_legal_name(None).unwrap();
    s.set_legal_address(None);
    s.set_timezone(None);
    repo.update(&mut s).await.unwrap();
    assert_eq!(repo.get(s.id()).await, Ok(Some(s)));
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
