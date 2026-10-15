//! Shipping zones and their options (design note 0015, sections 2 and 3). A zone owns
//! its options, as a product owns its variants: a cart reads one zone, and the rules
//! between options (unique codes, order) hold in one aggregate.

use std::collections::HashSet;

use hypha_core::{
    CountryCode, PendingEvent, Recorded, Recorder, ShippingOptionId, ShippingZoneId, Subject,
    TaxClassId, Version, Versioned, required_text,
};

use crate::{
    BuiltinRate, DeliveryEstimate, ProviderKey, ShippingError,
    events::{SHIPPING_ZONE, ShippingZoneChanged, ShippingZoneDeleted},
};

/// How an option's price is found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionRate {
    /// Computed by Hypha (SH4).
    Builtin(BuiltinRate),
    /// One service of a provider (SH5, Q2).
    Provider {
        provider: ProviderKey,
        service: String,
    },
}

/// A stable name for an option, for apps and analytics: lowercase ASCII letters, digits,
/// `_` and `-`, starting with a letter or digit; unique within its zone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OptionCode(String);

impl OptionCode {
    pub const MAX_LEN: usize = 64;

    pub fn new(code: &str) -> Result<Self, ShippingError> {
        let mut chars = code.chars();
        let valid = code.len() <= Self::MAX_LEN
            && chars
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
        if !valid {
            return Err(ShippingError::InvalidOptionCode(code.to_string()));
        }
        Ok(Self(code.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a buyer picks (SH3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShippingOption {
    id: ShippingOptionId,
    name: String,
    code: OptionCode,
    rate: OptionRate,
    estimate: Option<DeliveryEstimate>,
    tax_class: Option<TaxClassId>,
    is_backup: bool,
    enabled: bool,
}

impl ShippingOption {
    pub fn id(&self) -> ShippingOptionId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn code(&self) -> &OptionCode {
        &self.code
    }
    pub fn rate(&self) -> &OptionRate {
        &self.rate
    }
    pub fn estimate(&self) -> Option<DeliveryEstimate> {
        self.estimate
    }
    /// The tax class shipping by this option is taxed at; the region's default otherwise
    /// (SH10).
    pub fn tax_class(&self) -> Option<TaxClassId> {
        self.tax_class
    }
    /// Offered only when every provider option of the zone failed (SH7).
    pub fn is_backup(&self) -> bool {
        self.is_backup
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

/// A new option, checked by `ShippingZone::add_option`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOption {
    pub id: ShippingOptionId,
    pub name: String,
    pub code: String,
    pub rate: OptionRate,
    pub estimate: Option<DeliveryEstimate>,
    pub tax_class: Option<TaxClassId>,
    pub is_backup: bool,
    pub enabled: bool,
}

/// Changes to an option; `None` keeps a field. `Some(None)` clears an optional one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OptionPatch {
    pub name: Option<String>,
    pub code: Option<String>,
    pub rate: Option<OptionRate>,
    pub estimate: Option<Option<DeliveryEstimate>>,
    pub tax_class: Option<Option<TaxClassId>>,
    pub is_backup: Option<bool>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShippingZone {
    id: ShippingZoneId,
    version: Version,
    name: String,
    countries: Vec<CountryCode>,
    /// In display order.
    options: Vec<ShippingOption>,
    events: Recorder,
}

impl Recorded for ShippingZone {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for ShippingZone {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl ShippingZone {
    /// Plenty for "Standard, Express, Overnight" per zone, and keeps a zone one load.
    pub const MAX_OPTIONS: usize = 50;

    pub fn new(
        id: ShippingZoneId,
        name: &str,
        countries: Vec<CountryCode>,
    ) -> Result<Self, ShippingError> {
        let mut zone = Self {
            id,
            version: Version::NEW,
            name: required_text("name", name)?,
            countries: Vec::new(),
            options: Vec::new(),
            events: Recorder::default(),
        };
        zone.set_countries(countries)?;
        Ok(zone)
    }

    fn subject(&self) -> Subject {
        Subject::new(SHIPPING_ZONE, self.id.as_uuid())
    }

    fn changed(&mut self) {
        let event = ShippingZoneChanged {
            shipping_zone: self.id,
        };
        self.events.record_once(self.subject(), &event);
    }

    /// Records that the zone is being deleted; the repository's `delete` writes it.
    pub fn delete(&mut self) {
        let event = ShippingZoneDeleted {
            shipping_zone: self.id,
        };
        // Whatever else changed is moot now.
        self.events.take();
        self.events.record(self.subject(), &event);
    }

    pub fn rename(&mut self, name: &str) -> Result<(), ShippingError> {
        self.name = required_text("name", name)?;
        self.changed();
        Ok(())
    }

    /// Each country once. Across zones, a country is the repository's unique key
    /// (`keys::ZONE_COUNTRY`).
    pub fn set_countries(&mut self, countries: Vec<CountryCode>) -> Result<(), ShippingError> {
        let mut seen = HashSet::new();
        if let Some(twice) = countries.iter().find(|c| !seen.insert(**c)) {
            return Err(ShippingError::DuplicateCountry(*twice));
        }
        self.countries = countries;
        self.changed();
        Ok(())
    }

    pub fn add_option(&mut self, new: NewOption) -> Result<(), ShippingError> {
        if self.options.len() >= Self::MAX_OPTIONS {
            return Err(ShippingError::TooManyOptions {
                max: Self::MAX_OPTIONS,
            });
        }
        if self.option(new.id).is_some() {
            return Err(ShippingError::DuplicateOptionId(new.id));
        }
        let code = OptionCode::new(&new.code)?;
        self.check_code(&code, None)?;
        self.options.push(ShippingOption {
            id: new.id,
            name: required_text("name", &new.name)?,
            code,
            rate: checked_rate(new.rate)?,
            estimate: new.estimate,
            tax_class: new.tax_class,
            is_backup: new.is_backup,
            enabled: new.enabled,
        });
        self.changed();
        Ok(())
    }

    pub fn update_option(
        &mut self,
        id: ShippingOptionId,
        patch: OptionPatch,
    ) -> Result<(), ShippingError> {
        let index = self.index_of(id)?;
        let name = patch
            .name
            .map(|name| required_text("name", &name))
            .transpose()?;
        let code = patch.code.map(|code| OptionCode::new(&code)).transpose()?;
        if let Some(code) = &code {
            self.check_code(code, Some(id))?;
        }
        let rate = patch.rate.map(checked_rate).transpose()?;

        let option = &mut self.options[index];
        if let Some(name) = name {
            option.name = name;
        }
        if let Some(code) = code {
            option.code = code;
        }
        if let Some(rate) = rate {
            option.rate = rate;
        }
        if let Some(estimate) = patch.estimate {
            option.estimate = estimate;
        }
        if let Some(tax_class) = patch.tax_class {
            option.tax_class = tax_class;
        }
        if let Some(is_backup) = patch.is_backup {
            option.is_backup = is_backup;
        }
        if let Some(enabled) = patch.enabled {
            option.enabled = enabled;
        }
        self.changed();
        Ok(())
    }

    pub fn remove_option(&mut self, id: ShippingOptionId) -> Result<(), ShippingError> {
        let index = self.index_of(id)?;
        self.options.remove(index);
        self.changed();
        Ok(())
    }

    /// Puts the options in this order; it must name each of them once.
    pub fn reorder_options(&mut self, order: &[ShippingOptionId]) -> Result<(), ShippingError> {
        let mut seen = HashSet::new();
        if order.len() != self.options.len()
            || !order
                .iter()
                .all(|id| seen.insert(*id) && self.option(*id).is_some())
        {
            return Err(ShippingError::OrderNotAllOptions);
        }
        self.options
            .sort_by_key(|o| order.iter().position(|id| *id == o.id));
        self.changed();
        Ok(())
    }

    fn check_code(
        &self,
        code: &OptionCode,
        except: Option<ShippingOptionId>,
    ) -> Result<(), ShippingError> {
        if self
            .options
            .iter()
            .any(|o| Some(o.id) != except && &o.code == code)
        {
            return Err(ShippingError::DuplicateOptionCode(
                code.as_str().to_string(),
            ));
        }
        Ok(())
    }

    fn index_of(&self, id: ShippingOptionId) -> Result<usize, ShippingError> {
        self.options
            .iter()
            .position(|o| o.id == id)
            .ok_or(ShippingError::OptionNotFound(id))
    }

    pub fn id(&self) -> ShippingZoneId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn countries(&self) -> &[CountryCode] {
        &self.countries
    }

    pub fn covers(&self, country: CountryCode) -> bool {
        self.countries.contains(&country)
    }

    pub fn options(&self) -> &[ShippingOption] {
        &self.options
    }

    pub fn option(&self, id: ShippingOptionId) -> Option<&ShippingOption> {
        self.options.iter().find(|o| o.id == id)
    }
}

/// A provider option names a service (trimmed, required); a built-in rate was checked
/// when it was built.
fn checked_rate(rate: OptionRate) -> Result<OptionRate, ShippingError> {
    Ok(match rate {
        OptionRate::Provider { provider, service } => OptionRate::Provider {
            provider,
            service: required_text("service", &service)?,
        },
        builtin => builtin,
    })
}

/// Plain-data form for storage adapters (design note 0001, 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShippingZoneSnapshot {
    pub id: ShippingZoneId,
    pub version: Version,
    pub name: String,
    pub countries: Vec<CountryCode>,
    /// In display order.
    pub options: Vec<NewOption>,
}

impl ShippingZone {
    pub fn snapshot(&self) -> ShippingZoneSnapshot {
        ShippingZoneSnapshot {
            id: self.id,
            version: self.version,
            name: self.name.clone(),
            countries: self.countries.clone(),
            options: self
                .options
                .iter()
                .map(|o| NewOption {
                    id: o.id,
                    name: o.name.clone(),
                    code: o.code.as_str().to_string(),
                    rate: o.rate.clone(),
                    estimate: o.estimate,
                    tax_class: o.tax_class,
                    is_backup: o.is_backup,
                    enabled: o.enabled,
                })
                .collect(),
        }
    }

    /// Rebuilds a zone from storage with every rule `new` and `add_option` check.
    pub fn restore(s: ShippingZoneSnapshot) -> Result<Self, ShippingError> {
        let mut zone = Self::new(s.id, &s.name, s.countries)?;
        for option in s.options {
            zone.add_option(option)?;
        }
        zone.version = s.version;
        // Built through the editing methods; nothing happened, though.
        zone.events = Recorder::default();
        Ok(zone)
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{Currency, Money, Price};
    use uuid::Uuid;

    use super::*;

    fn oid(n: u128) -> ShippingOptionId {
        ShippingOptionId::from_uuid(Uuid::from_u128(n))
    }
    fn country(code: &str) -> CountryCode {
        CountryCode::new(code).unwrap()
    }
    fn flat() -> OptionRate {
        let price = Price::new(Money::new(500, Currency::Eur).unwrap()).unwrap();
        OptionRate::Builtin(BuiltinRate::flat(vec![price]).unwrap())
    }
    fn option(n: u128, code: &str) -> NewOption {
        NewOption {
            id: oid(n),
            name: format!("Option {n}"),
            code: code.to_string(),
            rate: flat(),
            estimate: None,
            tax_class: None,
            is_backup: false,
            enabled: true,
        }
    }
    fn zone() -> ShippingZone {
        ShippingZone::new(
            ShippingZoneId::from_uuid(Uuid::from_u128(1)),
            "EU",
            vec![country("HR"), country("SI")],
        )
        .unwrap()
    }

    #[test]
    fn a_zone_lists_each_country_once() {
        assert_eq!(
            ShippingZone::new(
                ShippingZoneId::from_uuid(Uuid::from_u128(1)),
                "EU",
                vec![country("HR"), country("HR")]
            ),
            Err(ShippingError::DuplicateCountry(country("HR")))
        );
        assert!(zone().covers(country("SI")));
        assert!(!zone().covers(country("AT")));
    }

    #[test]
    fn option_codes_are_unique_within_a_zone() {
        let mut z = zone();
        z.add_option(option(1, "standard")).unwrap();
        assert_eq!(
            z.add_option(option(2, "standard")),
            Err(ShippingError::DuplicateOptionCode("standard".to_string()))
        );
        z.add_option(option(2, "express")).unwrap();
        let rename = OptionPatch {
            code: Some("standard".to_string()),
            ..OptionPatch::default()
        };
        assert_eq!(
            z.update_option(oid(2), rename),
            Err(ShippingError::DuplicateOptionCode("standard".to_string()))
        );
    }

    #[test]
    fn option_codes_are_lowercase_names() {
        for bad in ["", "Standard", "-x", "a b", &"a".repeat(65)] {
            assert_eq!(
                OptionCode::new(bad),
                Err(ShippingError::InvalidOptionCode(bad.to_string())),
                "{bad:?}"
            );
        }
        assert!(OptionCode::new("next-day_2").is_ok());
    }

    #[test]
    fn a_provider_option_names_a_service() {
        let mut z = zone();
        let rate = OptionRate::Provider {
            provider: crate::ProviderKey::new("app_dhl").unwrap(),
            service: "  ".to_string(),
        };
        assert_eq!(
            z.add_option(NewOption {
                rate,
                ..option(1, "dhl")
            }),
            Err(ShippingError::Text(hypha_core::TextError::Missing(
                "service"
            )))
        );
    }

    #[test]
    fn options_reorder_by_a_full_list_only() {
        let mut z = zone();
        z.add_option(option(1, "a")).unwrap();
        z.add_option(option(2, "b")).unwrap();
        assert_eq!(z.reorder_options(&[oid(2), oid(1)]), Ok(()));
        let ids: Vec<_> = z.options().iter().map(|o| o.id()).collect();
        assert_eq!(ids, vec![oid(2), oid(1)]);
        assert_eq!(
            z.reorder_options(&[oid(2)]),
            Err(ShippingError::OrderNotAllOptions)
        );
        assert_eq!(
            z.reorder_options(&[oid(2), oid(2)]),
            Err(ShippingError::OrderNotAllOptions)
        );
    }

    #[test]
    fn a_zone_has_at_most_max_options() {
        let mut z = zone();
        for n in 0..ShippingZone::MAX_OPTIONS as u128 {
            z.add_option(option(n, &format!("o{n}"))).unwrap();
        }
        assert_eq!(
            z.add_option(option(999, "one-more")),
            Err(ShippingError::TooManyOptions {
                max: ShippingZone::MAX_OPTIONS
            })
        );
    }

    #[test]
    fn restore_round_trips() {
        let mut z = zone();
        z.add_option(option(1, "standard")).unwrap();
        z.set_version(Version::new(4));
        assert_eq!(ShippingZone::restore(z.snapshot()), Ok(z));
    }

    #[test]
    fn a_restored_zone_records_nothing_and_a_deleted_one_only_its_deletion() {
        let mut zone = ShippingZone::new(
            ShippingZoneId::from_uuid(Uuid::from_u128(1)),
            "EU",
            vec![country("HR")],
        )
        .unwrap();
        let types = |events: Vec<PendingEvent>| -> Vec<&'static str> {
            events.into_iter().map(|e| e.event_type).collect()
        };
        assert_eq!(types(zone.take_events()), ["shipping_zone.changed"]);
        let mut restored = ShippingZone::restore(zone.snapshot()).unwrap();
        assert!(restored.take_events().is_empty());
        restored.rename("Europe").unwrap();
        restored.delete();
        assert_eq!(types(restored.take_events()), ["shipping_zone.deleted"]);
    }
}
