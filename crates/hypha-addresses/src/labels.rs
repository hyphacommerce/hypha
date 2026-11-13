//! A key for each English label the address data names a field with, so a client can show
//! the label in its own language: the US's "ZIP code" is `zip_code`, Japan's "Prefecture"
//! `prefecture`. The set is closed and a contract: keys are added, never renamed. A label
//! the data gains without a key here has none (a test names it, so a regeneration adds
//! one).

/// What a field's label says, by meaning rather than by field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LabelKey {
    FirstName,
    LastName,
    Company,
    /// "Address": the first street line's default.
    Address1,
    /// "Apartment, suite, etc.": the second line's default.
    Address2,
    StreetAndHouseNumber,
    FullAddress,
    AdditionalAddress,
    City,
    CityTown,
    CityWardTownVillage,
    Ward,
    District,
    Suburb,
    Area,
    /// "Region": the region's default.
    Region,
    Province,
    State,
    StateTerritory,
    County,
    Prefecture,
    Emirate,
    Governorate,
    Department,
    /// "Postal code": the postal code's default.
    PostalCode,
    Postcode,
    ZipCode,
    PinCode,
    Phone,
}

impl LabelKey {
    pub const ALL: [LabelKey; 29] = [
        LabelKey::FirstName,
        LabelKey::LastName,
        LabelKey::Company,
        LabelKey::Address1,
        LabelKey::Address2,
        LabelKey::StreetAndHouseNumber,
        LabelKey::FullAddress,
        LabelKey::AdditionalAddress,
        LabelKey::City,
        LabelKey::CityTown,
        LabelKey::CityWardTownVillage,
        LabelKey::Ward,
        LabelKey::District,
        LabelKey::Suburb,
        LabelKey::Area,
        LabelKey::Region,
        LabelKey::Province,
        LabelKey::State,
        LabelKey::StateTerritory,
        LabelKey::County,
        LabelKey::Prefecture,
        LabelKey::Emirate,
        LabelKey::Governorate,
        LabelKey::Department,
        LabelKey::PostalCode,
        LabelKey::Postcode,
        LabelKey::ZipCode,
        LabelKey::PinCode,
        LabelKey::Phone,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            LabelKey::FirstName => "first_name",
            LabelKey::LastName => "last_name",
            LabelKey::Company => "company",
            LabelKey::Address1 => "address1",
            LabelKey::Address2 => "address2",
            LabelKey::StreetAndHouseNumber => "street_and_house_number",
            LabelKey::FullAddress => "full_address",
            LabelKey::AdditionalAddress => "additional_address",
            LabelKey::City => "city",
            LabelKey::CityTown => "city_town",
            LabelKey::CityWardTownVillage => "city_ward_town_village",
            LabelKey::Ward => "ward",
            LabelKey::District => "district",
            LabelKey::Suburb => "suburb",
            LabelKey::Area => "area",
            LabelKey::Region => "region",
            LabelKey::Province => "province",
            LabelKey::State => "state",
            LabelKey::StateTerritory => "state_territory",
            LabelKey::County => "county",
            LabelKey::Prefecture => "prefecture",
            LabelKey::Emirate => "emirate",
            LabelKey::Governorate => "governorate",
            LabelKey::Department => "department",
            LabelKey::PostalCode => "postal_code",
            LabelKey::Postcode => "postcode",
            LabelKey::ZipCode => "zip_code",
            LabelKey::PinCode => "pin_code",
            LabelKey::Phone => "phone",
        }
    }

    /// The English label this key stands for.
    pub fn label(self) -> &'static str {
        match self {
            LabelKey::FirstName => "First name",
            LabelKey::LastName => "Last name",
            LabelKey::Company => "Company",
            LabelKey::Address1 => "Address",
            LabelKey::Address2 => "Apartment, suite, etc.",
            LabelKey::StreetAndHouseNumber => "Street and house number",
            LabelKey::FullAddress => "Full address",
            LabelKey::AdditionalAddress => "Additional address",
            LabelKey::City => "City",
            LabelKey::CityTown => "City/town",
            LabelKey::CityWardTownVillage => "City/ward/town/village",
            LabelKey::Ward => "Ward",
            LabelKey::District => "District",
            LabelKey::Suburb => "Suburb",
            LabelKey::Area => "Area",
            LabelKey::Region => "Region",
            LabelKey::Province => "Province",
            LabelKey::State => "State",
            LabelKey::StateTerritory => "State/territory",
            LabelKey::County => "County",
            LabelKey::Prefecture => "Prefecture",
            LabelKey::Emirate => "Emirate",
            LabelKey::Governorate => "Governorate",
            LabelKey::Department => "Department",
            LabelKey::PostalCode => "Postal code",
            LabelKey::Postcode => "Postcode",
            LabelKey::ZipCode => "ZIP code",
            LabelKey::PinCode => "PIN code",
            LabelKey::Phone => "Phone",
        }
    }

    /// The key of an English label, matched exactly; `None` for one without.
    pub fn of(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.label() == label)
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::AddressField;

    use super::*;
    use crate::countries::COUNTRIES;

    #[test]
    fn every_label_the_data_names_has_a_key() {
        let mut missing: Vec<&str> = COUNTRIES
            .iter()
            .flat_map(|c| c.labels.iter().map(|(_, label)| *label))
            .chain(AddressField::ALL.into_iter().map(AddressField::label))
            .filter(|label| *label != AddressField::Country.label())
            .filter(|label| LabelKey::of(label).is_none())
            .collect();
        missing.sort_unstable();
        missing.dedup();
        assert_eq!(missing, Vec::<&str>::new(), "add a key for each");
    }

    #[test]
    fn keys_are_unique_snake_case() {
        let mut keys: Vec<&str> = LabelKey::ALL.iter().map(|k| k.as_str()).collect();
        assert!(keys.iter().all(|k| {
            k.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        }));
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), LabelKey::ALL.len());
    }

    #[test]
    fn a_label_reads_back_as_its_key() {
        for key in LabelKey::ALL {
            assert_eq!(LabelKey::of(key.label()), Some(key), "{}", key.as_str());
        }
    }

    #[test]
    fn a_label_without_a_key_has_none() {
        assert_eq!(LabelKey::of("Neighbourhood"), None);
    }
}
