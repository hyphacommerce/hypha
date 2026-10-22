//! Shipping zones (with their options and rate tables) and app providers: every field round
//! trips, a country is in one zone, versions hold.

use hypha_apps::{App, AppKey, AppRepo, HasApps, NewApp};
use hypha_core::{
    CountryCode, Currency, Money, Price, RepoError, Sealed, ShippingOptionId, ShippingZoneId,
    TaxClassId, Version, Versioned,
};
use hypha_shipping::{
    AppProvider, AppProviderRepo, BuiltinRate, DeliveryEstimate, HasAppProviders, NewAppProvider,
    NewOption, OptionPatch, OptionRate, ProviderKey, RateBasis, RateTable, ServiceInfo,
    ShippingZone, ShippingZoneRepo, Tier, keys,
};
use uuid::Uuid;

use super::scenario;

/// Runs every `ShippingZoneRepo` scenario, each on a fresh repo from `make`.
pub async fn shipping_zone_repo<R: ShippingZoneRepo>(make: impl AsyncFn() -> R) {
    scenario("shipping zone: insert stores every field, options and rate tables");
    zone_round_trips(&make().await).await;
    scenario("shipping zone: update replaces options, tables and countries");
    zone_updates(&make().await).await;
    scenario("shipping zone: a country is in one zone, on insert and update");
    zone_country_key(&make().await).await;
    scenario("shipping zone: get_by_country, list by name, delete");
    zone_lookups(&make().await).await;
    scenario("shipping zone: stale and missing updates fail");
    zone_stale(&make().await).await;
}

/// Runs every `AppProviderRepo` scenario, each on a fresh store from `make`. A provider is
/// an app's rate settings, so the store has apps too.
pub async fn app_provider_repo<S: HasAppProviders + HasApps>(make: impl AsyncFn() -> S) {
    scenario("app provider: insert stores every field; the key is unique");
    provider_round_trips(&make().await).await;
    scenario("app provider: update, stale update, list by key, delete");
    provider_updates(&make().await).await;
    scenario("app provider: needs its app, and goes when the app does");
    provider_needs_its_app(&make().await).await;
}

/// The app a provider's key names.
async fn add_app<S: HasApps>(store: &S, k: &str) {
    let mut app = App::new(NewApp {
        key: AppKey::new(k).unwrap(),
        id: super::apps::id_of(k),
        name: "Acme Carrier".to_string(),
        secret: Sealed::from_bytes(vec![1, 2, 3, 255]),
        webhook: None,
        subscriptions: vec![],
    })
    .unwrap();
    store.apps().insert(&mut app).await.unwrap();
}

fn zid(n: u128) -> ShippingZoneId {
    ShippingZoneId::from_uuid(Uuid::from_u128(n))
}
fn oid(n: u128) -> ShippingOptionId {
    ShippingOptionId::from_uuid(Uuid::from_u128(n))
}
fn country(code: &str) -> CountryCode {
    CountryCode::new(code).unwrap()
}
fn price(amount: i64, currency: Currency) -> Price {
    Price::new(Money::new(amount, currency).unwrap()).unwrap()
}

/// Weight tiers in EUR and USD, the largest bound and amount there are.
fn weight_tiers() -> OptionRate {
    let eur = RateTable::new(
        Currency::EUR,
        vec![
            Tier {
                up_to: Some(1),
                price: price(0, Currency::EUR),
            },
            Tier {
                up_to: Some(u64::MAX),
                price: price(500, Currency::EUR),
            },
            Tier {
                up_to: None,
                price: price(Money::MAX_AMOUNT, Currency::EUR),
            },
        ],
    )
    .unwrap();
    let usd = RateTable::new(
        Currency::USD,
        vec![Tier {
            up_to: None,
            price: price(700, Currency::USD),
        }],
    )
    .unwrap();
    OptionRate::Builtin(BuiltinRate::new(RateBasis::Weight, vec![usd, eur]).unwrap())
}

/// Every optional field set.
fn full_option(n: u128) -> NewOption {
    NewOption {
        id: oid(n),
        name: "Express".to_string(),
        code: format!("express-{n}"),
        rate: weight_tiers(),
        estimate: Some(DeliveryEstimate::new(1, u16::MAX).unwrap()),
        tax_class: Some(TaxClassId::from_uuid(Uuid::from_u128(77))),
        is_backup: true,
        enabled: false,
    }
}

