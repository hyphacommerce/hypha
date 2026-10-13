use hypha_core::{CountryCode, Currency, ErrorCode, TextError, VariantId};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum MarketError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error("a market needs at least one country")]
    NoCountries,
    #[error("country {0} is listed twice")]
    DuplicateCountry(CountryCode),
    #[error("country {0} is not in this market")]
    CountryNotInMarket(CountryCode),
    #[error("can't remove the last country of a market")]
    LastCountry,
}

/// One fixed price whose currency doesn't match its price list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MismatchedPrice {
    pub variant: VariantId,
    pub found: Currency,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PriceListError {
    #[error("{} fixed price(s) are not in the price list's currency {expected}", prices.len())]
    CurrencyMismatch {
        expected: Currency,
        /// Sorted by variant ID.
        prices: Vec<MismatchedPrice>,
    },
    #[error("variant {0} has more than one fixed price")]
    DuplicateFixedPrice(VariantId),
}

impl ErrorCode for MarketError {
    fn code(&self) -> &'static str {
        match self {
            MarketError::Text(e) => e.code(),
            MarketError::NoCountries => "no_countries",
            MarketError::DuplicateCountry(_) => "duplicate_country",
            MarketError::CountryNotInMarket(_) => "country_not_in_market",
            MarketError::LastCountry => "last_country",
        }
    }
}

impl ErrorCode for PriceListError {
    fn code(&self) -> &'static str {
        match self {
            PriceListError::CurrencyMismatch { .. } => "fixed_price_currency_mismatch",
            PriceListError::DuplicateFixedPrice(_) => "duplicate_fixed_price",
        }
    }
}
