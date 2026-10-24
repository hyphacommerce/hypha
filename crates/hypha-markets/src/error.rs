use hypha_core::{CountryCode, Currency, ErrorCode, MoneyError, TextError};

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

/// What's wrong with one entry of a list of fixed prices.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum FixedPriceProblem {
    #[error("the variant is listed more than once")]
    Duplicate,
    #[error("the price is in {found}, but the price list is in {expected}")]
    CurrencyMismatch { expected: Currency, found: Currency },
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error("the compare-at price is in another currency than the price")]
    CompareAtCurrencyMismatch,
    #[error("the compare-at price must be above the price")]
    CompareAtNotAbovePrice,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PriceListError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error("a price list's window must end after it starts")]
    EndsNotAfterStart,
    /// Every entry's problem, by its position in the request.
    #[error("{} fixed price(s) are invalid", .0.len())]
    InvalidFixedPrices(Vec<crate::price_list::EntryProblem>),
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

impl ErrorCode for FixedPriceProblem {
    fn code(&self) -> &'static str {
        match self {
            FixedPriceProblem::Duplicate => "duplicate_fixed_price",
            FixedPriceProblem::CurrencyMismatch { .. } => "fixed_price_currency_mismatch",
            FixedPriceProblem::Money(e) => e.code(),
            FixedPriceProblem::CompareAtCurrencyMismatch => "compare_at_currency_mismatch",
            FixedPriceProblem::CompareAtNotAbovePrice => "compare_at_not_above_price",
        }
    }
}

impl ErrorCode for PriceListError {
    fn code(&self) -> &'static str {
        match self {
            PriceListError::Text(e) => e.code(),
            PriceListError::EndsNotAfterStart => "ends_not_after_start",
            PriceListError::InvalidFixedPrices(_) => "fixed_prices_invalid",
        }
    }
}
