use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Currency {
    Eur,
    Usd,
    Rsd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Money {
    amount: i64,
    currency: Currency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct Price(Money);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MoneyError {
    #[error("currency mismatch: {0:?} vs {1:?}")]
    CurrencyMismatch(Currency, Currency),
    #[error("amount overflow")]
    Overflow,
    #[error("amount negative")]
    Negative,
}

impl Money {
    pub fn new(amount: i64, currency: Currency) -> Self {
        Self { amount, currency }
    }

    pub fn zero(currency: Currency) -> Self {
        Self::new(0, currency)
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
        Ok(Money::new(amount, self.currency))
    }

    pub fn checked_sub(self, other: Money) -> Result<Money, MoneyError> {
        self.ensure_same_currency(other)?;
        let amount = self
            .amount
            .checked_sub(other.amount)
            .ok_or(MoneyError::Overflow)?;
        Ok(Money::new(amount, self.currency))
    }

    pub fn checked_mul(self, quantity: u32) -> Result<Money, MoneyError> {
        let amount = self
            .amount
            .checked_mul(i64::from(quantity))
            .ok_or(MoneyError::Overflow)?;
        Ok(Money::new(amount, self.currency))
    }

    pub fn sum(items: &[Money], currency: Currency) -> Result<Money, MoneyError> {
        let mut total = Money::zero(currency);
        for item in items {
            total = total.checked_add(*item)?;
        }
        Ok(total)
    }
}

impl Price {
    pub fn new(money: Money) -> Result<Self, MoneyError> {
        if money.amount() < 0 {
            Err(MoneyError::Negative)
        } else {
            Ok(Self(money))
        }
    }

    pub fn money(&self) -> Money {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_same_currency() {
        let a = Money::new(1050, Currency::Eur);
        let b = Money::new(250, Currency::Eur);
        assert_eq!(a.checked_add(b), Ok(Money::new(1300, Currency::Eur)))
    }

    #[test]
    fn rejects_mismatched_currency() {
        let a = Money::new(69, Currency::Eur);
        let b = Money::new(420, Currency::Usd);
        assert_eq!(
            a.checked_add(b),
            Err(MoneyError::CurrencyMismatch(Currency::Eur, Currency::Usd))
        )
    }

    #[test]
    fn add_overflows() {
        let large_amount = Money::new(i64::MAX, Currency::Eur);
        let one = Money::new(1, Currency::Eur);
        assert_eq!(large_amount.checked_add(one), Err(MoneyError::Overflow))
    }

    #[test]
    fn sub_same_currency() {
        let a = Money::new(420, Currency::Eur);
        let b = Money::new(69, Currency::Eur);
        assert_eq!(a.checked_sub(b), Ok(Money::new(351, Currency::Eur)))
    }
    #[test]
    fn sub_can_go_negative() {
        let a = Money::new(69, Currency::Eur);
        let b = Money::new(420, Currency::Eur);
        assert_eq!(a.checked_sub(b), Ok(Money::new(-351, Currency::Eur)));
    }

    #[test]
    fn sub_rejects_mismatched_currency() {
        let a = Money::new(69, Currency::Eur);
        let b = Money::new(420, Currency::Rsd);
        assert_eq!(
            a.checked_sub(b),
            Err(MoneyError::CurrencyMismatch(Currency::Eur, Currency::Rsd))
        );
    }

    #[test]
    fn sub_overflows() {
        assert_eq!(
            Money::new(i64::MIN, Currency::Eur).checked_sub(Money::new(1, Currency::Eur)),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn mult_within_range() {
        let a = Money::new(69, Currency::Usd);
        let b = 420;
        assert_eq!(a.checked_mul(b), Ok(Money::new(28980, Currency::Usd)))
    }

    #[test]
    fn mult_overflows() {
        let large_amount = Money::new(i64::MAX, Currency::Eur);
        let two = 2;
        assert_eq!(large_amount.checked_mul(two), Err(MoneyError::Overflow));
    }

    #[test]
    fn mult_gives_zero() {
        let a = Money::new(69, Currency::Rsd);
        let b = 0;
        assert_eq!(a.checked_mul(b), Ok(Money::zero(Currency::Rsd)))
    }

    #[test]
    fn sum_empty_is_zero() {
        let empty_list: Vec<Money> = vec![];
        assert_eq!(
            Money::sum(&empty_list, Currency::Eur),
            Ok(Money::zero(Currency::Eur))
        )
    }

    #[test]
    fn sum_adds_all_items() {
        let items = vec![
            Money::new(67, Currency::Eur),
            Money::new(69, Currency::Eur),
            Money::new(420, Currency::Eur),
        ];
        assert_eq!(
            Money::sum(&items, Currency::Eur),
            Ok(Money::new(556, Currency::Eur))
        )
    }

    #[test]
    fn sum_rejects_mixed_currency() {
        let mixed_items = vec![
            Money::new(69, Currency::Eur),
            Money::new(420, Currency::Rsd),
        ];
        assert_eq!(
            Money::sum(&mixed_items, Currency::Eur),
            Err(MoneyError::CurrencyMismatch(Currency::Eur, Currency::Rsd))
        )
    }

    #[test]
    fn sum_overflows() {
        let items = vec![
            Money::new(i64::MAX, Currency::Eur),
            Money::new(1, Currency::Eur),
        ];
        assert_eq!(Money::sum(&items, Currency::Eur), Err(MoneyError::Overflow));
    }

    #[test]
    fn sum_respects_currency_param() {
        let items = vec![Money::new(69, Currency::Usd), Money::new(67, Currency::Usd)];
        assert_eq!(
            Money::sum(&items, Currency::Eur),
            Err(MoneyError::CurrencyMismatch(Currency::Eur, Currency::Usd))
        )
    }

    mod price {
        use super::*;

        #[test]
        fn accepts_zero() {
            let zero = Money::new(0, Currency::Eur);
            let price = Price::new(zero).unwrap();
            assert_eq!(price.money(), zero)
        }

        #[test]
        fn rejects_negative() {
            let minus_one = Money::new(-1, Currency::Eur);
            assert_eq!(Price::new(minus_one), Err(MoneyError::Negative));
        }
    }
}
