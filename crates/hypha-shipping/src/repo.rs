use hypha_core::{CountryCode, RepoError, ShippingZoneId};

use crate::{AppProvider, ProviderKey, ShippingZone};

/// Uniqueness rules the shipping repositories enforce, reported as
/// `RepoError::Duplicate(key)`.
pub mod keys {
    pub const ZONE_ID: &str = "shipping_zone_id";
    /// A country is in at most one zone.
    pub const ZONE_COUNTRY: &str = "shipping_zone_country";
    pub const PROVIDER_KEY: &str = "shipping_provider_key";
    /// A provider's key names an existing app (`hypha-apps`), reported as
    /// `RepoError::Reference`.
    pub const PROVIDER_APP: &str = "shipping_provider_app";
}

/// Option IDs are generated, so they are assumed unique across zones and have no key
/// (like option IDs on products).
#[allow(async_fn_in_trait)]
pub trait ShippingZoneRepo {
    async fn get(&self, id: ShippingZoneId) -> Result<Option<ShippingZone>, RepoError>;
    /// The zone that lists `country`, if any.
    async fn get_by_country(&self, country: CountryCode)
    -> Result<Option<ShippingZone>, RepoError>;
    /// Every zone, by name then ID: a store has a handful.
    async fn list(&self) -> Result<Vec<ShippingZone>, RepoError>;
    /// Stores a new zone at version 1. Fails with `Duplicate` on any key in `keys`.
    async fn insert(&self, zone: &mut ShippingZone) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`. Enforces
    /// `ZONE_COUNTRY`.
    async fn update(&self, zone: &mut ShippingZone) -> Result<(), RepoError>;
    /// Removes the zone and its options, writing the events it recorded
    /// (`ShippingZone::delete`); one that doesn't exist any more is fine.
    async fn delete(&self, zone: &mut ShippingZone) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait AppProviderRepo {
    async fn get(&self, key: &ProviderKey) -> Result<Option<AppProvider>, RepoError>;
    /// Every app provider, by key.
    async fn list(&self) -> Result<Vec<AppProvider>, RepoError>;
    /// Stores a new provider at version 1. Fails with `Duplicate(PROVIDER_KEY)`.
    async fn insert(&self, provider: &mut AppProvider) -> Result<(), RepoError>;
    async fn update(&self, provider: &mut AppProvider) -> Result<(), RepoError>;
    /// Removes it; one that doesn't exist is fine.
    async fn delete(&self, key: &ProviderKey) -> Result<(), RepoError>;
}

pub trait HasShippingZones {
    type ShippingZones: ShippingZoneRepo;
    fn shipping_zones(&self) -> &Self::ShippingZones;
}

impl<T: HasShippingZones + ?Sized> HasShippingZones for &T {
    type ShippingZones = T::ShippingZones;
    fn shipping_zones(&self) -> &Self::ShippingZones {
        (**self).shipping_zones()
    }
}

pub trait HasAppProviders {
    type AppProviders: AppProviderRepo;
    fn app_providers(&self) -> &Self::AppProviders;
}

impl<T: HasAppProviders + ?Sized> HasAppProviders for &T {
    type AppProviders = T::AppProviders;
    fn app_providers(&self) -> &Self::AppProviders {
        (**self).app_providers()
    }
}
