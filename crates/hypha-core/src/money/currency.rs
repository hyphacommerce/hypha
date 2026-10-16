use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::tables::CURRENCIES;

/// An ISO 4217 currency a shop can charge in (design note 0021, SU1): any code in core's
/// table, which leaves out funds, precious metals and test codes (XDR, XAU, XXX).
///
/// The three ASCII letters themselves, so it's `Copy`, compares by code and needs no
/// allocation. Every `Currency` holds a code from the table: `new` checks it, and the
/// constants are checked by a test.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Currency([u8; 3]);

/// A code that isn't in the table.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CurrencyError {
    #[error("unknown currency {0:?}: expected an ISO 4217 code such as \"EUR\"")]
    Unknown(String),
}

impl crate::ErrorCode for CurrencyError {
    fn code(&self) -> &'static str {
        match self {
            CurrencyError::Unknown(_) => "unknown_currency",
        }
    }
}

impl Currency {
    pub const EUR: Currency = Currency(*b"EUR");
    pub const USD: Currency = Currency(*b"USD");
    pub const RSD: Currency = Currency(*b"RSD");

    /// Case-sensitive: `"eur"` is unknown, as in JSON.
    pub fn new(code: &str) -> Result<Self, CurrencyError> {
        CURRENCIES
            .binary_search_by(|entry| entry.0.cmp(code))
            .map(|i| Self::from_entry(CURRENCIES[i].0))
            .map_err(|_| CurrencyError::Unknown(code.to_string()))
    }

    /// Every currency in the table, sorted by code.
    pub fn all() -> impl Iterator<Item = Currency> {
        CURRENCIES.iter().map(|entry| Self::from_entry(entry.0))
    }

    /// Table codes are three ASCII letters (a test in `tables` checks it).
    fn from_entry(code: &str) -> Self {
        let b = code.as_bytes();
        Self([b[0], b[1], b[2]])
    }

    /// This currency's row in the table.
    fn entry(&self) -> &'static (&'static str, u8, &'static str) {
        let i = CURRENCIES
            .binary_search_by(|entry| entry.0.as_bytes().cmp(&self.0[..]))
            .expect("every Currency holds a code from the table");
        &CURRENCIES[i]
    }

    /// The ISO 4217 code, the same string used in JSON.
    pub fn as_str(&self) -> &'static str {
        self.entry().0
    }

    /// Digits after the decimal point: 0 for JPY, 2 for EUR, 3 for KWD. Amounts are always
    /// integers in minor units; this only says where a client puts the point (1050 EUR is
    /// 10.50, 1050 JPY is 1050).
    pub fn minor_units(&self) -> u8 {
        self.entry().1
    }

    /// The English name ("Japanese Yen"), for agents and the CLI; UIs show local names.
    pub fn name(&self) -> &'static str {
        self.entry().2
    }
}

/// `EUR` rather than the bytes, so test failures read like the JSON.
impl fmt::Debug for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Currency {
    type Err = CurrencyError;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        Self::new(code)
    }
}

impl TryFrom<String> for Currency {
    type Error = CurrencyError;

    fn try_from(code: String) -> Result<Self, Self::Error> {
        Self::new(&code)
    }
}

impl From<Currency> for String {
    fn from(currency: Currency) -> String {
        currency.as_str().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_in_the_table() {
        for c in [Currency::EUR, Currency::USD, Currency::RSD] {
            assert_eq!(Currency::new(c.as_str()), Ok(c));
        }
    }

    #[test]
    fn accepts_every_table_code() {
        assert_eq!(Currency::all().count(), CURRENCIES.len());
        for c in Currency::all() {
            assert_eq!(c.as_str().parse::<Currency>(), Ok(c));
        }
    }

    #[test]
    fn minor_units_follow_iso() {
        assert_eq!(Currency::new("JPY").unwrap().minor_units(), 0);
        assert_eq!(Currency::EUR.minor_units(), 2);
        assert_eq!(Currency::RSD.minor_units(), 2);
        assert_eq!(Currency::new("KWD").unwrap().minor_units(), 3);
    }

    #[test]
    fn names_are_english() {
        assert_eq!(Currency::new("JPY").unwrap().name(), "Japanese Yen");
        assert_eq!(Currency::EUR.name(), "Euro");
    }

    #[test]
    fn rejects_lowercase() {
        assert_eq!(
            Currency::new("eur"),
            Err(CurrencyError::Unknown("eur".to_string()))
        );
    }

    #[test]
    fn rejects_funds_metals_and_test_codes() {
        for code in ["XDR", "XAU", "XXX", "XTS", "CLF", "USN"] {
            assert_eq!(
                Currency::new(code),
                Err(CurrencyError::Unknown(code.to_string()))
            );
        }
    }

    #[test]
    fn rejects_withdrawn_and_malformed_codes() {
        for code in ["HRK", "", "EU", "EURO", "€"] {
            assert_eq!(
                Currency::new(code),
                Err(CurrencyError::Unknown(code.to_string()))
            );
        }
    }

    #[test]
    fn json_is_the_code() {
        assert_eq!(serde_json::to_string(&Currency::EUR).unwrap(), "\"EUR\"");
        assert_eq!(
            serde_json::from_str::<Currency>("\"JPY\"").unwrap(),
            Currency::new("JPY").unwrap()
        );
        assert!(serde_json::from_str::<Currency>("\"Eur\"").is_err());
        assert!(serde_json::from_str::<Currency>("\"XXX\"").is_err());
    }

    #[test]
    fn debug_and_display_show_the_code() {
        assert_eq!(format!("{:?}", Currency::USD), "USD");
        assert_eq!(Currency::USD.to_string(), "USD");
    }
}
