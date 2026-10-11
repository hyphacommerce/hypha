use hypha_core::CountryCode;

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
