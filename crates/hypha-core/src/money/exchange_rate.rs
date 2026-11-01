use std::fmt;

use serde::{Deserialize, Serialize};

use super::decimal::{self, div_round};
use crate::{Currency, MoneyError, UnitCost};

/// What one unit of a currency is worth in another (1 USD = 0.9213 EUR is `0.9213`),
/// with up to `PLACES` decimals. Positive, at most `MAX_SCALED` in 1/`SCALE`. In JSON a
/// decimal string (`"0.9213"`), so it stays exact.
///
/// It names neither currency: the document holding it does (a purchase order's rate is
/// from its currency to the store's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ExchangeRate(i64);

impl ExchangeRate {
    pub const PLACES: u32 = 8;
    pub const SCALE: i64 = 100_000_000;
    /// Exact in JSON and JavaScript, like `Money`.
    pub const MAX_SCALED: i64 = crate::Money::MAX_AMOUNT;

    /// `scaled` in 1/`SCALE`: 92_130_000 is 0.9213. Zero or less is `InvalidExchangeRate`.
    pub fn new(scaled: i64) -> Result<Self, MoneyError> {
        if scaled <= 0 || scaled > Self::MAX_SCALED {
            return Err(MoneyError::InvalidExchangeRate(decimal_or_raw(scaled)));
        }
        Ok(Self(scaled))
    }

    /// A decimal string with at most `PLACES` decimals, as in JSON.
    pub fn parse(text: &str) -> Result<Self, MoneyError> {
        let scaled = decimal::parse(text, Self::PLACES, Self::MAX_SCALED)
            .filter(|s| *s > 0)
            .ok_or_else(|| MoneyError::InvalidExchangeRate(text.to_string()))?;
        Ok(Self(scaled))
    }

    pub fn scaled(self) -> i64 {
        self.0
    }

    /// `cost` in `to`, rounded half away from zero at `UnitCost`'s precision. Each
    /// currency's minor units are respected: at a rate of 0.0065, ¥1,000 (1000 JPY minor
    /// units) is €6.50 (650 EUR minor units).
    pub fn convert(self, cost: UnitCost, to: Currency) -> Result<UnitCost, MoneyError> {
        let from_minor = 10_i128.pow(u32::from(cost.currency().minor_units()));
        let to_minor = 10_i128.pow(u32::from(to.minor_units()));
        let scaled = div_round(
            i128::from(cost.scaled()) * i128::from(self.0) * to_minor,
            from_minor * i128::from(Self::SCALE),
        );
        UnitCost::from_i128(scaled, to)
    }
}

/// A scaled value as the decimal it stands for, for an error message.
fn decimal_or_raw(scaled: i64) -> String {
    let sign = if scaled < 0 { "-" } else { "" };
    let magnitude = i64::try_from(scaled.unsigned_abs()).unwrap_or(i64::MAX);
    format!("{sign}{}", decimal::format(magnitude, ExchangeRate::PLACES))
}

impl TryFrom<String> for ExchangeRate {
    type Error = MoneyError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<ExchangeRate> for String {
    fn from(rate: ExchangeRate) -> String {
        rate.to_string()
    }
}

/// The decimal, trailing zeros dropped: `0.9213`.
impl fmt::Display for ExchangeRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&decimal::format(self.0, Self::PLACES))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(text: &str) -> ExchangeRate {
        ExchangeRate::parse(text).unwrap()
    }

    #[test]
    fn parses_up_to_eight_decimals() {
        assert_eq!(rate("0.9213").scaled(), 92_130_000);
        assert_eq!(rate("1").scaled(), 100_000_000);
        assert_eq!(rate("0.00000001").scaled(), 1);
        assert_eq!(
            ExchangeRate::parse("0.000000001"),
            Err(MoneyError::InvalidExchangeRate("0.000000001".into()))
        );
    }

    #[test]
    fn refuses_zero() {
        assert_eq!(
            ExchangeRate::parse("0"),
            Err(MoneyError::InvalidExchangeRate("0".into()))
        );
        assert_eq!(
            ExchangeRate::new(0),
            Err(MoneyError::InvalidExchangeRate("0".into()))
        );
        assert_eq!(
            ExchangeRate::new(-150_000_000),
            Err(MoneyError::InvalidExchangeRate("-1.5".into()))
        );
    }

    #[test]
    fn json_is_a_decimal_string() {
        assert_eq!(
            serde_json::to_string(&rate("1.17230000")).unwrap(),
            "\"1.1723\""
        );
        assert_eq!(
            serde_json::from_str::<ExchangeRate>("\"1.1723\"").unwrap(),
            rate("1.1723")
        );
        assert!(serde_json::from_str::<ExchangeRate>("1.1723").is_err());
    }

    #[test]
    fn converts_between_currencies_with_two_decimals() {
        // $10.00 at 0.9213 is €9.213
        let ten_dollars = UnitCost::parse("1000", Currency::USD).unwrap();
        assert_eq!(
            rate("0.9213").convert(ten_dollars, Currency::EUR),
            UnitCost::parse("921.3", Currency::EUR)
        );
    }

    #[test]
    fn converts_across_minor_units() {
        let jpy = Currency::new("JPY").unwrap();
        let kwd = Currency::new("KWD").unwrap();
        // ¥1,000 at 0.0065 is €6.50
        let yen = UnitCost::parse("1000", jpy).unwrap();
        assert_eq!(
            rate("0.0065").convert(yen, Currency::EUR),
            UnitCost::parse("650", Currency::EUR)
        );
        // €6.50 at 153.85 is ¥1,000.025
        let euros = UnitCost::parse("650", Currency::EUR).unwrap();
        assert_eq!(
            rate("153.85").convert(euros, jpy),
            UnitCost::parse("1000.025", jpy)
        );
        // 1 KWD (1000 fils) at 3.25 is $3.25 (325 cents)
        let dinar = UnitCost::parse("1000", kwd).unwrap();
        assert_eq!(
            rate("3.25").convert(dinar, Currency::USD),
            UnitCost::parse("325", Currency::USD)
        );
    }

    #[test]
    fn conversion_rounds_half_away_from_zero_at_four_places() {
        // 0.0001 cents at 0.5 is 0.00005 cents → 0.0001
        let tiny = UnitCost::new(1, Currency::USD).unwrap();
        assert_eq!(
            rate("0.5").convert(tiny, Currency::EUR),
            UnitCost::new(1, Currency::EUR)
        );
        // at 0.49999999 it's just under half → 0
        assert_eq!(
            rate("0.49999999").convert(tiny, Currency::EUR),
            UnitCost::new(0, Currency::EUR)
        );
    }

    #[test]
    fn conversion_overflows_instead_of_panicking() {
        let most = UnitCost::new(UnitCost::MAX_SCALED, Currency::USD).unwrap();
        assert_eq!(
            rate("2").convert(most, Currency::EUR),
            Err(MoneyError::Overflow)
        );
    }
}
