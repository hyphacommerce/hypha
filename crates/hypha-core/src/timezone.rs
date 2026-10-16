use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::tables::TIMEZONES;

/// An IANA time zone name such as `Europe/Belgrade` (design note 0021, Q4), checked
/// against the compiled list of names; core has no rules data, so conversions happen in
/// clients. Old names (`Europe/Kiev`) are accepted and kept as given, as browsers still
/// report some of them.
///
/// It borrows the name from the `'static` table, so it's `Copy` without allocating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Timezone(&'static str);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TimezoneError {
    #[error("unknown time zone {0:?}: expected an IANA name such as \"Europe/Belgrade\"")]
    Unknown(String),
}

impl crate::ErrorCode for TimezoneError {
    fn code(&self) -> &'static str {
        match self {
            TimezoneError::Unknown(_) => "unknown_timezone",
        }
    }
}

impl Timezone {
    /// Case-sensitive, as the names are.
    pub fn new(name: &str) -> Result<Self, TimezoneError> {
        TIMEZONES
            .binary_search(&name)
            .map(|i| Self(TIMEZONES[i]))
            .map_err(|_| TimezoneError::Unknown(name.to_string()))
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for Timezone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl TryFrom<String> for Timezone {
    type Error = TimezoneError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::new(&name)
    }
}

impl From<Timezone> for String {
    fn from(tz: Timezone) -> String {
        tz.0.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_zones_links_and_utc() {
        for name in [
            "Europe/Belgrade",
            "America/Argentina/Buenos_Aires",
            "Europe/Kiev",
            "UTC",
        ] {
            assert_eq!(Timezone::new(name).unwrap().as_str(), name);
        }
    }

    #[test]
    fn rejects_unknown_names() {
        for name in ["europe/belgrade", "Mars/Olympus", "", "Factory", "CET "] {
            assert_eq!(
                Timezone::new(name),
                Err(TimezoneError::Unknown(name.to_string()))
            );
        }
    }

    #[test]
    fn json_is_the_name() {
        let tz: Timezone = serde_json::from_str("\"Asia/Tokyo\"").unwrap();
        assert_eq!(serde_json::to_string(&tz).unwrap(), "\"Asia/Tokyo\"");
        assert!(serde_json::from_str::<Timezone>("\"Asia/Edo\"").is_err());
    }
}
