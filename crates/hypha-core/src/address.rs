use thiserror::Error;

use crate::{CountryCode, CountryError};

/// The fields of an address as plain data: what callers fill in to build an `Address`,
/// and what storage and hosts read back out of one. Unvalidated, so the country is a
/// `String` like every other field: `Address::new` checks it (review plan D6).
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

/// A postal address, validated (design note 0005, C4). Shared by customers' address
/// books and, later, the addresses orders snapshot.
///
/// Every text field is trimmed; an empty optional field becomes `None`. Last name, first
/// address line and city are required. Postal code formats aren't checked yet.
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
}

impl crate::ErrorCode for AddressError {
    fn code(&self) -> &'static str {
        match self {
            AddressError::Missing(_) | AddressError::TooLong(_) => "invalid_address",
            AddressError::Country(e) => e.code(),
        }
    }
}

impl Address {
    pub const MAX_FIELD_LEN: usize = 255;

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

fn required(field: &'static str, value: String) -> Result<String, AddressError> {
    optional(field, Some(value))?.ok_or(AddressError::Missing(field))
}

fn optional(field: &'static str, value: Option<String>) -> Result<Option<String>, AddressError> {
    let Some(value) = value else { return Ok(None) };
    let value = value.trim();
    if value.chars().count() > Address::MAX_FIELD_LEN {
        return Err(AddressError::TooLong(field));
    }
    Ok((!value.is_empty()).then(|| value.to_string()))
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

    #[test]
    fn validated_parts_round_trip() {
        let a = Address::new(parts()).unwrap();
        assert_eq!(Address::new(a.parts().clone()), Ok(a));
    }
}
