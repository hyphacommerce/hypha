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
    Actor, AddressParts, AuthError, CountryError, Ctx, Currency, CurrencyError, EmailError,
    Language, LanguageError, MarketId, Scope, StoreId, TimezoneError, UserId, Versioned,
};
use hypha_stores::{
    CommandError, LanguageInput, MarketLanguages, Store, StoreError, StoreLanguage, StoreRepo,
    UpdateStore, create_store, ensure_single_store, get_store, update_store,
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
fn a_new_store_has_no_currency_until_one_is_chosen() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();
    assert_eq!(store.currency(), None);
    let update = |currency: Option<&str>| {
        let input = UpdateStore {
            currency: Some(currency.map(str::to_string)),
            ..UpdateStore::default()
        };
        block_on(update_store(
            &Ctx::system(),
            &repo,
            &clock(),
            store.id(),
            input,
        ))
    };

    assert_eq!(update(Some("EUR")).unwrap().currency(), Some(Currency::EUR));
    assert_eq!(
        block_on(get_store(&Ctx::system(), &repo, store.id()))
            .unwrap()
            .currency(),
        Some(Currency::EUR)
    );
    assert_eq!(update(None).unwrap().currency(), None);
}

#[test]
fn the_currency_is_fixed_once_a_cost_is_recorded() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();
    let update = |currency: Option<&str>, locked: bool| {
        let input = UpdateStore {
            currency: Some(currency.map(str::to_string)),
            currency_locked: locked,
            ..UpdateStore::default()
        };
        block_on(update_store(
            &Ctx::system(),
            &repo,
            &clock(),
            store.id(),
            input,
        ))
    };
    update(Some("EUR"), false).unwrap();

    assert_eq!(update(Some("USD"), true), Err(CommandError::CurrencyLocked));
    assert_eq!(update(None, true), Err(CommandError::CurrencyLocked));
    assert_eq!(
        update(Some("EUR"), true).unwrap().currency(),
        Some(Currency::EUR),
        "the same currency again is no change"
    );
    let stored = block_on(get_store(&Ctx::system(), &repo, store.id())).unwrap();
    assert_eq!(stored.currency(), Some(Currency::EUR));
}

#[test]
fn a_lock_alone_changes_nothing() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();
    let input = UpdateStore {
        currency_locked: true,
        ..UpdateStore::default()
    };
    let same = block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        input,
    ))
    .unwrap();
    assert_eq!(same.version(), store.version());
}

#[test]
fn update_store_refuses_an_unknown_currency() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();
    let input = UpdateStore {
        currency: Some(Some("eur".to_string())),
        ..UpdateStore::default()
    };
    assert_eq!(
        block_on(update_store(
            &Ctx::system(),
            &repo,
            &clock(),
            store.id(),
            input
        )),
        Err(CommandError::Currency(CurrencyError::Unknown(
            "eur".to_string()
        )))
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

fn lang(tag: &str) -> Language {
    Language::new(tag).unwrap()
}

fn listed(tag: &str, published: bool) -> LanguageInput {
    LanguageInput {
        language: tag.to_string(),
        published,
    }
}

fn offering(market: u128, tags: &[&str]) -> MarketLanguages {
    MarketLanguages {
        market: MarketId::from_uuid(Uuid::from_u128(market)),
        languages: tags.iter().map(|t| lang(t)).collect(),
    }
}

/// A store whose default is `en`, with `de` published and `fr` not.
fn store_in_three_languages(repo: &InMemoryStoreRepo) -> Store {
    let store = block_on(create_store(
        &Ctx::system(),
        repo,
        &SequentialIds::default(),
        "Acme",
    ))
    .unwrap();
    let input = UpdateStore {
        languages: Some(vec![listed("DE", true), listed("fr", false)]),
        ..UpdateStore::default()
    };
    block_on(update_store(
        &Ctx::system(),
        repo,
        &clock(),
        store.id(),
        input,
    ))
    .unwrap()
}

#[test]
fn update_store_replaces_the_languages() {
    let repo = InMemoryStoreRepo::default();
    let store = store_in_three_languages(&repo);
    assert_eq!(
        store.languages(),
        &[
            StoreLanguage {
                language: lang("de"),
                published: true
            },
            StoreLanguage {
                language: lang("fr"),
                published: false
            },
        ]
    );
    let stored = block_on(get_store(&Ctx::system(), &repo, store.id())).unwrap();
    assert_eq!(stored, store);

    let cleared = block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        UpdateStore {
            languages: Some(Vec::new()),
            ..UpdateStore::default()
        },
    ))
    .unwrap();
    assert_eq!(cleared.languages(), &[]);
}

