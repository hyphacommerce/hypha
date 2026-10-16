use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::tables::COUNTRIES;

/// An ISO 3166-1 alpha-2 country code (design note 0021, SU2): one of the officially
/// assigned codes in core's table, so `XK` or `UK` don't pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CountryCode([u8; 2]);

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CountryError {
    #[error("country code must be 2 bytes, got {0}")]
    InvalidLength(usize),
    #[error("country code must be uppercase ASCII letters")]
    NotUppercase,
    #[error("unknown country code {0:?}: expected an ISO 3166-1 alpha-2 code such as \"HR\"")]
    Unknown(String),
}

impl crate::ErrorCode for CountryError {
    fn code(&self) -> &'static str {
        match self {
            CountryError::InvalidLength(_) | CountryError::NotUppercase => "invalid_country_code",
            CountryError::Unknown(_) => "unknown_country_code",
        }
    }
}

impl CountryCode {
    pub fn new(s: &str) -> Result<Self, CountryError> {
        if s.len() != 2 {
            return Err(CountryError::InvalidLength(s.len()));
        }

        if !s.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(CountryError::NotUppercase);
        }

        let i = COUNTRIES
            .binary_search_by(|entry| entry.0.cmp(s))
            .map_err(|_| CountryError::Unknown(s.to_string()))?;
        Ok(Self::from_entry(COUNTRIES[i].0))
    }

    /// Every assigned code, sorted.
    pub fn all() -> impl Iterator<Item = CountryCode> {
        COUNTRIES.iter().map(|entry| Self::from_entry(entry.0))
    }

    /// Table codes are two ASCII letters (a test in `tables` checks it).
    fn from_entry(code: &str) -> Self {
        let b = code.as_bytes();
        Self([b[0], b[1]])
    }

    /// This code's row in the table.
    fn entry(&self) -> &'static (&'static str, &'static str) {
        let i = COUNTRIES
            .binary_search_by(|entry| entry.0.as_bytes().cmp(&self.0[..]))
            .expect("every CountryCode holds a code from the table");
        &COUNTRIES[i]
    }

    pub fn as_str(&self) -> &'static str {
        self.entry().0
    }

    /// The English name ("South Korea"), for agents and the CLI; UIs show local names.
    pub fn name(&self) -> &'static str {
        self.entry().1
    }
}

impl fmt::Display for CountryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<String> for CountryCode {
    type Error = CountryError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(&s)
    }
}

impl From<CountryCode> for String {
    fn from(code: CountryCode) -> String {
        code.as_str().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_code() {
        let code = CountryCode::new("RS").unwrap();
        assert_eq!(code.as_str(), "RS");
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(CountryCode::new(""), Err(CountryError::InvalidLength(0)));
        assert_eq!(CountryCode::new("SRB"), Err(CountryError::InvalidLength(3)));
    }

    #[test]
    fn rejects_lowercase_and_digits() {
        assert_eq!(CountryCode::new("rs"), Err(CountryError::NotUppercase));
        assert_eq!(CountryCode::new("R1"), Err(CountryError::NotUppercase));
    }

    #[test]
    fn rejects_multibyte_without_panicking() {
        // 2 characters, but "Ж" is 2 bytes in UTF-8, so 3 bytes total
        assert_eq!(CountryCode::new("Ж1"), Err(CountryError::InvalidLength(3)));
    }

    #[test]
    fn rejects_unassigned_codes() {
        for code in ["XK", "UK", "EU", "ZZ", "AA"] {
            assert_eq!(
                CountryCode::new(code),
                Err(CountryError::Unknown(code.to_string()))
            );
        }
    }

    #[test]
    fn accepts_every_table_code() {
        assert_eq!(CountryCode::all().count(), COUNTRIES.len());
        for code in CountryCode::all() {
            assert_eq!(CountryCode::new(code.as_str()), Ok(code));
        }
    }

    #[test]
    fn names_are_english() {
        assert_eq!(CountryCode::new("RS").unwrap().name(), "Serbia");
        assert_eq!(CountryCode::new("KR").unwrap().name(), "South Korea");
    }

    #[test]
    fn unknown_has_its_own_code() {
        use crate::ErrorCode;
        assert_eq!(
            CountryError::Unknown("XK".to_string()).code(),
            "unknown_country_code"
        );
        assert_eq!(CountryError::NotUppercase.code(), "invalid_country_code");
    }

    #[test]
    fn serializes_as_string() {
        let code = CountryCode::new("HR").unwrap();
        assert_eq!(serde_json::to_string(&code).unwrap(), "\"HR\"");
    }

    #[test]
    fn deserialize_rejects_invalid() {
        assert!(serde_json::from_str::<CountryCode>("\"hr\"").is_err());
        assert!(serde_json::from_str::<CountryCode>("\"XK\"").is_err());
    }
}
