#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]

use hypha_core::{
    Actor, AddressParts, AuthError, CountryError, Ctx, EmailError, LanguageError, Scope, StoreId,
    TimezoneError, UserId,
};
use hypha_stores::{
    CommandError, Store, StoreError, StoreRepo, UpdateStore, create_store, ensure_single_store,
    get_store, update_store,
};
use hypha_testkit::{FixedClock, InMemoryStoreRepo, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn clock() -> FixedClock {
    FixedClock::default()
}

fn user(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), scopes)
}

#[test]
fn ensure_single_store_creates_the_store_on_first_start() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();

    let store = block_on(ensure_single_store(&Ctx::system(), &repo, &ids, "My store")).unwrap();

    assert_eq!(store.name(), "My store");
    assert_eq!(block_on(repo.list_all()).unwrap(), vec![store]);
}

#[test]
fn ensure_single_store_returns_the_existing_store_on_later_starts() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let first = block_on(ensure_single_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();

    let again = block_on(ensure_single_store(&Ctx::system(), &repo, &ids, "Ignored")).unwrap();

    assert_eq!(again, first);
    assert_eq!(block_on(repo.list_all()).unwrap().len(), 1);
}

#[test]
fn ensure_single_store_refuses_more_than_one_store() {
    let repo = InMemoryStoreRepo::default();
    for n in [1, 2] {
        let mut store = Store::new(StoreId::from_uuid(Uuid::from_u128(n)), "Shop").unwrap();
        block_on(repo.insert(&mut store)).unwrap();
    }

    assert_eq!(
        block_on(ensure_single_store(
            &Ctx::system(),
            &repo,
            &SequentialIds::default(),
            "Shop"
        )),
        Err(CommandError::MultipleStores(2))
    );
}

#[test]
fn create_store_validates_the_name() {
    let repo = InMemoryStoreRepo::default();
    assert_eq!(
        block_on(create_store(
            &Ctx::system(),
            &repo,
            &SequentialIds::default(),
            " "
        )),
        Err(CommandError::Store(StoreError::EmptyName))
    );
    assert_eq!(block_on(repo.list_all()), Ok(vec![]));
}

#[test]
fn update_store_stores_the_new_name() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();

    let input = UpdateStore {
        name: Some("Acme EU".to_string()),
        ..UpdateStore::default()
    };
    block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        input,
    ))
    .unwrap();

    let loaded = block_on(get_store(&Ctx::system(), &repo, store.id())).unwrap();
    assert_eq!(loaded.name(), "Acme EU");
}

#[test]
fn update_store_with_nothing_to_change_writes_nothing() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();

    let same = block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        UpdateStore::default(),
    ));
    assert_eq!(same, Ok(store));
}

#[test]
fn get_unknown_store_is_not_found() {
    let id = StoreId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(get_store(&Ctx::system(), &InMemoryStoreRepo::default(), id)),
        Err(CommandError::StoreNotFound(id))
    );
}

#[test]
fn scopes_are_required() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();

    assert_eq!(
        block_on(get_store(&user(vec![]), &repo, store.id())),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::StoresRead
        )))
    );
    assert_eq!(
        block_on(update_store(
            &user(vec![Scope::StoresRead]),
            &repo,
            &clock(),
            store.id(),
            UpdateStore {
                name: Some("X".to_string()),
                ..UpdateStore::default()
            }
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::StoresWrite
        )))
    );
    // Even with nothing to change: the scope doesn't depend on the body.
    assert_eq!(
        block_on(update_store(
            &user(vec![Scope::StoresRead]),
            &repo,
            &clock(),
            store.id(),
            UpdateStore::default()
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::StoresWrite
        )))
    );
}

fn address(country: &str) -> AddressParts {
    AddressParts {
        first_name: None,
        last_name: "Acme".to_string(),
        company: Some("Acme d.o.o.".to_string()),
        line1: "Ilica 1".to_string(),
        line2: None,
        city: "Zagreb".to_string(),
        province: None,
        postal_code: Some("10000".to_string()),
        country: country.to_string(),
        phone: None,
    }
}

