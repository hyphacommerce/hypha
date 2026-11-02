use thiserror::Error;

use crate::{CountryCode, CountryError, TextError, optional_text, required_text};

/// The fields of an address as plain data: what callers fill in to build an `Address`,
/// and what storage and hosts read back out of one. Unvalidated, so the country is a
/// `String` like every other field: `Address::new` checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressParts {
    pub first_name: Option<String>,
    pub last_name: String,
    pub company: Option<String>,
    pub line1: String,
    pub line2: Option<String>,
    pub city: String,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub country: String,
    pub phone: Option<String>,
}

/// A postal address, validated. Shared by customers' address books, locations, carts and
/// the addresses orders snapshot.
///
/// Every text field is trimmed; an empty optional field becomes `None`. Last name, first
/// address line and city are required. These are the structural rules only: what a
/// country expects (its postal code format, its regions) is `AddressFormats`', which
/// every write of an address goes through and restoring a stored one never does, so rows
/// written before a format changed still load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    parts: AddressParts,
    country: CountryCode,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum AddressError {
    #[error("address field {0} is required")]
    Missing(&'static str),
    #[error("address field {0} is longer than {max} characters", max = Address::MAX_FIELD_LEN)]
    TooLong(&'static str),
    #[error(transparent)]
    Country(#[from] CountryError),
    /// Every problem found, structural and the country's format's.
    #[error("address has problems: {}", list_problems(.0))]
    Invalid(Vec<AddressProblem>),
}

fn list_problems(problems: &[AddressProblem]) -> String {
    problems
        .iter()
        .map(|p| format!("{} {}", p.field, p.code))
        .collect::<Vec<_>>()
        .join(", ")
}

impl crate::ErrorCode for AddressError {
    fn code(&self) -> &'static str {
        match self {
            AddressError::Missing(_) | AddressError::TooLong(_) | AddressError::Invalid(_) => {
                "invalid_address"
            }
            AddressError::Country(e) => e.code(),
        }
    }
}

impl AddressError {
    /// The problems this error stands for, by field: what a form marks. A wrong country
    /// is one on `country`.
    pub fn problems(&self) -> Vec<AddressProblem> {
        match self {
            AddressError::Invalid(problems) => problems.clone(),
            AddressError::Missing(field) => AddressField::from_name(field)
                .map(|f| vec![AddressProblem::structural(f, AddressProblemCode::Required)])
                .unwrap_or_default(),
            AddressError::TooLong(field) => AddressField::from_name(field)
                .map(|f| vec![AddressProblem::structural(f, AddressProblemCode::TooLong)])
                .unwrap_or_default(),
            AddressError::Country(e) => vec![AddressProblem {
                field: AddressField::Country,
                code: AddressProblemCode::InvalidFormat,
                message: e.to_string(),
            }],
        }
    }
}

impl Address {
    pub const MAX_FIELD_LEN: usize = crate::MAX_TEXT_LEN;

    pub fn new(parts: AddressParts) -> Result<Self, AddressError> {
        let country = CountryCode::new(&parts.country)?;
        let parts = AddressParts {
            first_name: optional("first_name", parts.first_name)?,
            last_name: required("last_name", parts.last_name)?,
            company: optional("company", parts.company)?,
            line1: required("line1", parts.line1)?,
            line2: optional("line2", parts.line2)?,
            city: required("city", parts.city)?,
            province: optional("province", parts.province)?,
            postal_code: optional("postal_code", parts.postal_code)?,
            country: country.as_str().to_string(),
            phone: optional("phone", parts.phone)?,
        };
        Ok(Self { parts, country })
    }

    pub fn parts(&self) -> &AddressParts {
        &self.parts
    }

    pub fn country(&self) -> CountryCode {
        self.country
    }
}

/// A field of an address, named as on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AddressField {
    FirstName,
    LastName,
    Company,
    Line1,
    Line2,
    City,
    Province,
    PostalCode,
    Country,
    Phone,
}

impl AddressField {
    pub const ALL: [AddressField; 10] = [
        AddressField::FirstName,
        AddressField::LastName,
        AddressField::Company,
        AddressField::Line1,
        AddressField::Line2,
        AddressField::City,
        AddressField::Province,
        AddressField::PostalCode,
        AddressField::Country,
        AddressField::Phone,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AddressField::FirstName => "first_name",
            AddressField::LastName => "last_name",
            AddressField::Company => "company",
            AddressField::Line1 => "line1",
            AddressField::Line2 => "line2",
            AddressField::City => "city",
            AddressField::Province => "province",
            AddressField::PostalCode => "postal_code",
            AddressField::Country => "country",
            AddressField::Phone => "phone",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.as_str() == name)
    }

    /// The field's label when a country has none of its own.
    pub fn label(self) -> &'static str {
        match self {
            AddressField::FirstName => "First name",
            AddressField::LastName => "Last name",
            AddressField::Company => "Company",
            AddressField::Line1 => "Address",
            AddressField::Line2 => "Apartment, suite, etc.",
            AddressField::City => "City",
            AddressField::Province => "Region",
            AddressField::PostalCode => "Postal code",
            AddressField::Country => "Country/region",
            AddressField::Phone => "Phone",
        }
    }

    /// The field's value in `parts`; `None` when it's empty.
    pub fn of(self, parts: &AddressParts) -> Option<&str> {
        let value = match self {
            AddressField::FirstName => parts.first_name.as_deref(),
            AddressField::LastName => Some(parts.last_name.as_str()),
            AddressField::Company => parts.company.as_deref(),
            AddressField::Line1 => Some(parts.line1.as_str()),
            AddressField::Line2 => parts.line2.as_deref(),
            AddressField::City => Some(parts.city.as_str()),
            AddressField::Province => parts.province.as_deref(),
            AddressField::PostalCode => parts.postal_code.as_deref(),
            AddressField::Country => Some(parts.country.as_str()),
            AddressField::Phone => parts.phone.as_deref(),
        };
        value.filter(|v| !v.trim().is_empty())
    }
}

