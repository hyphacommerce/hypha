use std::fmt;

use serde::{Deserialize, Serialize};

use super::decimal::{self, div_round};
use crate::{Currency, Money, MoneyError};

/// What one unit of stock cost, in minor units with `PLACES` more decimals: a screw bought
/// at €0.0035, or an average over receipts at different prices, isn't a whole cent, and
/// rounding every average to cents drifts on cheap, high-volume items.
///
/// Kept as an integer in 1/10,000 of a minor unit, not negative, at most
/// `Money::MAX_AMOUNT` of those. In JSON it's `{amount, currency}` like `Money`, with
/// `amount` a decimal string of minor units (`"0.35"` is €0.0035, `"1050"` is €10.50), so
/// it reads in the same unit as every other amount and stays exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawUnitCost", into = "RawUnitCost")]
pub struct UnitCost {
    /// In 1/`SCALE` of a minor unit.
    scaled: i64,
    currency: Currency,
}

/// The JSON shape of a `UnitCost`.
#[derive(Serialize, Deserialize)]
struct RawUnitCost {
    amount: String,
    currency: Currency,
}

impl TryFrom<RawUnitCost> for UnitCost {
    type Error = MoneyError;

    fn try_from(raw: RawUnitCost) -> Result<Self, Self::Error> {
        UnitCost::parse(&raw.amount, raw.currency)
    }
}

impl From<UnitCost> for RawUnitCost {
    fn from(cost: UnitCost) -> Self {
        RawUnitCost {
            amount: cost.minor_units(),
            currency: cost.currency,
        }
    }
}

impl UnitCost {
    /// Decimals kept beyond the currency's minor unit.
    pub const PLACES: u32 = 4;
    /// Parts of a minor unit: 10^`PLACES`.
    pub const SCALE: i64 = 10_000;
    /// The most `scaled` holds: exact in JSON and JavaScript, like `Money`.
    pub const MAX_SCALED: i64 = Money::MAX_AMOUNT;

    /// `scaled` is in 1/`SCALE` of a minor unit: 35 EUR is €0.0035. `Negative` below
    /// zero, `Overflow` past `MAX_SCALED`.
    pub fn new(scaled: i64, currency: Currency) -> Result<Self, MoneyError> {
        if scaled < 0 {
            return Err(MoneyError::Negative);
        }
        if scaled > Self::MAX_SCALED {
            return Err(MoneyError::Overflow);
        }
        Ok(Self { scaled, currency })
    }

    pub fn zero(currency: Currency) -> Self {
        Self {
            scaled: 0,
            currency,
        }
    }

    /// The cost of `amount` exactly: `Money` of 1050 EUR is a `UnitCost` of €10.50.
    pub fn from_money(amount: Money) -> Result<Self, MoneyError> {
        let scaled = amount
            .amount()
            .checked_mul(Self::SCALE)
            .ok_or(MoneyError::Overflow)?;
        Self::new(scaled, amount.currency())
    }

    /// A decimal string of minor units with at most `PLACES` decimals, as in JSON:
    /// `"0.35"` is €0.0035. Anything else is `InvalidUnitCost`.
    pub fn parse(minor_units: &str, currency: Currency) -> Result<Self, MoneyError> {
        let scaled = decimal::parse(minor_units, Self::PLACES, Self::MAX_SCALED)
            .ok_or_else(|| MoneyError::InvalidUnitCost(minor_units.to_string()))?;
        Self::new(scaled, currency)
    }

    /// The share of `total` one of `quantity` units carries, rounded half away from zero
    /// at `PLACES`: a €10.00 shipping cost over 3 units is €3.3333 each. `quantity` 0 is
    /// `None`.
    pub fn per_unit(total: Money, quantity: u32) -> Option<Result<Self, MoneyError>> {
        if quantity == 0 {
            return None;
        }
        let scaled = div_round(
            i128::from(total.amount()) * i128::from(Self::SCALE),
            i128::from(quantity),
        );
        Some(Self::from_i128(scaled, total.currency()))
    }

