use hypha_core::{
    Actor, AuthError, CountryCode, Ctx, Currency, Money, Price, Scope, ShippingOptionId,
    ShippingZoneId, UserId,
};
use hypha_shipping::{
    AddOption, BuiltinRate, CommandError, CreateAppProvider, CreateZone, OptionRate, ProviderError,
    ProviderKey, RateProvider, RateRequest, SECRET_PREFIX, ServiceInfo, ShippingError,
    UpdateAppProvider, UpdateZone, add_option, create_app_provider, create_zone, delete_zone,
    get_app_provider, get_zone, list_zones, open_secret, provider_services,
    rotate_app_provider_secret, update_app_provider, update_zone,
};
use hypha_testkit::{InMemoryAppProviderRepo, InMemoryShippingZoneRepo, SequentialIds, TestEnv};
use pollster::block_on;
use uuid::Uuid;

fn countries(codes: &[&str]) -> Vec<String> {
    codes.iter().map(|c| c.to_string()).collect()
}

fn zone_input(name: &str, codes: &[&str]) -> CreateZone {
    CreateZone {
        name: name.to_string(),
        countries: countries(codes),
    }
}

fn reader() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::ShippingRead],
    )
}

fn flat() -> OptionRate {
    let price = Price::new(Money::new(490, Currency::Eur).unwrap()).unwrap();
    OptionRate::Builtin(BuiltinRate::flat(vec![price]).unwrap())
}

// ---- zones ----

#[test]
fn a_zone_is_created_and_read_back() {
    let zones = InMemoryShippingZoneRepo::default();
    let ids = SequentialIds::default();
    let zone = block_on(create_zone(
        &Ctx::system(),
        &zones,
        &ids,
        zone_input("EU", &["HR", "SI"]),
    ))
    .unwrap();
    assert_eq!(zone.id(), ShippingZoneId::from_uuid(Uuid::from_u128(1)));
    assert_eq!(
        block_on(get_zone(&reader(), &zones, zone.id())),
        Ok(zone.clone())
    );
    assert_eq!(block_on(list_zones(&reader(), &zones)), Ok(vec![zone]));
}

#[test]
fn a_country_already_in_a_zone_is_refused_naming_it() {
    let zones = InMemoryShippingZoneRepo::default();
    let ids = SequentialIds::default();
    let eu = block_on(create_zone(
        &Ctx::system(),
        &zones,
        &ids,
        zone_input("EU", &["HR"]),
    ))
    .unwrap();
    assert_eq!(
        block_on(create_zone(
            &Ctx::system(),
            &zones,
            &ids,
            zone_input("Other", &["SI", "HR"])
        )),
        Err(CommandError::CountryInOtherZone {
            country: CountryCode::new("HR").unwrap(),
            zone: eu.id(),
        })
    );
    let other = block_on(create_zone(
        &Ctx::system(),
        &zones,
        &ids,
        zone_input("Other", &["SI"]),
    ))
    .unwrap();
    let take_hr = UpdateZone {
        countries: Some(countries(&["SI", "HR"])),
        ..UpdateZone::default()
    };
    assert_eq!(
        block_on(update_zone(&Ctx::system(), &zones, other.id(), take_hr)),
        Err(CommandError::CountryInOtherZone {
            country: CountryCode::new("HR").unwrap(),
            zone: eu.id(),
        })
    );
}

#[test]
fn an_unknown_country_code_is_refused() {
    let zones = InMemoryShippingZoneRepo::default();
    let result = block_on(create_zone(
        &Ctx::system(),
        &zones,
        &SequentialIds::default(),
        zone_input("EU", &["XX1"]),
    ));
    assert!(matches!(result, Err(CommandError::Country(_))));
}

#[test]
fn options_are_added_and_a_deleted_zone_is_gone() {
    let zones = InMemoryShippingZoneRepo::default();
    let ids = SequentialIds::default();
    let zone = block_on(create_zone(
        &Ctx::system(),
        &zones,
        &ids,
        zone_input("EU", &["HR"]),
    ))
    .unwrap();
    let input = AddOption {
        name: "Standard".to_string(),
        code: "standard".to_string(),
        rate: flat(),
        estimate: None,
        tax_class: None,
        is_backup: false,
        enabled: true,
    };
    let zone = block_on(add_option(&Ctx::system(), &zones, &ids, zone.id(), input)).unwrap();
    assert_eq!(
        zone.options()[0].id(),
        ShippingOptionId::from_uuid(Uuid::from_u128(2))
    );
    assert_eq!(
        block_on(delete_zone(&Ctx::system(), &zones, zone.id())),
        Ok(())
    );
    assert_eq!(
        block_on(get_zone(&reader(), &zones, zone.id())),
        Err(CommandError::ZoneNotFound(zone.id()))
    );
}

#[test]
fn changing_zones_needs_shipping_write() {
    let zones = InMemoryShippingZoneRepo::default();
    assert_eq!(
        block_on(create_zone(
            &reader(),
            &zones,
            &SequentialIds::default(),
            zone_input("EU", &["HR"])
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ShippingWrite
        )))
    );
}

// ---- app providers ----

fn acme() -> CreateAppProvider {
    CreateAppProvider {
        key: "app_acme".to_string(),
        name: "Acme".to_string(),
        callback_url: "https://acme.example/rates".to_string(),
        timeout_ms: None,
        services: vec![],
    }
}

