use hypha_core::{CountryCode, Currency, ErrorCode, ShippingOptionId, TextError};
use thiserror::Error;

use crate::ProviderKey;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ShippingError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error("country {0} is listed twice")]
    DuplicateCountry(CountryCode),
    #[error("a zone has at most {max} options")]
    TooManyOptions { max: usize },
    #[error("option {0} already exists")]
    DuplicateOptionId(ShippingOptionId),
    #[error("option code {0:?} must be lowercase letters, digits, '_' and '-', up to 64")]
    InvalidOptionCode(String),
    #[error("another option of this zone has the code {0:?}")]
    DuplicateOptionCode(String),
    #[error("option {0} not found")]
    OptionNotFound(ShippingOptionId),
    #[error("the new order must list every option of the zone once")]
    OrderNotAllOptions,
    #[error("a rate needs a table for at least one currency")]
    NoRateTables,
    #[error("the {0} table has no tiers")]
    EmptyRateTable(Currency),
    #[error("the last tier of the {0} table must be open (no upper bound)")]
    LastTierBounded(Currency),
    #[error("the tiers of the {0} table must have rising upper bounds above zero")]
    TiersNotAscending(Currency),
    #[error("a tier of the {table} table is priced in {tier}")]
    TierCurrencyMismatch { table: Currency, tier: Currency },
    #[error("a rate has two {0} tables")]
    DuplicateRateCurrency(Currency),
    #[error("a flat rate has one price per currency; the {0} table has tiers")]
    FlatRateWithTiers(Currency),
    #[error("a delivery estimate of {min_days} to {max_days} days runs backwards")]
    InvalidEstimate { min_days: u16, max_days: u16 },
    #[error(
        "provider key {0:?} must be app_<name> or spore_<name>, lowercase letters, digits and '_'"
    )]
    InvalidProviderKey(String),
    #[error("{0} isn't an app provider key (app_<name>)")]
    NotAnAppKey(ProviderKey),
    #[error("callback URL {0:?} must be https:// (or http:// to localhost)")]
    InvalidCallbackUrl(String),
    #[error("a callback timeout is {min} to {max} ms")]
    InvalidTimeout { min: u32, max: u32 },
    #[error("service {0:?} is listed twice")]
    DuplicateServiceCode(String),
}

impl ErrorCode for ShippingError {
    fn code(&self) -> &'static str {
        match self {
            ShippingError::Text(e) => e.code(),
            ShippingError::DuplicateCountry(_) => "duplicate_country",
            ShippingError::TooManyOptions { .. } => "too_many_shipping_options",
            ShippingError::DuplicateOptionId(_) => "duplicate_shipping_option",
            ShippingError::InvalidOptionCode(_) => "invalid_shipping_option_code",
            ShippingError::DuplicateOptionCode(_) => "shipping_option_code_taken",
            ShippingError::OptionNotFound(_) => "shipping_option_not_found",
            ShippingError::OrderNotAllOptions => "invalid_shipping_option_order",
            ShippingError::NoRateTables
            | ShippingError::EmptyRateTable(_)
            | ShippingError::LastTierBounded(_)
            | ShippingError::TiersNotAscending(_)
            | ShippingError::TierCurrencyMismatch { .. }
            | ShippingError::DuplicateRateCurrency(_)
            | ShippingError::FlatRateWithTiers(_) => "invalid_shipping_rate",
            ShippingError::InvalidEstimate { .. } => "invalid_delivery_estimate",
            ShippingError::InvalidProviderKey(_) | ShippingError::NotAnAppKey(_) => {
                "invalid_shipping_provider_key"
            }
            ShippingError::InvalidCallbackUrl(_) => "invalid_callback_url",
            ShippingError::InvalidTimeout { .. } => "invalid_callback_timeout",
            ShippingError::DuplicateServiceCode(_) => "duplicate_shipping_service",
        }
    }
}

impl From<hypha_core::CallbackUrlError> for ShippingError {
    fn from(e: hypha_core::CallbackUrlError) -> Self {
        ShippingError::InvalidCallbackUrl(e.0)
    }
}