    /// In 1/`SCALE` of a minor unit.
    pub fn scaled(&self) -> i64 {
        self.scaled
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    /// The amount in minor units as a decimal string, as JSON carries it.
    pub fn minor_units(&self) -> String {
        decimal::format(self.scaled, Self::PLACES)
    }

    pub fn checked_add(self, other: UnitCost) -> Result<UnitCost, MoneyError> {
        if self.currency != other.currency {
            return Err(MoneyError::CurrencyMismatch(self.currency, other.currency));
        }
        let scaled = self
            .scaled
            .checked_add(other.scaled)
            .ok_or(MoneyError::Overflow)?;
        Self::new(scaled, self.currency)
    }

    /// What `quantity` units cost, rounded to the minor unit half away from zero once,
    /// on the whole: 3 at €0.0035 is €0.01 (0.0105 cents rounds to 1 cent).
    pub fn total(self, quantity: u32) -> Result<Money, MoneyError> {
        let minor = div_round(
            i128::from(self.scaled) * i128::from(quantity),
            i128::from(Self::SCALE),
        );
        let amount = i64::try_from(minor).map_err(|_| MoneyError::Overflow)?;
        Money::new(amount, self.currency)
    }

    /// This cost with `value` (in 1/`SCALE` of a minor unit, either sign) spread over
    /// `units`: `value / units` added per unit, rounded half away from zero, and never
    /// below zero (a cost can't be negative, so a reduction larger than the cost leaves
    /// none). `units` 0 or less changes nothing.
    pub fn revalued(self, value: i128, units: i128) -> Result<UnitCost, MoneyError> {
        if units <= 0 {
            return Ok(self);
        }
        let shifted = i128::from(self.scaled) + div_round(value, units);
        Self::from_i128(shifted.max(0), self.currency)
    }

    pub(crate) fn from_i128(scaled: i128, currency: Currency) -> Result<Self, MoneyError> {
        let scaled = i64::try_from(scaled).map_err(|_| MoneyError::Overflow)?;
        Self::new(scaled, currency)
    }
}

/// `0.35 EUR`: minor units, as in JSON.
impl fmt::Display for UnitCost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.minor_units(), self.currency)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn eur(scaled: i64) -> UnitCost {
        UnitCost::new(scaled, Currency::EUR).unwrap()
    }

    fn money(amount: i64) -> Money {
        Money::new(amount, Currency::EUR).unwrap()
    }

    #[test]
    fn accepts_zero_and_the_maximum() {
        assert_eq!(
            UnitCost::new(0, Currency::EUR),
            Ok(UnitCost::zero(Currency::EUR))
        );
        assert_eq!(eur(UnitCost::MAX_SCALED).scaled(), 9_007_199_254_740_991);
    }

    #[test]
    fn refuses_negative_costs() {
        assert_eq!(UnitCost::new(-1, Currency::EUR), Err(MoneyError::Negative));
    }

    #[test]
    fn refuses_past_the_maximum() {
        assert_eq!(
            UnitCost::new(UnitCost::MAX_SCALED + 1, Currency::EUR),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn from_money_keeps_the_amount_exactly() {
        assert_eq!(UnitCost::from_money(money(1050)), Ok(eur(10_500_000)));
        assert_eq!(UnitCost::from_money(money(-1)), Err(MoneyError::Negative));
        assert_eq!(
            UnitCost::from_money(money(Money::MAX_AMOUNT)),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn parses_minor_units_with_up_to_four_decimals() {
        assert_eq!(UnitCost::parse("0.35", Currency::EUR), Ok(eur(3_500)));
        assert_eq!(UnitCost::parse("1050", Currency::EUR), Ok(eur(10_500_000)));
        assert_eq!(
            UnitCost::parse("0.00001", Currency::EUR),
            Err(MoneyError::InvalidUnitCost("0.00001".into()))
        );
        assert_eq!(
            UnitCost::parse("-1", Currency::EUR),
            Err(MoneyError::InvalidUnitCost("-1".into()))
        );
    }

    #[test]
    fn json_is_minor_units_as_a_string() {
        let cost = eur(35);
        assert_eq!(
            serde_json::to_value(cost).unwrap(),
            json!({ "amount": "0.0035", "currency": "EUR" })
        );
        assert_eq!(
            serde_json::from_value::<UnitCost>(json!({ "amount": "0.0035", "currency": "EUR" }))
                .unwrap(),
            cost
        );
    }

    #[test]
    fn json_refuses_numbers_and_bad_strings() {
        assert!(
            serde_json::from_value::<UnitCost>(json!({ "amount": 35, "currency": "EUR" })).is_err()
        );
        assert!(
            serde_json::from_value::<UnitCost>(json!({ "amount": "1.23456", "currency": "EUR" }))
                .is_err()
        );
    }

    #[test]
    fn per_unit_rounds_half_away_from_zero_at_four_places() {
        // 1000 cents / 3 = 333.33333… → 333.3333
        assert_eq!(UnitCost::per_unit(money(1000), 3), Some(Ok(eur(3_333_333))));
        // 2000 cents / 3 = 666.66666… → 666.6667
        assert_eq!(UnitCost::per_unit(money(2000), 3), Some(Ok(eur(6_666_667))));
        // 1 cent / 20000 = 0.00005 → 0.0001 (a half rounds up)
        assert_eq!(UnitCost::per_unit(money(1), 20_000), Some(Ok(eur(1))));
        // 1 cent / 20001 = 0.0000499… → 0
        assert_eq!(UnitCost::per_unit(money(1), 20_001), Some(Ok(eur(0))));
    }

    #[test]
    fn per_unit_of_nothing_is_none() {
        assert_eq!(UnitCost::per_unit(money(1000), 0), None);
    }

    #[test]
    fn total_rounds_once_on_the_whole() {
        // 3 × 0.35 cents = 1.05 cents → 1
        assert_eq!(eur(3_500).total(3), Ok(money(1)));
        // 10 × 0.35 cents = 3.5 cents → 4 (half away from zero)
        assert_eq!(eur(3_500).total(10), Ok(money(4)));
        // 2 × 0.2499 cents = 0.4998 → 0
        assert_eq!(eur(2_499).total(2), Ok(money(0)));
        assert_eq!(eur(10_500_000).total(0), Ok(money(0)));
    }

    #[test]
    fn total_overflows_instead_of_panicking() {
        assert_eq!(
            eur(UnitCost::MAX_SCALED).total(u32::MAX),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn add_needs_one_currency() {
        assert_eq!(eur(1).checked_add(eur(2)), Ok(eur(3)));
        assert_eq!(
            eur(1).checked_add(UnitCost::zero(Currency::USD)),
            Err(MoneyError::CurrencyMismatch(Currency::EUR, Currency::USD))
        );
        assert_eq!(
            eur(UnitCost::MAX_SCALED).checked_add(eur(1)),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn revalued_spreads_a_value_over_the_units() {
        // €10.00 (10,000,000 scaled) over 4 units adds 250 cents each.
        assert_eq!(eur(1_000_000).revalued(10_000_000, 4), Ok(eur(3_500_000)));
        // A negative value takes it off.
        assert_eq!(eur(3_500_000).revalued(-10_000_000, 4), Ok(eur(1_000_000)));
    }

    #[test]
    fn revalued_rounds_half_away_from_zero_both_ways() {
        // 1 over 2 is 0.5 → 1; −1 over 2 is −0.5 → −1.
        assert_eq!(eur(10).revalued(1, 2), Ok(eur(11)));
        assert_eq!(eur(10).revalued(-1, 2), Ok(eur(9)));
        // 1 over 3 is 0.33… → 0.
        assert_eq!(eur(10).revalued(1, 3), Ok(eur(10)));
        assert_eq!(eur(10).revalued(-1, 3), Ok(eur(10)));
    }

    #[test]
    fn revalued_never_goes_below_zero() {
        assert_eq!(eur(100).revalued(-1_000, 2), Ok(eur(0)));
    }

    #[test]
    fn revalued_over_no_units_changes_nothing() {
        assert_eq!(eur(100).revalued(5_000, 0), Ok(eur(100)));
        assert_eq!(eur(100).revalued(5_000, -3), Ok(eur(100)));
    }

    #[test]
    fn revalued_overflows_instead_of_panicking() {
        assert_eq!(
            eur(UnitCost::MAX_SCALED).revalued(2, 1),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn displays_minor_units_and_currency() {
        assert_eq!(eur(35).to_string(), "0.0035 EUR");
    }
}
