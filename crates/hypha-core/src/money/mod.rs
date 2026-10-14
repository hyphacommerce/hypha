use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod basis_points;
mod price;
mod tax_rate;

pub use basis_points::BasisPoints;
pub use price::Price;
pub use tax_rate::TaxRate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "UPPERCASE")]
pub enum Currency {
    Eur,
    Usd,
    Rsd,
}

impl Currency {
    /// ISO 4217 code, the same string used in JSON.
    pub fn as_str(&self) -> &'static str {
        match self {
            Currency::Eur => "EUR",
            Currency::Usd => "USD",
            Currency::Rsd => "RSD",
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A currency code Hypha doesn't support.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown currency {0:?}")]
pub struct UnknownCurrency(pub String);

impl FromStr for Currency {
    type Err = UnknownCurrency;

    /// Parses the ISO 4217 code produced by `as_str` (e.g. "EUR"); case-sensitive.
    fn from_str(code: &str) -> Result<Self, Self::Err> {
        [Currency::Eur, Currency::Usd, Currency::Rsd]
            .into_iter()
            .find(|c| c.as_str() == code)
            .ok_or_else(|| UnknownCurrency(code.to_string()))
    }
}

/// An amount in minor units (1050 = €10.50) and its currency.
///
/// Amounts are limited to ±`MAX_AMOUNT` (2⁵³ − 1, about 90 trillion in major units):
/// the largest integer JavaScript (JSON clients, Cloudflare D1) represents exactly.
/// Every way of making a `Money` enforces it, including deserialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawMoney")]
pub struct Money {
    amount: i64,
    currency: Currency,
}

/// Unvalidated JSON shape of `Money`; only used to deserialize through `Money::new`.
#[derive(Deserialize)]
struct RawMoney {
    amount: i64,
    currency: Currency,
}

impl TryFrom<RawMoney> for Money {
    type Error = MoneyError;

    fn try_from(raw: RawMoney) -> Result<Self, Self::Error> {
        Money::new(raw.amount, raw.currency)
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum MoneyError {
    #[error("currency mismatch: {0} vs {1}")]
    CurrencyMismatch(Currency, Currency),
    #[error("amount overflow")]
    Overflow,
    #[error("amount negative")]
    Negative,
    #[error("basis points must be at least -10000, got {0}")]
    InvalidBasisPoints(i32),
    #[error("tax rate must be at most 1000000 (100%), got {0}")]
    InvalidTaxRate(u32),
}

impl crate::ErrorCode for MoneyError {
    fn code(&self) -> &'static str {
        match self {
            MoneyError::CurrencyMismatch(..) => "currency_mismatch",
            MoneyError::Overflow => "amount_overflow",
            MoneyError::Negative => "negative_amount",
            MoneyError::InvalidBasisPoints(_) => "invalid_basis_points",
            MoneyError::InvalidTaxRate(_) => "invalid_tax_rate",
        }
    }
}

impl Money {
    /// 2⁵³ − 1, see the type docs.
    pub const MAX_AMOUNT: i64 = (1 << 53) - 1;

    /// Fails with `Overflow` if `amount` is outside ±`MAX_AMOUNT`.
    pub fn new(amount: i64, currency: Currency) -> Result<Self, MoneyError> {
        if amount.unsigned_abs() > Self::MAX_AMOUNT.unsigned_abs() {
            return Err(MoneyError::Overflow);
        }
        Ok(Self { amount, currency })
    }

    pub fn zero(currency: Currency) -> Self {
        Self {
            amount: 0,
            currency,
        }
    }

    pub fn amount(&self) -> i64 {
        self.amount
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    fn ensure_same_currency(&self, other: Money) -> Result<(), MoneyError> {
        if self.currency != other.currency {
            Err(MoneyError::CurrencyMismatch(self.currency, other.currency))
        } else {
            Ok(())
        }
    }

    pub fn checked_add(self, other: Money) -> Result<Money, MoneyError> {
        self.ensure_same_currency(other)?;
        let amount = self
            .amount
            .checked_add(other.amount)
            .ok_or(MoneyError::Overflow)?;
        Money::new(amount, self.currency)
    }

    pub fn checked_sub(self, other: Money) -> Result<Money, MoneyError> {
        self.ensure_same_currency(other)?;
        let amount = self
            .amount
            .checked_sub(other.amount)
            .ok_or(MoneyError::Overflow)?;
        Money::new(amount, self.currency)
    }

    pub fn checked_mul(self, quantity: u32) -> Result<Money, MoneyError> {
        let amount = self
            .amount
            .checked_mul(i64::from(quantity))
            .ok_or(MoneyError::Overflow)?;
        Money::new(amount, self.currency)
    }

    pub fn sum(items: &[Money], currency: Currency) -> Result<Money, MoneyError> {
        let mut total = Money::zero(currency);
        for item in items {
            total = total.checked_add(*item)?;
        }
        Ok(total)
    }

    /// Adjust by a percentage in basis points (-1000 = 10% off, +500 = 5% markup),
    /// rounding half away from zero.
    pub fn adjust(self, by: BasisPoints) -> Result<Money, MoneyError> {
        const SCALE: i128 = 10_000;

        // i128 so the intermediate product can never overflow
        let numerator = i128::from(self.amount) * (SCALE + i128::from(by.value()));
        let mut result = numerator / SCALE;
        let remainder = numerator % SCALE;

        // round half away from zero
        if remainder.abs() * 2 >= SCALE {
            result += numerator.signum();
        }

        let amount = i64::try_from(result).map_err(|_| MoneyError::Overflow)?;
        Money::new(amount, self.currency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur(n: i64) -> Money {
        Money::new(n, Currency::Eur).unwrap()
    }

    fn bp(n: i32) -> BasisPoints {
        BasisPoints::new(n).unwrap()
    }

    // ---- currency ----

    #[test]
    fn currency_parses_its_own_code_only() {
        for c in [Currency::Eur, Currency::Usd, Currency::Rsd] {
            assert_eq!(c.as_str().parse::<Currency>(), Ok(c));
        }
        assert_eq!(
            "eur".parse::<Currency>(),
            Err(UnknownCurrency("eur".to_string()))
        );
    }

    #[test]
    fn currency_display_matches_json() {
        for c in [Currency::Eur, Currency::Usd, Currency::Rsd] {
            assert_eq!(serde_json::to_string(&c).unwrap(), format!("\"{c}\""));
        }
    }

    #[test]
    fn currency_serializes_uppercase() {
        assert_eq!(serde_json::to_string(&Currency::Eur).unwrap(), "\"EUR\"");
        assert_eq!(
            serde_json::from_str::<Currency>("\"RSD\"").unwrap(),
            Currency::Rsd
        );
        assert!(serde_json::from_str::<Currency>("\"Eur\"").is_err());
    }

    // ---- range ----

    #[test]
    fn accepts_amounts_up_to_the_cap() {
        assert_eq!(eur(Money::MAX_AMOUNT).amount(), 9_007_199_254_740_991);
        assert_eq!(eur(-Money::MAX_AMOUNT).amount(), -9_007_199_254_740_991);
    }

    #[test]
    fn rejects_amounts_beyond_the_cap() {
        assert_eq!(
            Money::new(Money::MAX_AMOUNT + 1, Currency::Eur),
            Err(MoneyError::Overflow)
        );
        assert_eq!(
            Money::new(-Money::MAX_AMOUNT - 1, Currency::Eur),
            Err(MoneyError::Overflow)
        );
        assert_eq!(
            Money::new(i64::MIN, Currency::Eur),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn deserialization_enforces_the_cap() {
        let ok = r#"{"amount":9007199254740991,"currency":"EUR"}"#;
        assert_eq!(
            serde_json::from_str::<Money>(ok).unwrap(),
            eur(Money::MAX_AMOUNT)
        );
        let too_big = r#"{"amount":9007199254740992,"currency":"EUR"}"#;
        assert!(serde_json::from_str::<Money>(too_big).is_err());
    }

    #[test]
    fn serializes_as_amount_and_currency() {
        assert_eq!(
            serde_json::to_string(&eur(1050)).unwrap(),
            r#"{"amount":1050,"currency":"EUR"}"#
        );
    }

    // ---- add ----

    #[test]
    fn adds_same_currency() {
        assert_eq!(eur(1050).checked_add(eur(250)), Ok(eur(1300)));
    }

    #[test]
    fn rejects_mismatched_currency() {
        let usd = Money::new(420, Currency::Usd).unwrap();
        assert_eq!(
            eur(69).checked_add(usd),
            Err(MoneyError::CurrencyMismatch(Currency::Eur, Currency::Usd))
        );
    }

    #[test]
    fn add_overflows() {
        assert_eq!(
            eur(Money::MAX_AMOUNT).checked_add(eur(1)),
            Err(MoneyError::Overflow)
        );
    }

    // ---- sub ----

    #[test]
    fn sub_same_currency() {
        assert_eq!(eur(420).checked_sub(eur(69)), Ok(eur(351)));
    }

    #[test]
    fn sub_can_go_negative() {
        assert_eq!(eur(69).checked_sub(eur(420)), Ok(eur(-351)));
    }

    #[test]
    fn sub_rejects_mismatched_currency() {
        let rsd = Money::new(420, Currency::Rsd).unwrap();
        assert_eq!(
            eur(69).checked_sub(rsd),
            Err(MoneyError::CurrencyMismatch(Currency::Eur, Currency::Rsd))
        );
    }

    #[test]
    fn sub_overflows() {
        assert_eq!(
            eur(-Money::MAX_AMOUNT).checked_sub(eur(1)),
            Err(MoneyError::Overflow)
        );
    }

    // ---- mul ----

    #[test]
    fn mul_within_range() {
        let usd = Money::new(69, Currency::Usd).unwrap();
        assert_eq!(
            usd.checked_mul(420),
            Ok(Money::new(28980, Currency::Usd).unwrap())
        );
    }

    #[test]
    fn mul_overflows() {
        assert_eq!(
            eur(Money::MAX_AMOUNT).checked_mul(2),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn mul_by_zero_gives_zero() {
        let rsd = Money::new(69, Currency::Rsd).unwrap();
        assert_eq!(rsd.checked_mul(0), Ok(Money::zero(Currency::Rsd)));
    }

    // ---- sum ----

    #[test]
    fn sum_empty_is_zero() {
        let empty: Vec<Money> = vec![];
        assert_eq!(Money::sum(&empty, Currency::Eur), Ok(eur(0)));
    }

    #[test]
    fn sum_adds_all_items() {
        let items = vec![eur(67), eur(69), eur(420)];
        assert_eq!(Money::sum(&items, Currency::Eur), Ok(eur(556)));
    }

    #[test]
    fn sum_rejects_mixed_currency() {
        let items = vec![eur(69), Money::new(420, Currency::Rsd).unwrap()];
        assert_eq!(
            Money::sum(&items, Currency::Eur),
            Err(MoneyError::CurrencyMismatch(Currency::Eur, Currency::Rsd))
        );
    }

    #[test]
    fn sum_overflows() {
        let items = vec![eur(Money::MAX_AMOUNT), eur(1)];
        assert_eq!(Money::sum(&items, Currency::Eur), Err(MoneyError::Overflow));
    }

    #[test]
    fn sum_respects_currency_param() {
        let items = vec![
            Money::new(69, Currency::Usd).unwrap(),
            Money::new(67, Currency::Usd).unwrap(),
        ];
        assert_eq!(
            Money::sum(&items, Currency::Eur),
            Err(MoneyError::CurrencyMismatch(Currency::Eur, Currency::Usd))
        );
    }

    // ---- adjust ----

    #[test]
    fn adjust_ten_percent_off() {
        assert_eq!(eur(1000).adjust(bp(-1000)), Ok(eur(900)));
    }

    #[test]
    fn adjust_rounds_half_away_from_zero() {
        // 1005 * 0.9 = 904.5 -> 905
        assert_eq!(eur(1005).adjust(bp(-1000)), Ok(eur(905)));
    }

    #[test]
    fn adjust_rounds_below_half_down() {
        // 1006 * 0.9 = 905.4 -> 905
        assert_eq!(eur(1006).adjust(bp(-1000)), Ok(eur(905)));
    }

    #[test]
    fn adjust_rounds_above_half_up() {
        // 1001 * 0.9 = 900.9 -> 901
        assert_eq!(eur(1001).adjust(bp(-1000)), Ok(eur(901)));
    }

    #[test]
    fn adjust_markup() {
        assert_eq!(eur(1000).adjust(bp(500)), Ok(eur(1050)));
    }

    #[test]
    fn adjust_by_zero_is_unchanged() {
        for amount in [0, 1, 1005, -1005, Money::MAX_AMOUNT] {
            assert_eq!(
                eur(amount).adjust(BasisPoints::ZERO),
                Ok(eur(amount)),
                "{amount}"
            );
        }
    }

    #[test]
    fn adjust_full_discount_gives_zero() {
        assert_eq!(eur(1000).adjust(bp(BasisPoints::MIN)), Ok(eur(0)));
    }

    #[test]
    fn adjust_negative_money_is_symmetric() {
        // -1005 * 0.9 = -904.5 -> -905
        assert_eq!(eur(-1005).adjust(bp(-1000)), Ok(eur(-905)));
    }

    #[test]
    fn adjust_keeps_currency() {
        let rsd = Money::new(1000, Currency::Rsd).unwrap();
        assert_eq!(
            rsd.adjust(bp(-1000)),
            Ok(Money::new(900, Currency::Rsd).unwrap())
        );
    }

    #[test]
    fn adjust_overflows_instead_of_panicking() {
        assert_eq!(
            eur(Money::MAX_AMOUNT).adjust(bp(1)),
            Err(MoneyError::Overflow)
        );
    }
}