impl std::fmt::Display for AddressField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What's wrong with a field. A contract with clients, like error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddressProblemCode {
    /// Empty where the address, or its country, needs it.
    Required,
    /// Over `Address::MAX_FIELD_LEN` characters.
    TooLong,
    /// Not in the shape the country uses (a postal code that doesn't match its pattern).
    InvalidFormat,
    /// Not one of the country's regions.
    UnknownRegion,
    /// A postal code that belongs to another region than the one given.
    RegionMismatch,
}

impl AddressProblemCode {
    pub fn as_str(self) -> &'static str {
        match self {
            AddressProblemCode::Required => "required",
            AddressProblemCode::TooLong => "too_long",
            AddressProblemCode::InvalidFormat => "invalid_format",
            AddressProblemCode::UnknownRegion => "unknown_region",
            AddressProblemCode::RegionMismatch => "region_mismatch",
        }
    }
}

impl std::fmt::Display for AddressProblemCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One field's problem, with a message for the person filling the form in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressProblem {
    pub field: AddressField,
    pub code: AddressProblemCode,
    pub message: String,
}

impl AddressProblem {
    /// One of `Address::new`'s own problems, worded with the field's default label.
    fn structural(field: AddressField, code: AddressProblemCode) -> Self {
        let message = match code {
            AddressProblemCode::TooLong => format!(
                "{} is too long (at most {} characters)",
                field.label(),
                Address::MAX_FIELD_LEN
            ),
            _ => format!("Enter {}", with_article(field.label())),
        };
        Self {
            field,
            code,
            message,
        }
    }
}

/// A field's label as it reads inside a sentence: "State" → "state", but "ZIP code"
/// stays.
pub fn in_sentence(label: &str) -> String {
    let mut chars = label.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if !second.is_uppercase() => {
            first.to_lowercase().chain(label.chars().skip(1)).collect()
        }
        _ => label.to_string(),
    }
}

/// `in_sentence(label)` after "a" or "an": "an address", "a ZIP code".
pub fn with_article(label: &str) -> String {
    let word = in_sentence(label);
    let article = match word.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    };
    format!("{article} {word}")
}

/// Something about an address worth a second look that doesn't stop it being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddressWarningCode {
    /// The country expects a house number and the first line has no digit.
    HouseNumberMissing,
}

impl AddressWarningCode {
    pub fn as_str(self) -> &'static str {
        match self {
            AddressWarningCode::HouseNumberMissing => "house_number_missing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressWarning {
    pub field: AddressField,
    pub code: AddressWarningCode,
    pub message: String,
}

/// What an address is, as far as its text tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AddressFlags {
    /// A post office box in either street line.
    pub po_box: bool,
    /// A US military address (APO, FPO, DPO).
    pub military: bool,
    /// A parcel locker or post office counter (DHL's Packstation, Postfiliale).
    pub parcel_locker: bool,
    /// A letter outside the Latin script in any field.
    pub non_latin: bool,
}

/// An address checked against its country's format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedAddress {
    /// Normalised: the region as its code where the country lists regions, the postal
    /// code upper-cased with its spaces collapsed (or dropped where the country has none).
    pub address: Address,
    /// What stops the address being written.
    pub problems: Vec<AddressProblem>,
    pub warnings: Vec<AddressWarning>,
    pub flags: AddressFlags,
}

