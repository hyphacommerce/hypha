use std::fmt;

use crate::InventoryError;

/// What a reservation is for, chosen by the caller (orders will use the order line).
/// Reserving again under the same reference changes nothing, so a retried request
/// can't reserve twice.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReservationRef(String);

impl ReservationRef {
    pub const MAX_LEN: usize = 200;

    pub fn new(reference: &str) -> Result<Self, InventoryError> {
        let valid = !reference.is_empty()
            && reference.chars().count() <= Self::MAX_LEN
            && !reference.chars().any(char::is_control);
        if !valid {
            return Err(InventoryError::InvalidReference);
        }
        Ok(Self(reference.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ReservationRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_an_order_line_like_reference() {
        assert_eq!(
            ReservationRef::new("ordl_0192").map(|r| r.as_str().to_string()),
            Ok("ordl_0192".to_string())
        );
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(
            ReservationRef::new(""),
            Err(InventoryError::InvalidReference)
        );
    }

    #[test]
    fn accepts_max_length_and_rejects_one_more() {
        let max = "r".repeat(ReservationRef::MAX_LEN);
        assert!(ReservationRef::new(&max).is_ok());
        assert_eq!(
            ReservationRef::new(&format!("{max}r")),
            Err(InventoryError::InvalidReference)
        );
    }

    #[test]
    fn rejects_control_characters() {
        assert_eq!(
            ReservationRef::new("line\n1"),
            Err(InventoryError::InvalidReference)
        );
    }
}
