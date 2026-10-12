use serde::Serialize;

use super::{Money, MoneyError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct Price(Money);

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
    use crate::money::Currency;

    #[test]
    fn accepts_zero() {
        let zero = Money::zero(Currency::Eur);
        let price = Price::new(zero).unwrap();
        assert_eq!(price.money(), zero);
    }

    #[test]
    fn rejects_negative() {
        let minus_one = Money::new(-1, Currency::Eur);
        assert_eq!(Price::new(minus_one), Err(MoneyError::Negative));
    }
}