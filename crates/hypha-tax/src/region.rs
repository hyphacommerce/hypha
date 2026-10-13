use hypha_core::{CountryCode, TaxClassId, TaxRate, TaxRegionId, Version, Versioned};

use crate::TaxError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassRate {
    pub class: TaxClassId,
    pub rate: TaxRate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxRegion {
    id: TaxRegionId,
    version: Version,
    country: CountryCode,
    default_rate: TaxRate,
    class_rates: Vec<ClassRate>,
}

impl Versioned for TaxRegion {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl TaxRegion {
    pub fn new(id: TaxRegionId, country: CountryCode, default_rate: TaxRate) -> Self {
        Self {
            id,
            version: Version::NEW,
            country,
            default_rate,
            class_rates: Vec::new(),
        }
    }

    pub fn set_default_rate(&mut self, rate: TaxRate) {
        self.default_rate = rate;
    }

    pub fn set_class_rate(&mut self, class: TaxClassId, rate: TaxRate) {
        match self.class_rates.iter_mut().find(|r| r.class == class) {
            Some(existing) => existing.rate = rate,
            None => self.class_rates.push(ClassRate { class, rate }),
        }
    }

    pub fn remove_class_rate(&mut self, class: TaxClassId) -> Result<(), TaxError> {
        let before = self.class_rates.len();
        self.class_rates.retain(|r| r.class != class);
        if self.class_rates.len() == before {
            return Err(TaxError::ClassRateNotFound(class));
        }
        Ok(())
    }

    pub fn rate_for(&self, class: Option<TaxClassId>) -> (TaxRate, Option<TaxClassId>) {
        class
            .and_then(|c| self.class_rates.iter().find(|r| r.class == c))
            .map(|r| (r.rate, Some(r.class)))
            .unwrap_or((self.default_rate, None))
    }

    pub fn id(&self) -> TaxRegionId {
        self.id
    }

    pub fn country(&self) -> CountryCode {
        self.country
    }

    pub fn default_rate(&self) -> TaxRate {
        self.default_rate
    }

    pub fn class_rates(&self) -> &[ClassRate] {
        &self.class_rates
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxRegionSnapshot {
    pub id: TaxRegionId,
    pub version: Version,
    pub country: CountryCode,
    pub default_rate: TaxRate,
    pub class_rates: Vec<ClassRate>,
}

impl TaxRegion {
    pub fn snapshot(&self) -> TaxRegionSnapshot {
        TaxRegionSnapshot {
            id: self.id,
            version: self.version,
            country: self.country,
            default_rate: self.default_rate,
            class_rates: self.class_rates.clone(),
        }
    }

    pub fn restore(s: TaxRegionSnapshot) -> Result<TaxRegion, TaxError> {
        let mut region = TaxRegion::new(s.id, s.country, s.default_rate);
        for rate in s.class_rates {
            if region.class_rates.iter().any(|r| r.class == rate.class) {
                return Err(TaxError::DuplicateClassRate(rate.class));
            }
            region.class_rates.push(rate)
        }
        region.version = s.version;
        Ok(region)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn region() -> TaxRegion {
        TaxRegion::new(
            TaxRegionId::from_uuid(Uuid::from_u128(1)),
            CountryCode::new("HR").unwrap(),
            rate(250_000),
        )
    }

    fn rate(ppm: u32) -> TaxRate {
        TaxRate::new(ppm).unwrap()
    }

    fn class(n: u128) -> TaxClassId {
        TaxClassId::from_uuid(Uuid::from_u128(n))
    }

    #[test]
    fn unclassified_products_pay_the_default_rate() {
        assert_eq!(region().rate_for(None), (rate(250_000), None));
    }

    #[test]
    fn class_with_a_rate_pays_it() {
        let mut r = region();
        r.set_class_rate(class(2), rate(50_000));
        assert_eq!(r.rate_for(Some(class(2))), (rate(50_000), Some(class(2))));
    }

    #[test]
    fn class_without_a_rate_here_pays_the_default() {
        let mut r = region();
        r.set_class_rate(class(2), rate(50_000));
        assert_eq!(r.rate_for(Some(class(3))), (rate(250_000), None));
    }

    #[test]
    fn set_class_rate_replaces_the_previous_one() {
        let mut r = region();
        r.set_class_rate(class(2), rate(50_000));
        r.set_class_rate(class(2), rate(130_000));
        assert_eq!(
            r.class_rates(),
            &[ClassRate {
                class: class(2),
                rate: rate(130_000)
            }]
        )
    }

    #[test]
    fn remove_class_rate_falls_back_to_default() {
        let mut r = region();
        r.set_class_rate(class(2), rate(50_000));
        assert_eq!(r.remove_class_rate(class(2)), Ok(()));
        assert_eq!(r.rate_for(Some(class(2))), (rate(250_000), None));
    }

    #[test]
    fn remove_missing_class_rate_fails() {
        assert_eq!(
            region().remove_class_rate(class(2)),
            Err(TaxError::ClassRateNotFound(class(2)))
        );
    }

    #[test]
    fn restore_round_trips() {
        let mut r = region();
        r.set_class_rate(class(2), rate(50_000));
        r.set_class_rate(class(3), TaxRate::ZERO);
        r.set_version(Version::new(4));
        assert_eq!(TaxRegion::restore(r.snapshot()), Ok(r));
    }

    #[test]
    fn restore_rejects_duplicate_class_rate() {
        let mut r = region();
        r.set_class_rate(class(2), rate(50_000));
        let mut s = r.snapshot();
        s.class_rates.push(s.class_rates[0]);
        assert_eq!(
            TaxRegion::restore(s),
            Err(TaxError::DuplicateClassRate(class(2)))
        );
    }
}