/// Each country's address format: required fields, postal code patterns, regions.
pub trait AddressFormats {
    /// Checks a structurally valid address and normalises it. Never refuses: what's wrong
    /// is listed in `problems`.
    fn check(&self, address: &Address) -> CheckedAddress;

    /// The name of `country`'s region `code`, where the country lists its regions.
    fn region_name(&self, country: CountryCode, code: &str) -> Option<&str>;

    /// What every write of an address goes through: `Address::new`'s rules, then the
    /// country's. Refuses with every problem found at once (`AddressError::Invalid`), so a
    /// form can mark each field; a wrong country refuses alone.
    fn validate(&self, parts: AddressParts) -> Result<CheckedAddress, AddressError> {
        let structural = match Address::new(parts.clone()) {
            Ok(address) => {
                let checked = self.check(&address);
                return if checked.problems.is_empty() {
                    Ok(checked)
                } else {
                    Err(AddressError::Invalid(checked.problems))
                };
            }
            Err(AddressError::Country(e)) => return Err(AddressError::Country(e)),
            Err(_) => Address::problems_of(&parts),
        };
        // The country's checks still run on what can be checked, so every problem is
        // reported at once.
        let mut problems = structural;
        if let Some(lenient) = Address::lenient(&parts) {
            let reported: Vec<AddressField> = problems.iter().map(|p| p.field).collect();
            problems.extend(
                self.check(&lenient)
                    .problems
                    .into_iter()
                    .filter(|p| !reported.contains(&p.field)),
            );
        }
        problems.sort_by_key(|p| p.field);
        Err(AddressError::Invalid(problems))
    }
}

impl Address {
    /// `Address::new`'s problems with `parts`, every field's at once.
    fn problems_of(parts: &AddressParts) -> Vec<AddressProblem> {
        let mut problems = Vec::new();
        for field in AddressField::ALL {
            if field == AddressField::Country {
                continue;
            }
            let value = field.of(parts);
            let needed = matches!(
                field,
                AddressField::LastName | AddressField::Line1 | AddressField::City
            );
            match value {
                None if needed => problems.push(AddressProblem::structural(
                    field,
                    AddressProblemCode::Required,
                )),
                Some(v) if v.trim().chars().count() > Address::MAX_FIELD_LEN => problems.push(
                    AddressProblem::structural(field, AddressProblemCode::TooLong),
                ),
                _ => {}
            }
        }
        problems
    }