#[test]
fn update_store_sets_and_clears_the_details() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();
    let input = UpdateStore {
        contact_email: Some(Some(" Hi@Acme.example ".to_string())),
        description: Some(Some("Mugs and cups.".to_string())),
        legal_name: Some(Some("Acme d.o.o.".to_string())),
        legal_address: Some(Some(address("HR"))),
        timezone: Some(Some("Europe/Zagreb".to_string())),
        default_language: Some("HR".to_string()),
        ..UpdateStore::default()
    };
    block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        input,
    ))
    .unwrap();
    let loaded = block_on(get_store(&Ctx::system(), &repo, store.id())).unwrap();
    assert_eq!(
        loaded.contact_email().map(|e| e.as_str()),
        Some("hi@acme.example")
    );
    assert_eq!(loaded.description(), Some("Mugs and cups."));
    assert_eq!(
        loaded.legal_address().map(|a| a.parts().city.as_str()),
        Some("Zagreb")
    );
    assert_eq!(loaded.timezone().map(|t| t.as_str()), Some("Europe/Zagreb"));
    assert_eq!(loaded.default_language().as_str(), "hr");

    let clear = UpdateStore {
        contact_email: Some(None),
        description: Some(None),
        legal_name: Some(None),
        legal_address: Some(None),
        timezone: Some(None),
        ..UpdateStore::default()
    };
    let cleared = block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        clear,
    ))
    .unwrap();
    assert!(cleared.contact_email().is_none() && cleared.legal_address().is_none());
    assert_eq!(
        (
            cleared.description(),
            cleared.legal_name(),
            cleared.timezone()
        ),
        (None, None, None)
    );
    assert_eq!(cleared.name(), "Acme", "absent fields stay");
}

#[test]
fn update_store_rejects_bad_details_and_changes_nothing() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();
    let update = |input: UpdateStore| {
        block_on(update_store(
            &Ctx::system(),
            &repo,
            &clock(),
            store.id(),
            input,
        ))
    };
    assert_eq!(
        update(UpdateStore {
            contact_email: Some(Some("nope".to_string())),
            ..UpdateStore::default()
        }),
        Err(CommandError::Email(EmailError::Invalid("nope".to_string())))
    );
    assert_eq!(
        update(UpdateStore {
            timezone: Some(Some("Mars/Base".to_string())),
            ..UpdateStore::default()
        }),
        Err(CommandError::Timezone(TimezoneError::Unknown(
            "Mars/Base".to_string()
        )))
    );
    assert_eq!(
        update(UpdateStore {
            default_language: Some("en_GB".to_string()),
            ..UpdateStore::default()
        }),
        Err(CommandError::Language(LanguageError::Invalid(
            "en_GB".to_string()
        )))
    );
    assert_eq!(
        update(UpdateStore {
            legal_address: Some(Some(address("XK"))),
            ..UpdateStore::default()
        }),
        Err(CommandError::Address(hypha_core::AddressError::Country(
            CountryError::Unknown("XK".to_string())
        )))
    );
    assert_eq!(
        update(UpdateStore {
            description: Some(Some("x".repeat(501))),
            ..UpdateStore::default()
        }),
        Err(CommandError::Store(StoreError::DescriptionTooLong {
            max: 500
        }))
    );
    assert_eq!(
        block_on(get_store(&Ctx::system(), &repo, store.id())),
        Ok(store)
    );
}

#[test]
fn update_store_tracks_the_setup_guide() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();
    let step = UpdateStore {
        setup_step: Some(Some("shipping".to_string())),
        ..UpdateStore::default()
    };
    let on = block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        step,
    ))
    .unwrap();
    assert_eq!(
        on.setup().step.as_ref().map(|s| s.as_str()),
        Some("shipping")
    );
    assert_eq!(on.setup().finished_at, None);

    let finish = UpdateStore {
        setup_finished: Some(true),
        ..UpdateStore::default()
    };
    let done = block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        finish,
    ))
    .unwrap();
    assert_eq!(done.setup().finished_at, Some(FixedClock::DEFAULT));
    assert_eq!(
        done.setup().step.as_ref().map(|s| s.as_str()),
        Some("shipping"),
        "kept"
    );

    let reopen = UpdateStore {
        setup_finished: Some(false),
        ..UpdateStore::default()
    };
    let open = block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        reopen,
    ))
    .unwrap();
    assert_eq!(open.setup().finished_at, None);

    let bad = UpdateStore {
        setup_step: Some(Some("Shipping!".to_string())),
        ..UpdateStore::default()
    };
    assert_eq!(
        block_on(update_store(
            &Ctx::system(),
            &repo,
            &clock(),
            store.id(),
            bad
        )),
        Err(CommandError::Store(StoreError::InvalidSetupStep(
            "Shipping!".to_string()
        )))
    );
}