#[test]
fn update_store_refuses_languages_that_break_a_rule() {
    let repo = InMemoryStoreRepo::default();
    let store = store_in_three_languages(&repo);
    let update = |default: Option<&str>, languages: Option<Vec<LanguageInput>>| {
        let input = UpdateStore {
            default_language: default.map(str::to_string),
            languages,
            ..UpdateStore::default()
        };
        block_on(update_store(
            &Ctx::system(),
            &repo,
            &clock(),
            store.id(),
            input,
        ))
    };
    assert_eq!(
        update(None, Some(vec![listed("de_DE", true)])),
        Err(CommandError::Language(LanguageError::Invalid(
            "de_DE".to_string()
        )))
    );
    assert_eq!(
        update(None, Some(vec![listed("en", true)])),
        Err(CommandError::Store(StoreError::LanguageIsDefault(lang(
            "en"
        ))))
    );
    assert_eq!(
        update(None, Some(vec![listed("de", true), listed("de", false)])),
        Err(CommandError::Store(StoreError::DuplicateLanguage(lang(
            "de"
        ))))
    );
    assert_eq!(
        update(Some("de"), None),
        Err(CommandError::Store(StoreError::DefaultLanguageListed(
            lang("de")
        )))
    );
    assert_eq!(
        update(Some("de"), Some(vec![listed("de", true)])),
        Err(CommandError::Store(StoreError::LanguageIsDefault(lang(
            "de"
        ))))
    );
    let stored = block_on(get_store(&Ctx::system(), &repo, store.id())).unwrap();
    assert_eq!(stored, store, "nothing changed");
}

#[test]
fn the_default_and_another_language_swap_in_one_update() {
    let repo = InMemoryStoreRepo::default();
    let store = store_in_three_languages(&repo);
    let input = UpdateStore {
        default_language: Some("de".to_string()),
        languages: Some(vec![listed("en", true), listed("fr", false)]),
        ..UpdateStore::default()
    };
    let swapped = block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        input,
    ))
    .unwrap();
    assert_eq!(swapped.offered_languages(), vec![lang("de"), lang("en")]);
}

#[test]
fn the_default_language_is_fixed_while_the_store_has_translations() {
    let repo = InMemoryStoreRepo::default();
    let store = store_in_three_languages(&repo);
    let update = |input: UpdateStore| {
        block_on(update_store(
            &Ctx::system(),
            &repo,
            &clock(),
            store.id(),
            UpdateStore {
                has_translations: true,
                ..input
            },
        ))
    };
    // Alone.
    assert_eq!(
        update(UpdateStore {
            default_language: Some("es".to_string()),
            ..UpdateStore::default()
        }),
        Err(CommandError::DefaultLanguageInUse)
    );
    // Swapped with a language on the list.
    assert_eq!(
        update(UpdateStore {
            default_language: Some("de".to_string()),
            languages: Some(vec![listed("en", true), listed("fr", false)]),
            ..UpdateStore::default()
        }),
        Err(CommandError::DefaultLanguageInUse)
    );
    let stored = block_on(get_store(&Ctx::system(), &repo, store.id())).unwrap();
    assert_eq!(stored.default_language(), &lang("en"));

    // The same default again, and the other languages, change freely.
    let kept = update(UpdateStore {
        default_language: Some("EN".to_string()),
        languages: Some(vec![listed("de", true)]),
        ..UpdateStore::default()
    })
    .unwrap();
    assert_eq!(kept.offered_languages(), vec![lang("en"), lang("de")]);
}

#[test]
fn translations_alone_change_nothing() {
    let repo = InMemoryStoreRepo::default();
    let store = store_in_three_languages(&repo);
    let same = block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        UpdateStore {
            has_translations: true,
            ..UpdateStore::default()
        },
    ))
    .unwrap();
    assert_eq!(same.version(), store.version());
}

