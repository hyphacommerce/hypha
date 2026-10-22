use thiserror::Error;

/// How many units of a variant: a cart line, a reservation. Always at least 1, so
/// "nothing" is never a quantity: removing a line or releasing a reservation is its own
/// operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Quantity(u32);

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum QuantityError {
    #[error("quantity must be at least 1")]
    Zero,
    #[error("quantity is too large")]
    Overflow,
}

impl crate::ErrorCode for QuantityError {
    fn code(&self) -> &'static str {
        match self {
            QuantityError::Zero => "invalid_quantity",
            QuantityError::Overflow => "quantity_overflow",
        }
    }
}

impl Quantity {
    /// A single unit: "can one be sold?" (search's availability).
    pub const ONE: Quantity = Quantity(1);

    pub fn new(n: u32) -> Result<Self, QuantityError> {
        if n == 0 {
            Err(QuantityError::Zero)
        } else {
            Ok(Self(n))
        }
    }

    pub fn value(&self) -> u32 {
        self.0
    }

    pub fn checked_add(self, other: Quantity) -> Result<Quantity, QuantityError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(QuantityError::Overflow)
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
        assert_eq!(Quantity::new(0), Err(QuantityError::Zero));
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
            Err(QuantityError::Overflow)
        );
    }
}
