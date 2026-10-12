use crate::error::CartError;

/// How many units of a variant a cart line holds. Always at least 1:
/// removing a line is its own operation, not "set quantity to 0".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Quantity(u32);

impl Quantity {
    pub fn new(n: u32) -> Result<Self, CartError> {
        if n == 0 {
            Err(CartError::ZeroQuantity)
        } else {
            Ok(Self(n))
        }
    }

    pub fn value(&self) -> u32 {
        self.0
    }

    pub fn checked_add(self, other: Quantity) -> Result<Quantity, CartError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(CartError::QuantityOverflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_one() {
        assert_eq!(Quantity::new(1).map(|q| q.value()), Ok(1));
    }

    #[test]
    fn rejects_zero() {
        assert_eq!(Quantity::new(0), Err(CartError::ZeroQuantity));
    }

    #[test]
    fn adds_quantities() {
        let sum = Quantity::new(2)
            .unwrap()
            .checked_add(Quantity::new(3).unwrap());
        assert_eq!(sum, Quantity::new(5));
    }

    #[test]
    fn add_rejects_overflow() {
        let max = Quantity::new(u32::MAX).unwrap();
        assert_eq!(
            max.checked_add(Quantity::new(1).unwrap()),
            Err(CartError::QuantityOverflow)
        );
    }
}