    /// `parts` trimmed, with `Address::new`'s required fields allowed empty: only for
    /// running a country's checks beside the structural ones. `None` for a wrong country.
    fn lenient(parts: &AddressParts) -> Option<Address> {
        let country = CountryCode::new(&parts.country).ok()?;
        let trim = |v: &Option<String>| {
            v.as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        Some(Address {
            parts: AddressParts {
                first_name: trim(&parts.first_name),
                last_name: parts.last_name.trim().to_string(),
                company: trim(&parts.company),
                line1: parts.line1.trim().to_string(),
                line2: trim(&parts.line2),
                city: parts.city.trim().to_string(),
                province: trim(&parts.province),
                postal_code: trim(&parts.postal_code),
                country: country.as_str().to_string(),
                phone: trim(&parts.phone),
            },
            country,
        })
    }
}

fn required(field: &'static str, value: String) -> Result<String, AddressError> {
    Ok(required_text(field, &value)?)
}

fn optional(field: &'static str, value: Option<String>) -> Result<Option<String>, AddressError> {
    Ok(optional_text(field, value)?)
}

/// Address fields follow the shared text rule but report as `invalid_address`.
impl From<TextError> for AddressError {
    fn from(e: TextError) -> Self {
        match e {
            TextError::Missing(field) => AddressError::Missing(field),
            TextError::TooLong(field) => AddressError::TooLong(field),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> AddressParts {
        AddressParts {
            first_name: Some(" Ana ".to_string()),
            last_name: "Horvat".to_string(),
            company: Some(String::new()),
            line1: "Ilica 1".to_string(),
            line2: None,
            city: "Zagreb".to_string(),
            province: None,
            postal_code: Some("10000".to_string()),
            country: "HR".to_string(),
            phone: Some("  ".to_string()),
        }
    }

    #[test]
    fn trims_and_drops_empty_optional_fields() {
        let a = Address::new(parts()).unwrap();
        assert_eq!(a.parts().first_name.as_deref(), Some("Ana"));
        assert_eq!(a.parts().company, None);
        assert_eq!(a.parts().phone, None);
        assert_eq!(a.parts().postal_code.as_deref(), Some("10000"));
    }

    #[test]
    fn requires_last_name_line1_and_city() {
        for (field, blank) in [
            (
                "last_name",
                AddressParts {
                    last_name: " ".to_string(),
                    ..parts()
                },
            ),
            (
                "line1",
                AddressParts {
                    line1: String::new(),
                    ..parts()
                },
            ),
            (
                "city",
                AddressParts {
                    city: String::new(),
                    ..parts()
                },
            ),
        ] {
            assert_eq!(Address::new(blank), Err(AddressError::Missing(field)));
        }
    }

    #[test]
    fn caps_each_field_at_max_length() {
        let max = "x".repeat(Address::MAX_FIELD_LEN);
        assert!(
            Address::new(AddressParts {
                line2: Some(max.clone()),
                ..parts()
            })
            .is_ok()
        );
        assert_eq!(
            Address::new(AddressParts {
                line2: Some(format!("{max}x")),
                ..parts()
            }),
            Err(AddressError::TooLong("line2"))
        );
    }

    #[test]
    fn checks_the_country() {
        assert_eq!(
            Address::new(AddressParts {
                country: "Croatia".to_string(),
                ..parts()
            }),
            Err(AddressError::Country(CountryError::InvalidLength(7)))
        );
        assert_eq!(Address::new(parts()).unwrap().country().as_str(), "HR");
    }

    /// Postal codes must be five digits; nothing else is checked.
    struct FiveDigits;

    impl AddressFormats for FiveDigits {
        fn check(&self, address: &Address) -> CheckedAddress {
            let ok = address
                .parts()
                .postal_code
                .as_deref()
                .is_some_and(|c| c.len() == 5 && c.bytes().all(|b| b.is_ascii_digit()));
            let problems = if ok {
                Vec::new()
            } else {
                vec![AddressProblem {
                    field: AddressField::PostalCode,
                    code: AddressProblemCode::InvalidFormat,
                    message: "Five digits".to_string(),
                }]
            };
            CheckedAddress {
                address: address.clone(),
                problems,
                warnings: Vec::new(),
                flags: AddressFlags::default(),
            }
        }

        fn region_name(&self, _: CountryCode, _: &str) -> Option<&str> {
            None
        }
    }

    fn codes(e: &AddressError) -> Vec<(AddressField, AddressProblemCode)> {
        e.problems().iter().map(|p| (p.field, p.code)).collect()
    }

    #[test]
    fn validate_answers_the_checked_address() {
        let checked = FiveDigits.validate(parts()).unwrap();
        assert_eq!(checked.address, Address::new(parts()).unwrap());
    }

    #[test]
    fn validate_refuses_the_formats_problems() {
        let e = FiveDigits
            .validate(AddressParts {
                postal_code: Some("1".to_string()),
                ..parts()
            })
            .unwrap_err();
        assert_eq!(
            codes(&e),
            [(AddressField::PostalCode, AddressProblemCode::InvalidFormat)]
        );
    }

    #[test]
    fn validate_reports_structural_and_format_problems_together() {
        let e = FiveDigits
            .validate(AddressParts {
                line1: " ".to_string(),
                city: "x".repeat(Address::MAX_FIELD_LEN + 1),
                postal_code: Some("1".to_string()),
                ..parts()
            })
            .unwrap_err();
        assert_eq!(
            codes(&e),
            [
                (AddressField::Line1, AddressProblemCode::Required),
                (AddressField::City, AddressProblemCode::TooLong),
                (AddressField::PostalCode, AddressProblemCode::InvalidFormat),
            ]
        );
        assert_eq!(crate::ErrorCode::code(&e), "invalid_address");
    }

    #[test]
    fn validate_refuses_a_wrong_country_alone() {
        let e = FiveDigits
            .validate(AddressParts {
                country: "Croatia".to_string(),
                line1: String::new(),
                ..parts()
            })
            .unwrap_err();
        assert_eq!(e, AddressError::Country(CountryError::InvalidLength(7)));
    }

    #[test]
    fn a_missing_field_is_a_required_problem() {
        assert_eq!(
            codes(&AddressError::Missing("line1")),
            [(AddressField::Line1, AddressProblemCode::Required)]
        );
    }

    #[test]
    fn field_names_read_back() {
        for field in AddressField::ALL {
            assert_eq!(AddressField::from_name(field.as_str()), Some(field));
        }
    }

    #[test]
    fn validated_parts_round_trip() {
        let a = Address::new(parts()).unwrap();
        assert_eq!(Address::new(a.parts().clone()), Ok(a));
    }
}
