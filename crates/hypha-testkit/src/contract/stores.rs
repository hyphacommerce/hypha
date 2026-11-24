use hypha_core::{
    Address, AddressParts, Currency, Email, HasOutbox, Language, OutboxRepo, RepoError, StoreId,
    Timestamp, Timezone, Version, Versioned,
};
use hypha_stores::{SetupStep, Store, StoreLanguage, StoreRepo, repo::keys};
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
    scenario("store: its other languages keep their order and whether they're published");
    languages_round_trip(&make().await).await;
}

/// Runs the scenarios of a store's events, each on fresh ports from `make`.
pub async fn store_events<S: StoreRepo + HasOutbox>(make: impl AsyncFn() -> S) {
    scenario("store: a write that changed fields records one store.updated, in its commit");
    events_follow_writes(&make().await).await;
}

async fn events_follow_writes<S: StoreRepo + HasOutbox>(ports: &S) {
    let mut s = store(1, "Acme");
    ports.insert(&mut s).await.unwrap();
    // Nothing changed: the write stands, with no event.
    ports.update(&mut s).await.unwrap();

    let mut stale = s.clone();
    s.rename("Acme EU").unwrap();
    s.set_languages(
        Language::new("hr").unwrap(),
        vec![StoreLanguage {
            language: Language::new("de").unwrap(),
            published: true,
        }],
    )
    .unwrap();
    ports.update(&mut s).await.unwrap();

    // A write that fails leaves no event behind.
    stale.rename("Acme US").unwrap();
    assert_eq!(ports.update(&mut stale).await, Err(RepoError::StaleVersion));

    let events = ports.outbox().events_after(0, 100).await.unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(
        (
            event.event_type.as_str(),
            event.subject_kind.as_str(),
            event.subject_id,
            event.subject_version,
        ),
        (
            "store.updated",
            "store",
            s.id().as_uuid(),
            Some(s.version())
        )
    );
    assert_eq!(
        event.data,
        serde_json::json!({
            "store": s.id(),
            "fields": ["default_language", "languages", "name"]
        })
    );
}

async fn languages_round_trip<R: StoreRepo>(repo: &R) {
    let other = |tag: &str, published| StoreLanguage {
        language: Language::new(tag).unwrap(),
        published,
    };
    let mut s = store(1, "Acme");
    repo.insert(&mut s).await.unwrap();
    assert_eq!(repo.get(s.id()).await.unwrap().unwrap().languages(), &[]);

    // Not in alphabetical order, so the stored order is the list's.
    s.set_languages(
        Language::new("hr").unwrap(),
        vec![
            other("sr-Latn", true),
            other("de", false),
            other("en-GB", true),
        ],
    )
    .unwrap();
    repo.update(&mut s).await.unwrap();
    assert_eq!(repo.get(s.id()).await, Ok(Some(s.clone())));

    s.set_languages(Language::new("hr").unwrap(), vec![other("de", true)])
        .unwrap();
    repo.update(&mut s).await.unwrap();
    assert_eq!(repo.get(s.id()).await, Ok(Some(s.clone())));
    assert_eq!(repo.list_all().await, Ok(vec![s.clone()]));

    s.set_languages(Language::new("hr").unwrap(), Vec::new())
        .unwrap();
    repo.update(&mut s).await.unwrap();
    assert_eq!(repo.get(s.id()).await, Ok(Some(s)));

    // A store inserted with languages keeps them too.
    let mut b = store(2, "Other");
    b.set_languages(Language::new("en").unwrap(), vec![other("fr", false)])
        .unwrap();
    repo.insert(&mut b).await.unwrap();
    assert_eq!(repo.get(b.id()).await, Ok(Some(b)));
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
    s.set_default_language(Language::new("sr-Latn-RS").unwrap())
        .unwrap();
    s.set_currency(Some(Currency::RSD));
    s.set_setup_step(Some(SetupStep::new("shipping").unwrap()));
    // A microsecond past a whole second, so precision is checked too.
    s.finish_setup(Timestamp::from_unix_micros(1_767_225_600_000_001));
    repo.update(&mut s).await.unwrap();
    assert_eq!(repo.get(s.id()).await, Ok(Some(s.clone())));

    s.set_setup_step(None);
    s.reopen_setup();
    s.set_contact_email(None);
    s.set_description(None).unwrap();
    s.set_legal_name(None).unwrap();
    s.set_legal_address(None);
    s.set_timezone(None);
    s.set_currency(None);
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
