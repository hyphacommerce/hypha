//! Weight, in whole grams: what carriers and weight-tiered rates read. Grams fit every
//! parcel carriers take; a `u32` holds up to about 4,295 tonnes.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Weight(u32);

impl Weight {
    pub const ZERO: Weight = Weight(0);

    pub fn from_grams(grams: u32) -> Self {
        Self(grams)
    }

    pub fn grams(self) -> u32 {
        self.0
    }
}

/// "1250 g".
impl fmt::Display for Weight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} g", self.0)
    }
}

/// The weight of several things together, in grams: a `u64`, so no cart can overflow it
/// (2^32 parcels of 4,295 tonnes each).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TotalWeight(u64);

impl TotalWeight {
    pub fn grams(self) -> u64 {
        self.0
    }

    /// Adds `quantity` of `weight`.
    pub fn add(self, weight: Weight, quantity: u32) -> Self {
        Self(self.0 + u64::from(weight.grams()) * u64::from(quantity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_total_adds_each_weight_times_its_quantity() {
        let total = TotalWeight::default()
            .add(Weight::from_grams(250), 3)
            .add(Weight::from_grams(1000), 1);
        assert_eq!(total.grams(), 1750);
    }

    #[test]
    fn a_total_holds_the_largest_weights_without_overflowing() {
        let total = TotalWeight::default().add(Weight::from_grams(u32::MAX), u32::MAX);
        assert_eq!(total.grams(), u64::from(u32::MAX) * u64::from(u32::MAX));
    }
}
