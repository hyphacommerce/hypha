//! Spores' settings (design note 0030, §8): one record per spore, every value round
//! trips, the key is unique, versions hold.

use hypha_apps::spore_settings::keys;
use hypha_apps::{AppKey, SporeSettings, SporeSettingsRepo};
use hypha_core::{RepoError, Stamped, Version, Versioned};
use serde_json::{Map, Value, json};

use super::scenario;

fn key(name: &str) -> AppKey {
    AppKey::of_spore(name).unwrap()
}

fn values(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(map) => map,
        _ => unreachable!("an object"),
    }
}

/// Runs every `SporeSettingsRepo` scenario, each on a fresh repo from `make`.
pub async fn spore_settings_repo<R: SporeSettingsRepo>(make: impl AsyncFn() -> R) {
    scenario("spore settings: insert stores every value at version 1; one record per spore");
    round_trips(&make().await).await;
    scenario("spore settings: update stores changes, a stale one fails; spores apart");
    updates(&make().await).await;
}

async fn round_trips<R: SporeSettingsRepo>(repo: &R) {
    assert_eq!(repo.get(&key("stripe")).await, Ok(None));
    let stored_values = values(json!({
        "mode": "test",
        "retries": 3,
        "capture": true,
        "rate": 0.5,
        "secret_key": { "sealed": "00ff" }
    }));
    let mut settings =
        SporeSettings::restore(key("stripe"), stored_values.clone(), Version::NEW, None);
    repo.insert(&mut settings).await.unwrap();
    assert_eq!(settings.version(), Version::new(1));
    assert!(settings.stamps().is_some());
    let stored = repo.get(&key("stripe")).await.unwrap().unwrap();
    assert_eq!(stored, settings);
    assert_eq!(stored.values(), &stored_values);

    let mut again = SporeSettings::restore(key("stripe"), Map::new(), Version::NEW, None);
    assert_eq!(
        repo.insert(&mut again).await,
        Err(RepoError::Duplicate(keys::SPORE_SETTINGS))
    );
}

async fn updates<R: SporeSettingsRepo>(repo: &R) {
    let mut settings = SporeSettings::restore(
        key("stripe"),
        values(json!({ "mode": "test" })),
        Version::NEW,
        None,
    );
    repo.insert(&mut settings).await.unwrap();
    let mut other = SporeSettings::restore(key("reviews"), values(json!({})), Version::NEW, None);
    repo.insert(&mut other).await.unwrap();

    let stale = settings.clone();
    let mut changed = SporeSettings::restore(
        key("stripe"),
        values(json!({ "mode": "live" })),
        settings.version(),
        settings.stamps(),
    );
    repo.update(&mut changed).await.unwrap();
    assert_eq!(changed.version(), Version::new(2));
    let stored = repo.get(&key("stripe")).await.unwrap().unwrap();
    assert_eq!(stored.values(), &values(json!({ "mode": "live" })));
    assert_eq!(stored.version(), Version::new(2));

    let mut stale = stale;
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(
        repo.get(&key("reviews")).await.unwrap().unwrap().values(),
        &Map::new()
    );
    let mut never = SporeSettings::restore(key("loyalty"), Map::new(), Version::new(1), None);
    assert_eq!(repo.update(&mut never).await, Err(RepoError::StaleVersion));
}
