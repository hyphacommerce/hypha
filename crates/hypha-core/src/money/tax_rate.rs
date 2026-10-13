use serde::{Deserialize, Serialize};

use crate::{Money, MoneyError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct TaxRate(u32);

impl TaxRate {
    pub const MAX: u32 = 1_000_000;
    pub const ZERO: TaxRate = TaxRate(0);

    pub fn new(ppm: u32) -> Result<Self, MoneyError> {
        if ppm > Self::MAX {
            Err(MoneyError::InvalidTaxRate(ppm))
        } else {
            Ok(Self(ppm))
        }
    }

    pub fn value(&self) -> u32 {
        self.0
    }

    pub fn tax_on_net(self, net: Money) -> Result<Money, MoneyError> {
        let rate = i128::from(self.0);
        let tax = div_round(i128::from(net.amount()) * rate, i128::from(Self::MAX));
        to_money(tax, net)
    }

    pub fn tax_in_gross(self, gross: Money) -> Result<Money, MoneyError> {
        let rate = i128::from(self.0);
        let tax = div_round(
            i128::from(gross.amount()) * rate,
            i128::from(Self::MAX) + rate,
        );
        to_money(tax, gross)
    }
}

fn div_round(numerator: i128, denominator: i128) -> i128 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder.abs() * 2 >= denominator {
        quotient + numerator.signum()
    } else {
        quotient
    }
}

fn to_money(amount: i128, like: Money) -> Result<Money, MoneyError> {
    let amount = i64::try_from(amount).map_err(|_| MoneyError::Overflow)?;
    Money::new(amount, like.currency())
}

impl TryFrom<u32> for TaxRate {
    type Error = MoneyError;

    fn try_from(ppm: u32) -> Result<Self, Self::Error> {
        Self::new(ppm)
    }
}

impl From<TaxRate> for u32 {
    fn from(rate: TaxRate) -> u32 {
        rate.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;

    fn eur(n: i64) -> Money {
        Money::new(n, Currency::Eur).unwrap()
    }

    fn rate(ppm: u32) -> TaxRate {
        TaxRate::new(ppm).unwrap()
    }

    #[test]
    fn accepts_zero_and_one_hundred_percent() {
        assert_eq!(TaxRate::new(0), Ok(TaxRate::ZERO));
        assert_eq!(TaxRate::new(1_000_000).map(|r| r.value()), Ok(1_000_000));
    }

    #[test]
    fn rejects_above_one_hundred_percent() {
        assert_eq!(
            TaxRate::new(1_000_001),
            Err(MoneyError::InvalidTaxRate(1_000_001))
        );
    }

    #[test]
    fn deserializes_with_validation() {
        assert_eq!(
            serde_json::from_str::<TaxRate>("250000").unwrap(),
            rate(250_000)
        );
        assert!(serde_json::from_str::<TaxRate>("1000001").is_err());
    }

    #[test]
    fn serializes_as_plain_number() {
        assert_eq!(serde_json::to_string(&rate(88_750)).unwrap(), "88750");
    }

    // ---- on net ----

    #[test]
    fn tax_on_net_multiplies() {
        assert_eq!(rate(250_000).tax_on_net(eur(1000)), Ok(eur(250)));
    }

    #[test]
    fn tax_on_net_supports_three_decimals() {
        assert_eq!(rate(88_750).tax_on_net(eur(1000)), Ok(eur(89)));
    }

    #[test]
    fn tax_on_net_rounds_half_away_from_zero() {
        assert_eq!(rate(250_000).tax_on_net(eur(10)), Ok(eur(3)));
        assert_eq!(rate(250_000).tax_on_net(eur(6)), Ok(eur(2)));
        assert_eq!(rate(250_000).tax_on_net(eur(9)), Ok(eur(2)));
        assert_eq!(rate(250_000).tax_on_net(eur(-10)), Ok(eur(-3)));
    }

    #[test]
    fn tax_on_net_at_the_limits() {
        assert_eq!(TaxRate::ZERO.tax_on_net(eur(1000)), Ok(eur(0)));
        assert_eq!(
            rate(TaxRate::MAX).tax_on_net(eur(Money::MAX_AMOUNT)),
            Ok(eur(Money::MAX_AMOUNT))
        );
    }

    // ---- in gross ----
    #[test]
    fn tax_in_gross_extracts() {
        assert_eq!(rate(250_000).tax_in_gross(eur(1250)), Ok(eur(250)));
    }

    #[test]
    fn tax_in_gross_rounds_half_away_from_zero() {
        assert_eq!(rate(250_000).tax_in_gross(eur(1000)), Ok(eur(200)));
        assert_eq!(rate(200_000).tax_in_gross(eur(999)), Ok(eur(167)));
        assert_eq!(rate(200_000).tax_in_gross(eur(-999)), Ok(eur(-167)));
    }

    #[test]
    fn tax_in_gross_at_the_limits() {
        assert_eq!(TaxRate::ZERO.tax_in_gross(eur(1000)), Ok(eur(0)));
        assert_eq!(rate(TaxRate::MAX).tax_in_gross(eur(1000)), Ok(eur(500)));
    }

    #[test]
    fn tax_keeps_currency() {
        let rsd = Money::new(1000, Currency::Rsd).unwrap();
        assert_eq!(
            rate(200_000).tax_on_net(rsd),
            Ok(Money::new(200, Currency::Rsd).unwrap())
        )
    }
}
