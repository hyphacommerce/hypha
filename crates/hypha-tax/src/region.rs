use hypha_core::{
    CountryCode, PendingEvent, Recorded, Recorder, Stamped, Stamps, Subject, TaxClassId, TaxRate,
    TaxRegionId, Version, Versioned,
};

use crate::{
    RestoreError, TaxError,
    events::{TAX_REGION, TaxRegionChanged},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassRate {
    pub class: TaxClassId,
    pub rate: TaxRate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxRegion {
    id: TaxRegionId,
    version: Version,
    /// When it was stored and last changed, set by the adapter.
    stamps: Option<Stamps>,
    country: CountryCode,
    default_rate: TaxRate,
    class_rates: Vec<ClassRate>,
    events: Recorder,
}

impl Recorded for TaxRegion {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for TaxRegion {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for TaxRegion {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl TaxRegion {
    pub fn new(id: TaxRegionId, country: CountryCode, default_rate: TaxRate) -> Self {
        let mut region = Self {
            id,
            version: Version::NEW,
            stamps: None,
            country,
            default_rate,
            class_rates: Vec::new(),
            events: Recorder::default(),
        };
        region.changed();
        region
    }

    fn changed(&mut self) {
        let event = TaxRegionChanged {
            tax_region: self.id,
        };
        self.events
            .record_once(Subject::new(TAX_REGION, self.id.as_uuid()), &event);
    }

    pub fn set_default_rate(&mut self, rate: TaxRate) {
        if self.default_rate != rate {
            self.default_rate = rate;
            self.changed();
        }
    }

    pub fn set_class_rate(&mut self, class: TaxClassId, rate: TaxRate) {
        match self.class_rates.iter_mut().find(|r| r.class == class) {
            Some(existing) if existing.rate == rate => return,
            Some(existing) => existing.rate = rate,
            None => self.class_rates.push(ClassRate { class, rate }),
        }
        self.changed();
    }

    pub fn remove_class_rate(&mut self, class: TaxClassId) -> Result<(), TaxError> {
        let before = self.class_rates.len();
        self.class_rates.retain(|r| r.class != class);
        if self.class_rates.len() == before {
            return Err(TaxError::ClassRateNotFound(class));
        }
        self.changed();
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
    pub stamps: Option<Stamps>,
    pub country: CountryCode,
    pub default_rate: TaxRate,
    pub class_rates: Vec<ClassRate>,
}

impl TaxRegion {
    pub fn snapshot(&self) -> TaxRegionSnapshot {
        TaxRegionSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            country: self.country,
            default_rate: self.default_rate,
            class_rates: self.class_rates.clone(),
        }
    }

    /// Rebuilds a region from storage: each class has one rate at most.
    pub fn restore(s: TaxRegionSnapshot) -> Result<TaxRegion, RestoreError> {
        let mut region = TaxRegion::new(s.id, s.country, s.default_rate);
        for rate in s.class_rates {
            if region.class_rates.iter().any(|r| r.class == rate.class) {
                return Err(RestoreError::DuplicateClassRate(rate.class));
            }
            region.class_rates.push(rate);
        }
        region.version = s.version;
        region.stamps = s.stamps;
        // Built through `new`, which records a change that didn't happen.
        region.events = Recorder::default();
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
        );
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
            Err(RestoreError::DuplicateClassRate(class(2)))
        );
    }

    fn event_types(r: &mut TaxRegion) -> Vec<&'static str> {
        r.take_events().into_iter().map(|e| e.event_type).collect()
    }

    #[test]
    fn restore_records_nothing() {
        let mut restored = TaxRegion::restore(region().snapshot()).unwrap();
        assert_eq!(event_types(&mut restored), Vec::<&str>::new());
    }

    #[test]
    fn changing_rates_records_one_change() {
        let mut r = TaxRegion::restore(region().snapshot()).unwrap();
        r.set_default_rate(rate(130_000));
        r.set_class_rate(class(2), rate(50_000));
        assert_eq!(event_types(&mut r), vec!["tax_region.changed"]);
    }

    #[test]
    fn setting_the_same_rate_records_nothing() {
        let mut r = TaxRegion::restore(region().snapshot()).unwrap();
        r.set_default_rate(rate(250_000));
        assert_eq!(event_types(&mut r), Vec::<&str>::new());
    }
}