#[test]
fn a_language_a_market_offers_stays_offered() {
    let repo = InMemoryStoreRepo::default();
    let store = store_in_three_languages(&repo);
    let update = |default: Option<&str>, languages: Option<Vec<LanguageInput>>| {
        let input = UpdateStore {
            default_language: default.map(str::to_string),
            languages,
            market_languages: vec![
                offering(1, &["en", "de"]),
                offering(2, &["de"]),
                offering(3, &[]),
            ],
            ..UpdateStore::default()
        };
        block_on(update_store(
            &Ctx::system(),
            &repo,
            &clock(),
            store.id(),
            input,
        ))
    };
    // Removed.
    assert_eq!(
        update(None, Some(vec![listed("fr", false)])),
        Err(CommandError::LanguageInUse(vec![
            offering(1, &["de"]),
            offering(2, &["de"])
        ]))
    );
    // Unpublished.
    assert_eq!(
        update(None, Some(vec![listed("de", false), listed("fr", false)])),
        Err(CommandError::LanguageInUse(vec![
            offering(1, &["de"]),
            offering(2, &["de"])
        ]))
    );
    // The default replaced by a language no market names.
    assert_eq!(
        update(Some("hr"), None),
        Err(CommandError::LanguageInUse(vec![offering(1, &["en"])]))
    );
    let stored = block_on(get_store(&Ctx::system(), &repo, store.id())).unwrap();
    assert_eq!(stored, store, "nothing changed");

    // Languages no market names change freely, and so does a swap that keeps both.
    assert!(update(None, Some(vec![listed("de", true)])).is_ok());
    assert!(update(Some("de"), Some(vec![listed("en", true)])).is_ok());
}

#[test]
fn a_market_already_out_of_step_doesnt_stop_other_changes() {
    let repo = InMemoryStoreRepo::default();
    let store = store_in_three_languages(&repo);
    // `hr` was never offered and `fr` isn't published: neither is this change's doing.
    let input = UpdateStore {
        default_language: Some("en".to_string()),
        languages: Some(vec![
            listed("de", true),
            listed("fr", false),
            listed("it", true),
        ]),
        market_languages: vec![offering(1, &["hr", "fr", "de"])],
        ..UpdateStore::default()
    };
    let changed = block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        input,
    ))
    .unwrap();
    assert_eq!(changed.languages().len(), 3);
}

#[test]
fn update_store_records_what_changed() {
    use hypha_core::{HasOutbox, OutboxRepo};
    use hypha_testkit::InMemoryPorts;

    let ports = InMemoryPorts::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &ports, &ids, "Acme")).unwrap();
    let update = |input: UpdateStore| {
        block_on(update_store(
            &Ctx::system(),
            &ports,
            &clock(),
            store.id(),
            input,
        ))
    };
    update(UpdateStore {
        name: Some("Acme".to_string()),
        default_language: Some("en".to_string()),
        ..UpdateStore::default()
    })
    .unwrap();
    update(UpdateStore {
        description: Some(Some("Mugs.".to_string())),
        languages: Some(vec![listed("de", true)]),
        ..UpdateStore::default()
    })
    .unwrap();
    // Refused: nothing recorded.
    update(UpdateStore {
        name: Some("Acme EU".to_string()),
        languages: Some(vec![listed("en", true)]),
        ..UpdateStore::default()
    })
    .unwrap_err();

    let events = block_on(ports.outbox().events_after(0, 100)).unwrap();
    let fields: Vec<_> = events
        .iter()
        .map(|e| (e.event_type.as_str(), e.data["fields"].clone()))
        .collect();
    assert_eq!(
        fields,
        [(
            "store.updated",
            serde_json::json!(["description", "languages"])
        )]
    );
}

#[test]
fn what_markets_offer_alone_changes_nothing() {
    let repo = InMemoryStoreRepo::default();
    let store = store_in_three_languages(&repo);
    let input = UpdateStore {
        market_languages: vec![offering(1, &["hr"])],
        ..UpdateStore::default()
    };
    let same = block_on(update_store(
        &Ctx::system(),
        &repo,
        &clock(),
        store.id(),
        input,
    ))
    .unwrap();
    assert_eq!(same.version(), store.version());
}
