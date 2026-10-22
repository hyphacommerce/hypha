//! Shipping: where a store ships (zones), what it offers there (options and their prices),
//! and the port providers calculate rates behind.
//!
//! A leaf domain: it depends on core only. It never reads products or carts; the cart,
//! a composing crate, hands it plain values (`RateRequest`).

pub mod app_provider;
pub mod commands;
pub mod error;
pub mod events;
pub mod offers;
pub mod provider;
pub mod rate;
pub mod repo;
pub mod zone;

pub use app_provider::{AppProvider, AppProviderSnapshot, NewAppProvider};
pub use commands::{
    AddOption, CommandError, CreateAppProvider, CreateZone, UpdateAppProvider, UpdateZone,
    add_option, create_app_provider, create_zone, delete_app_provider, delete_zone,
    get_app_provider, get_zone, list_app_providers, list_zones, provider_services, remove_option,
    reorder_options, update_app_provider, update_option, update_zone,
};
pub use error::ShippingError;
pub use events::{ShippingZoneChanged, ShippingZoneDeleted};
pub use hypha_core::CallbackUrl;
pub use offers::{NotOffered, Offer, OfferOutcome, QuoteSource, offers};
pub use provider::{
    DeliveryEstimate, ProviderError, ProviderKey, ProviderKind, Rate, RateItem, RateProvider,
    RateRequest, ServiceInfo, ShippingRates,
};
pub use rate::{BuiltinRate, RateBasis, RateTable, Tier, Unpriced};
pub use repo::{AppProviderRepo, HasAppProviders, HasShippingZones, ShippingZoneRepo, keys};
pub use zone::{
    NewOption, OptionCode, OptionPatch, OptionRate, ShippingOption, ShippingZone,
    ShippingZoneSnapshot,
};
