use serde::{Deserialize, Serialize};

use super::MoneyError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "i32", into = "i32")]
pub struct BasisPoints(i32);

impl BasisPoints {
    pub const MIN: i32 = -10_000;
    pub const ZERO: BasisPoints = BasisPoints(0);

    pub fn new(bp: i32) -> Result<Self, MoneyError> {
        if bp < Self::MIN {
            Err(MoneyError::InvalidBasisPoints(bp))
        } else {
            Ok(Self(bp))
        }
    }

    pub fn value(&self) -> i32 {
        self.0
    }
}

impl TryFrom<i32> for BasisPoints {
    type Error = MoneyError;

    fn try_from(bp: i32) -> Result<Self, Self::Error> {
        Self::new(bp)
    }
}

impl From<BasisPoints> for i32 {
    fn from(bp: BasisPoints) -> i32 {
        bp.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_minimum() {
        assert_eq!(BasisPoints::new(-10_000).map(|b| b.value()), Ok(-10_000));
    }

    #[test]
    fn rejects_below_minimum() {
        assert_eq!(
            BasisPoints::new(-10_001),
            Err(MoneyError::InvalidBasisPoints(-10_001))
        );
    }

    #[test]
    fn accepts_large_markup() {
        assert!(BasisPoints::new(1_000_000).is_ok());
    }

    #[test]
    fn deserializes_with_validation() {
        assert_eq!(
            serde_json::from_str::<BasisPoints>("-1000").unwrap(),
            BasisPoints::new(-1000).unwrap()
        );
        assert!(serde_json::from_str::<BasisPoints>("-10001").is_err());
    }

    #[test]
    fn serializes_as_plain_number() {
        let bp = BasisPoints::new(500).unwrap();
        assert_eq!(serde_json::to_string(&bp).unwrap(), "500");
    }
}