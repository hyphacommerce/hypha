use hypha_core::{CountryCode, Currency, VariantId};

use crate::catalog::CatalogContext;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MarketError {
    #[error("a market needs at least one country")]
    NoCountries,
    #[error("country {0} is listed twice")]
    DuplicateCountry(CountryCode),
    #[error("country {0} is not in this market")]
    CountryNotInMarket(CountryCode),
    #[error("can't remove the last country of a market")]
    LastCountry,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CatalogError {
    #[error("more than one catalog for {0:?}")]
    DuplicateContext(CatalogContext),
}

/// One fixed price whose currency doesn't match its price list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MismatchedPrice {
    pub variant: VariantId,
    pub found: Currency,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PriceListError {
    #[error("{} fixed price(s) are not in the price list's currency {expected:?}", prices.len())]
    CurrencyMismatch {
        expected: Currency,
        /// Sorted by variant ID.
        prices: Vec<MismatchedPrice>,
    },
}
