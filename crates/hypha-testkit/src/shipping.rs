use std::{collections::BTreeMap, sync::Mutex};

use hypha_core::{CountryCode, Recorded, RepoError, ShippingZoneId, Version, Versioned};
use hypha_shipping::{
    AppProvider, AppProviderRepo, ProviderKey, ShippingZone, ShippingZoneRepo, keys,
};

use crate::{InMemoryOutbox, table::Table};

#[derive(Default)]
pub struct InMemoryShippingZoneRepo {
    zones: Table<ShippingZoneId, ShippingZone>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryShippingZoneRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The events this repo wrote (into its outbox, shared or not).
    pub fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }
}

/// The unique keys `new` would break against `other`: a shared country.
fn unique(new: &ShippingZone, other: &ShippingZone) -> Option<&'static str> {
    new.countries()
        .iter()
        .any(|c| other.covers(*c))
        .then_some(keys::ZONE_COUNTRY)
}

impl ShippingZoneRepo for InMemoryShippingZoneRepo {
    async fn get(&self, id: ShippingZoneId) -> Result<Option<ShippingZone>, RepoError> {
        Ok(self.zones.get(&id))
    }

    async fn get_by_country(
        &self,
        country: CountryCode,
    ) -> Result<Option<ShippingZone>, RepoError> {
        Ok(self.zones.find(|z| z.covers(country)))
    }

    async fn list(&self) -> Result<Vec<ShippingZone>, RepoError> {
        let mut zones = self.zones.all();
        zones.sort_by(|a, b| (a.name(), a.id()).cmp(&(b.name(), b.id())));
        Ok(zones)
    }

    async fn insert(&self, zone: &mut ShippingZone) -> Result<(), RepoError> {
        let events = zone.take_events();
        self.zones.insert(zone.id(), zone, keys::ZONE_ID, &unique)?;
        self.outbox.write(events, Some(zone.version()));
        Ok(())
    }

    async fn update(&self, zone: &mut ShippingZone) -> Result<(), RepoError> {
        let events = zone.take_events();
        self.zones.update(zone.id(), zone, &unique)?;
        self.outbox.write(events, Some(zone.version()));
        Ok(())
    }

    async fn delete(&self, zone: &mut ShippingZone) -> Result<(), RepoError> {
        let events = zone.take_events();
        self.zones.remove(&zone.id());
        self.outbox.write(events, None);
        Ok(())
    }
}

/// App providers by key. Keys are strings, which `Table` (`Copy` keys) doesn't take, so
/// the version rules are spelled out here.
#[derive(Default)]
pub struct InMemoryAppProviderRepo {
    providers: Mutex<BTreeMap<ProviderKey, AppProvider>>,
}

impl AppProviderRepo for InMemoryAppProviderRepo {
    async fn get(&self, key: &ProviderKey) -> Result<Option<AppProvider>, RepoError> {
        Ok(self.providers.lock().unwrap().get(key).cloned())
    }

    async fn list(&self) -> Result<Vec<AppProvider>, RepoError> {
        Ok(self.providers.lock().unwrap().values().cloned().collect())
    }

    async fn insert(&self, provider: &mut AppProvider) -> Result<(), RepoError> {
        let mut providers = self.providers.lock().unwrap();
        if providers.contains_key(provider.key()) {
            return Err(RepoError::Duplicate(keys::PROVIDER_KEY));
        }
        provider.set_version(Version::NEW.next());
        providers.insert(provider.key().clone(), provider.clone());
        Ok(())
    }

    async fn update(&self, provider: &mut AppProvider) -> Result<(), RepoError> {
        let mut providers = self.providers.lock().unwrap();
        match providers.get(provider.key()) {
            Some(stored) if stored.version() == provider.version() => {
                provider.set_version(provider.version().next());
                providers.insert(provider.key().clone(), provider.clone());
                Ok(())
            }
            _ => Err(RepoError::StaleVersion),
        }
    }

    async fn delete(&self, key: &ProviderKey) -> Result<(), RepoError> {
        self.providers.lock().unwrap().remove(key);
        Ok(())
    }
}

/// A `RateProvider` that knows no provider: for tests whose shipping is built-in only.
pub struct NoRateProviders;

impl hypha_shipping::RateProvider for NoRateProviders {
    async fn services(
        &self,
        provider: &ProviderKey,
    ) -> Result<Vec<hypha_shipping::ServiceInfo>, hypha_shipping::ProviderError> {
        Err(hypha_shipping::ProviderError::Unknown(provider.clone()))
    }

    async fn quote(
        &self,
        provider: &ProviderKey,
        _: &hypha_shipping::RateRequest,
    ) -> Result<Vec<hypha_shipping::Rate>, hypha_shipping::ProviderError> {
        Err(hypha_shipping::ProviderError::Unknown(provider.clone()))
    }
}
