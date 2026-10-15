//! Rate providers (design note 0015, SH5): who calculates a price Hypha's built-in rates
//! don't. External apps answer a signed HTTP callback; spores are compiled in. Both sit
//! behind `RateProvider`, which the host implements once and dispatches by key.

use std::fmt;

use hypha_core::{Address, Currency, Money, Price, Quantity, VariantId, Weight};
use thiserror::Error;

use crate::ShippingError;

/// Which provider: `app_<name>` (an external app, by HTTP callback) or `spore_<name>`
/// (a compiled-in spore). The name is lowercase ASCII letters, digits and `_`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderKey(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    App,
    Spore,
}

impl ProviderKey {
    pub const MAX_LEN: usize = 64;

    pub fn new(key: &str) -> Result<Self, ShippingError> {
        let name = key
            .strip_prefix("app_")
            .or_else(|| key.strip_prefix("spore_"))
            .ok_or_else(|| ShippingError::InvalidProviderKey(key.to_string()))?;
        let mut chars = name.chars();
        let valid = key.len() <= Self::MAX_LEN
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !valid {
            return Err(ShippingError::InvalidProviderKey(key.to_string()));
        }
        Ok(Self(key.to_string()))
    }

    pub fn kind(&self) -> ProviderKind {
        if self.0.starts_with("app_") {
            ProviderKind::App
        } else {
            ProviderKind::Spore
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Delivery time in days, `min` to `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryEstimate {
    min_days: u16,
    max_days: u16,
}

impl DeliveryEstimate {
    pub fn new(min_days: u16, max_days: u16) -> Result<Self, ShippingError> {
        if min_days > max_days {
            return Err(ShippingError::InvalidEstimate { min_days, max_days });
        }
        Ok(Self { min_days, max_days })
    }

    pub fn min_days(self) -> u16 {
        self.min_days
    }

    pub fn max_days(self) -> u16 {
        self.max_days
    }
}

/// One line of a group, as a provider sees it: plain values, no product (SH11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateItem {
    pub variant: VariantId,
    pub sku: Option<String>,
    pub quantity: Quantity,
    pub unit_price: Price,
    pub weight: Option<Weight>,
}

/// What a provider quotes for: one group of a cart (section 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateRequest {
    pub currency: Currency,
    /// The address of the location that would ship the group, when it has one.
    pub origin: Option<Address>,
    pub destination: Address,
    pub items: Vec<RateItem>,
    /// The group's lines, as priced.
    pub subtotal: Money,
}

/// A service a provider can quote: what an option offers by `service` (Q2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceInfo {
    pub code: String,
    pub name: String,
}

/// One rate a provider returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rate {
    pub service: String,
    pub name: String,
    /// Never negative: a provider's answer that is goes no further than the host's
    /// parsing, which reports it as a failure.
    pub amount: Price,
    pub estimate: Option<DeliveryEstimate>,
    /// The provider's quote ID, kept to buy the label later (design note 0017).
    pub reference: Option<String>,
}

/// Why a provider gave no rates. Never fails a cart: its options are left out (SH7).
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ProviderError {
    /// No provider with this key is registered (a removed app, a spore not in this build).
    #[error("no shipping provider {0}")]
    Unknown(ProviderKey),
    #[error("shipping provider {0} is disabled")]
    Disabled(ProviderKey),
    #[error("shipping provider {0} didn't answer in time")]
    Timeout(ProviderKey),
    /// The provider answered, but not with rates: an HTTP error, a malformed body.
    #[error("shipping provider {provider} failed: {reason}")]
    Failed {
        provider: ProviderKey,
        reason: String,
    },
    /// Rates in another currency than the request's.
    #[error("shipping provider {provider} quoted in {got}, not {expected}")]
    WrongCurrency {
        provider: ProviderKey,
        expected: Currency,
        got: Currency,
    },
}

impl hypha_core::ErrorCode for ProviderError {
    fn code(&self) -> &'static str {
        match self {
            ProviderError::Unknown(_) => "shipping_provider_unknown",
            ProviderError::Disabled(_) => "shipping_provider_disabled",
            ProviderError::Timeout(_) => "shipping_provider_timeout",
            ProviderError::Failed { .. } => "shipping_provider_failed",
            ProviderError::WrongCurrency { .. } => "shipping_provider_wrong_currency",
        }
    }
}

/// Calculates rates for options priced by a provider. The host implements it once,
/// dispatching by key to the external apps and spores it was built with: static
/// dispatch, no `dyn`, so futures stay provably `Send` (CLAUDE.md). Each call enforces
/// its provider's timeout.
#[allow(async_fn_in_trait)]
pub trait RateProvider {
    async fn services(&self, provider: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError>;
    async fn quote(
        &self,
        provider: &ProviderKey,
        request: &RateRequest,
    ) -> Result<Vec<Rate>, ProviderError>;
}

/// What a spore implements: the same two calls for itself, without a key. The host's
/// `RateProvider` routes `spore_<name>` to it.
#[allow(async_fn_in_trait)]
pub trait ShippingRates {
    async fn services(&self) -> Result<Vec<ServiceInfo>, ProviderError>;
    async fn quote(&self, request: &RateRequest) -> Result<Vec<Rate>, ProviderError>;
}