#[test]
fn a_provider_gets_a_secret_shown_once_and_kept_sealed() {
    let providers = InMemoryAppProviderRepo::default();
    let env = TestEnv::default();
    let (provider, secret) = block_on(create_app_provider(
        &Ctx::system(),
        &providers,
        &env,
        &env,
        acme(),
    ))
    .unwrap();
    assert!(secret.starts_with(SECRET_PREFIX));
    assert_eq!(secret.len(), SECRET_PREFIX.len() + 64);
    assert_eq!(provider.timeout_ms(), 3_000);
    assert_ne!(provider.secret().as_bytes(), secret.as_bytes());
    let stored = block_on(get_app_provider(&reader(), &providers, "app_acme")).unwrap();
    assert_eq!(open_secret(&env, &stored).as_deref(), Ok(secret.as_str()));

    let (_, rotated) = block_on(rotate_app_provider_secret(
        &Ctx::system(),
        &providers,
        &env,
        &env,
        "app_acme",
    ))
    .unwrap();
    assert_ne!(rotated, secret);
    let stored = block_on(get_app_provider(&reader(), &providers, "app_acme")).unwrap();
    assert_eq!(open_secret(&env, &stored).as_deref(), Ok(rotated.as_str()));
}

#[test]
fn a_provider_key_is_taken_once_and_must_be_an_app_key() {
    let providers = InMemoryAppProviderRepo::default();
    let env = TestEnv::default();
    block_on(create_app_provider(
        &Ctx::system(),
        &providers,
        &env,
        &env,
        acme(),
    ))
    .unwrap();
    assert_eq!(
        block_on(create_app_provider(
            &Ctx::system(),
            &providers,
            &env,
            &env,
            acme()
        ))
        .map(|_| ()),
        Err(CommandError::ProviderExists(
            ProviderKey::new("app_acme").unwrap()
        ))
    );
    let spore = CreateAppProvider {
        key: "spore_dhl".to_string(),
        ..acme()
    };
    assert_eq!(
        block_on(create_app_provider(
            &Ctx::system(),
            &providers,
            &env,
            &env,
            spore
        ))
        .map(|_| ()),
        Err(CommandError::Shipping(ShippingError::NotAnAppKey(
            ProviderKey::new("spore_dhl").unwrap()
        )))
    );
}

#[test]
fn a_callback_url_is_https_or_local_http() {
    let providers = InMemoryAppProviderRepo::default();
    let env = TestEnv::default();
    for bad in [
        "http://acme.example/rates",
        "ftp://x",
        "https://",
        "acme.example",
    ] {
        let input = CreateAppProvider {
            callback_url: bad.to_string(),
            ..acme()
        };
        assert_eq!(
            block_on(create_app_provider(
                &Ctx::system(),
                &providers,
                &env,
                &env,
                input
            ))
            .map(|_| ()),
            Err(CommandError::Shipping(ShippingError::InvalidCallbackUrl(
                bad.to_string()
            ))),
            "{bad}"
        );
    }
    let local = CreateAppProvider {
        callback_url: "http://localhost:4000/rates".to_string(),
        ..acme()
    };
    assert!(
        block_on(create_app_provider(
            &Ctx::system(),
            &providers,
            &env,
            &env,
            local
        ))
        .is_ok()
    );
}

#[test]
fn updating_a_provider_checks_its_timeout() {
    let providers = InMemoryAppProviderRepo::default();
    let env = TestEnv::default();
    block_on(create_app_provider(
        &Ctx::system(),
        &providers,
        &env,
        &env,
        acme(),
    ))
    .unwrap();
    let too_long = UpdateAppProvider {
        timeout_ms: Some(10_001),
        ..UpdateAppProvider::default()
    };
    assert_eq!(
        block_on(update_app_provider(
            &Ctx::system(),
            &providers,
            "app_acme",
            too_long
        )),
        Err(CommandError::Shipping(ShippingError::InvalidTimeout {
            min: 100,
            max: 10_000
        }))
    );
    let off = UpdateAppProvider {
        enabled: Some(false),
        ..UpdateAppProvider::default()
    };
    let updated = block_on(update_app_provider(
        &Ctx::system(),
        &providers,
        "app_acme",
        off,
    ))
    .unwrap();
    assert!(!updated.enabled());
}

/// A provider registry that knows one spore.
struct OneSpore;

impl RateProvider for OneSpore {
    async fn services(&self, key: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError> {
        match key.as_str() {
            "spore_dhl" => Ok(vec![ServiceInfo {
                code: "express".to_string(),
                name: "DHL Express".to_string(),
            }]),
            _ => Err(ProviderError::Unknown(key.clone())),
        }
    }
    async fn quote(
        &self,
        key: &ProviderKey,
        _: &RateRequest,
    ) -> Result<Vec<hypha_shipping::Rate>, ProviderError> {
        Err(ProviderError::Unknown(key.clone()))
    }
}

#[test]
fn a_providers_services_come_from_the_registry() {
    assert_eq!(
        block_on(provider_services(&Ctx::system(), &OneSpore, "spore_dhl")).map(|s| s.len()),
        Ok(1)
    );
    assert_eq!(
        block_on(provider_services(&Ctx::system(), &OneSpore, "spore_ups")),
        Err(CommandError::Provider(ProviderError::Unknown(
            ProviderKey::new("spore_ups").unwrap()
        )))
    );
}

#[test]
fn asking_for_services_needs_shipping_write() {
    // It may call the app's URL, which a reader shouldn't be able to trigger.
    assert_eq!(
        block_on(provider_services(&reader(), &OneSpore, "spore_dhl")).map(|s| s.len()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ShippingWrite
        )))
    );
}
