use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CountryCode([u8; 2]);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CountryError {
    #[error("country code must be 2 bytes, got {0}")]
    InvalidLength(usize),
    #[error("country code must be uppercase ASCII letters")]
    NotUppercase,
}

impl CountryCode {
    pub fn new(s: &str) -> Result<Self, CountryError> {
        if s.len() != 2 {
            return Err(CountryError::InvalidLength(s.len()));
        }

        if !s.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(CountryError::NotUppercase);
        }

        let b = s.as_bytes();
        Ok(Self([b[0], b[1]]))
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("country code is ASCII")
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
    fn serializes_as_string() {
        let code = CountryCode::new("HR").unwrap();
        assert_eq!(serde_json::to_string(&code).unwrap(), "\"HR\"");
    }

    #[test]
    fn deserialize_rejects_invalid() {
        assert!(serde_json::from_str::<CountryCode>("\"hr\"").is_err());
    }
}