fn provider_option(n: u128) -> NewOption {
    NewOption {
        id: oid(n),
        name: "DHL".to_string(),
        code: format!("dhl-{n}"),
        rate: OptionRate::Provider {
            provider: ProviderKey::new("spore_dhl").unwrap(),
            service: "EXPRESS_WORLDWIDE".to_string(),
        },
        estimate: None,
        tax_class: None,
        is_backup: false,
        enabled: true,
    }
}

fn zone(n: u128, name: &str, countries: &[&str]) -> ShippingZone {
    ShippingZone::new(zid(n), name, countries.iter().map(|c| country(c)).collect()).unwrap()
}

async fn zone_round_trips<R: ShippingZoneRepo>(repo: &R) {
    assert_eq!(repo.get(zid(1)).await, Ok(None));
    let mut z = zone(1, "EU", &["SI", "HR", "AT"]);
    z.add_option(full_option(11)).unwrap();
    z.add_option(provider_option(12)).unwrap();
    let flat = BuiltinRate::flat(vec![price(490, Currency::EUR)]).unwrap();
    z.add_option(NewOption {
        rate: OptionRate::Builtin(flat),
        ..provider_option(13)
    })
    .unwrap();
    repo.insert(&mut z).await.unwrap();
    assert_eq!(z.version(), Version::new(1));
    assert_eq!(repo.get(zid(1)).await, Ok(Some(z.clone())));

    // A zone with no countries and no options.
    let mut empty = zone(2, "Nowhere", &[]);
    repo.insert(&mut empty).await.unwrap();
    assert_eq!(repo.get(zid(2)).await, Ok(Some(empty)));
}

async fn zone_updates<R: ShippingZoneRepo>(repo: &R) {
    let mut z = zone(1, "EU", &["HR"]);
    z.add_option(full_option(11)).unwrap();
    z.add_option(provider_option(12)).unwrap();
    repo.insert(&mut z).await.unwrap();

    z.rename("Europe").unwrap();
    z.set_countries(vec![country("SI"), country("HR")]).unwrap();
    z.remove_option(oid(11)).unwrap();
    z.update_option(
        oid(12),
        OptionPatch {
            rate: Some(weight_tiers()),
            estimate: Some(Some(DeliveryEstimate::new(2, 3).unwrap())),
            ..OptionPatch::default()
        },
    )
    .unwrap();
    z.add_option(full_option(13)).unwrap();
    z.reorder_options(&[oid(13), oid(12)]).unwrap();
    repo.update(&mut z).await.unwrap();
    assert_eq!(z.version(), Version::new(2));
    assert_eq!(repo.get(zid(1)).await, Ok(Some(z)));
}

async fn zone_country_key<R: ShippingZoneRepo>(repo: &R) {
    repo.insert(&mut zone(1, "EU", &["HR", "SI"]))
        .await
        .unwrap();
    assert_eq!(
        repo.insert(&mut zone(2, "Balkans", &["RS", "HR"])).await,
        Err(RepoError::Duplicate(keys::ZONE_COUNTRY))
    );
    let mut other = zone(2, "Balkans", &["RS"]);
    repo.insert(&mut other).await.unwrap();
    other
        .set_countries(vec![country("RS"), country("SI")])
        .unwrap();
    assert_eq!(
        repo.update(&mut other).await,
        Err(RepoError::Duplicate(keys::ZONE_COUNTRY))
    );
    // A zone moving its own countries around is fine.
    let mut eu = repo.get(zid(1)).await.unwrap().unwrap();
    eu.set_countries(vec![country("SI"), country("HR")])
        .unwrap();
    repo.update(&mut eu).await.unwrap();
    assert_eq!(
        repo.insert(&mut zone(1, "Again", &["DE"])).await,
        Err(RepoError::Duplicate(keys::ZONE_ID))
    );
}

