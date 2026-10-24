use hypha_core::{
    CountryCode, Currency, CustomerGroupId, ErrorCode, MoneyError, PriceListId, TextError,
};

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
    #[error("price list {0} is listed twice")]
    DuplicatePriceList(PriceListId),
    #[error("a catalog holds at most {max} price lists")]
    TooManyPriceLists { max: usize },
    #[error("price list {0} isn't on this catalog")]
    PriceListNotInCatalog(PriceListId),
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
    #[error("a fixed price has at most {max} quantity prices")]
    TooManyBreaks { max: usize },
    #[error("quantity prices start from 2, each for more than the one before")]
    BreakQuantityInvalid,
    #[error("a quantity price is in another currency than the price")]
    BreakCurrencyMismatch,
    #[error("each quantity price must be below the one before it, the first below the price")]
    BreaksNotDescending,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PriceListError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error("a price list's window must end after it starts")]
    EndsNotAfterStart,
    #[error("an adjustment applies to at most {max} products or categories")]
    TooManyTargets { max: usize },
    #[error("{0} is listed twice in the adjustment")]
    DuplicateTarget(String),
    /// Every entry's problem, by its position in the request.
    #[error("{} fixed price(s) are invalid", .0.len())]
    InvalidFixedPrices(Vec<crate::price_list::EntryProblem>),
    #[error("an audience names at most {max} customer groups")]
    TooManyAudienceGroups { max: usize },
    #[error("customer group {0} is listed twice in the audience")]
    DuplicateAudienceGroup(CustomerGroupId),
}

impl ErrorCode for MarketError {
    fn code(&self) -> &'static str {
        match self {
            MarketError::Text(e) => e.code(),
            MarketError::NoCountries => "no_countries",
            MarketError::DuplicateCountry(_) => "duplicate_country",
            MarketError::CountryNotInMarket(_) => "country_not_in_market",
            MarketError::LastCountry => "last_country",
            MarketError::DuplicatePriceList(_) => "duplicate_price_list",
            MarketError::TooManyPriceLists { .. } => "too_many_price_lists",
            MarketError::PriceListNotInCatalog(_) => "price_list_not_in_catalog",
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
            FixedPriceProblem::TooManyBreaks { .. } => "too_many_price_breaks",
            FixedPriceProblem::BreakQuantityInvalid => "price_break_quantity_invalid",
            FixedPriceProblem::BreakCurrencyMismatch => "price_break_currency_mismatch",
            FixedPriceProblem::BreaksNotDescending => "price_breaks_not_descending",
        }
    }
}

impl ErrorCode for PriceListError {
    fn code(&self) -> &'static str {
        match self {
            PriceListError::Text(e) => e.code(),
            PriceListError::EndsNotAfterStart => "ends_not_after_start",
            PriceListError::TooManyTargets { .. } => "too_many_adjustment_targets",
            PriceListError::DuplicateTarget(_) => "duplicate_adjustment_target",
            PriceListError::InvalidFixedPrices(_) => "fixed_prices_invalid",
            PriceListError::TooManyAudienceGroups { .. } => "too_many_audience_groups",
            PriceListError::DuplicateAudienceGroup(_) => "duplicate_audience_group",
        }
    }
}