async fn zone_lookups<R: ShippingZoneRepo>(repo: &R) {
    let mut eu = zone(1, "EU", &["HR", "SI"]);
    let mut domestic = zone(2, "Domestic", &["RS"]);
    let mut also_eu = zone(3, "EU", &["AT"]);
    for z in [&mut eu, &mut domestic, &mut also_eu] {
        repo.insert(z).await.unwrap();
    }
    assert_eq!(
        repo.get_by_country(country("SI")).await,
        Ok(Some(eu.clone()))
    );
    assert_eq!(repo.get_by_country(country("DE")).await, Ok(None));
    // By name, then ID.
    assert_eq!(
        repo.list().await,
        Ok(vec![domestic.clone(), eu.clone(), also_eu.clone()])
    );
    let mut gone = eu.clone();
    gone.delete();
    repo.delete(&mut gone).await.unwrap();
    assert_eq!(repo.get(zid(1)).await, Ok(None));
    assert_eq!(repo.get_by_country(country("HR")).await, Ok(None));
    assert_eq!(repo.delete(&mut eu.clone()).await, Ok(()));
    // Its countries are free again.
    repo.insert(&mut zone(4, "New EU", &["HR"])).await.unwrap();
}

async fn zone_stale<R: ShippingZoneRepo>(repo: &R) {
    let mut z = zone(1, "EU", &["HR"]);
    repo.insert(&mut z).await.unwrap();
    let mut stale = z.clone();
    z.rename("Europe").unwrap();
    repo.update(&mut z).await.unwrap();
    stale.rename("Stale").unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    let mut missing = zone(9, "Missing", &[]);
    missing.set_version(Version::new(1));
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

fn key(k: &str) -> ProviderKey {
    ProviderKey::new(k).unwrap()
}

fn provider(k: &str, services: Vec<ServiceInfo>) -> AppProvider {
    AppProvider::new(NewAppProvider {
        key: key(k),
        callback_url: "https://rates.acme.example/hypha".to_string(),
        timeout_ms: AppProvider::MAX_TIMEOUT_MS,
        services,
    })
    .unwrap()
}

fn service(code: &str) -> ServiceInfo {
    ServiceInfo {
        code: code.to_string(),
        name: format!("Service {code}"),
    }
}

async fn provider_round_trips<S: HasAppProviders + HasApps>(store: &S) {
    let repo = store.app_providers();
    add_app(store, "app_acme").await;
    assert_eq!(repo.get(&key("app_acme")).await, Ok(None));
    let mut p = provider("app_acme", vec![service("b"), service("a")]);
    repo.insert(&mut p).await.unwrap();
    assert_eq!(p.version(), Version::new(1));
    assert_eq!(repo.get(&key("app_acme")).await, Ok(Some(p)));
    assert_eq!(
        repo.insert(&mut provider("app_acme", vec![])).await,
        Err(RepoError::Duplicate(keys::PROVIDER_KEY))
    );
}

async fn provider_updates<S: HasAppProviders + HasApps>(store: &S) {
    let repo = store.app_providers();
    add_app(store, "app_zeta").await;
    add_app(store, "app_alpha").await;
    let mut p = provider("app_zeta", vec![service("a")]);
    repo.insert(&mut p).await.unwrap();
    let mut alpha = provider("app_alpha", vec![]);
    repo.insert(&mut alpha).await.unwrap();

    let stale = p.clone();
    p.set_callback_url("http://localhost:4000/rates").unwrap();
    p.set_services(vec![]).unwrap();
    p.set_timeout_ms(AppProvider::MIN_TIMEOUT_MS).unwrap();
    repo.update(&mut p).await.unwrap();
    assert_eq!(p.version(), Version::new(2));
    assert_eq!(repo.get(&key("app_zeta")).await, Ok(Some(p.clone())));
    assert_eq!(
        repo.update(&mut stale.clone()).await,
        Err(RepoError::StaleVersion)
    );

    assert_eq!(repo.list().await, Ok(vec![alpha, p]));
    repo.delete(&key("app_zeta")).await.unwrap();
    assert_eq!(repo.get(&key("app_zeta")).await, Ok(None));
    assert_eq!(repo.delete(&key("app_zeta")).await, Ok(()));
}

async fn provider_needs_its_app<S: HasAppProviders + HasApps>(store: &S) {
    let repo = store.app_providers();
    assert_eq!(
        repo.insert(&mut provider("app_ghost", vec![])).await,
        Err(RepoError::Reference(keys::PROVIDER_APP))
    );
    add_app(store, "app_acme").await;
    repo.insert(&mut provider("app_acme", vec![service("a")]))
        .await
        .unwrap();
    store
        .apps()
        .delete(&AppKey::new("app_acme").unwrap())
        .await
        .unwrap();
    assert_eq!(repo.get(&key("app_acme")).await, Ok(None));
}
