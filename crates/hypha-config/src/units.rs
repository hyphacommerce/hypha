//! Durations and sizes as people write them (design note 0017, CF8): `"30s"`, `"7d"`,
//! `"2MiB"`. One number and one unit, so there's one way to read each value.

use core::{fmt, str::FromStr, time::Duration};
use std::borrow::Cow;

use hypha_core::ErrorCode;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

/// Why a duration or size didn't read. Each holds the text as written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UnitError {
    #[error("{0:?}: expected a whole number and a unit, like \"30s\"")]
    NoNumber(String),
    #[error("{0:?}: the unit is ms, s, m, h or d, like \"30s\"")]
    DurationUnit(String),
    #[error("{0:?}: the unit is B, KiB, MiB or GiB, like \"2MiB\"")]
    SizeUnit(String),
    #[error("{0:?} is too large")]
    TooLarge(String),
}

impl ErrorCode for UnitError {
    fn code(&self) -> &'static str {
        match self {
            UnitError::NoNumber(_) => "unit_no_number",
            UnitError::DurationUnit(_) => "unit_duration_unit",
            UnitError::SizeUnit(_) => "unit_size_unit",
            UnitError::TooLarge(_) => "unit_too_large",
        }
    }
}

/// A duration: a whole number and `ms`, `s`, `m`, `h` or `d`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dur(pub Duration);

const DURATION_UNITS: [(&str, u64); 5] = [
    ("d", 86_400_000),
    ("h", 3_600_000),
    ("m", 60_000),
    ("s", 1_000),
    ("ms", 1),
];

impl FromStr for Dur {
    type Err = UnitError;

    fn from_str(s: &str) -> Result<Self, UnitError> {
        let (n, unit) = split(s)?;
        let millis = DURATION_UNITS
            .iter()
            .find(|(name, _)| *name == unit)
            .map(|(_, millis)| *millis)
            .ok_or_else(|| UnitError::DurationUnit(s.to_string()))?;
        n.checked_mul(millis)
            .map(|ms| Dur(Duration::from_millis(ms)))
            .ok_or_else(|| UnitError::TooLarge(s.to_string()))
    }
}

impl fmt::Display for Dur {
    /// In the largest unit that holds it exactly: 120 seconds is `2m`, 90 is `90s`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let millis = self.0.as_millis();
        let (unit, size) = DURATION_UNITS
            .iter()
            .find(|(_, size)| millis > 0 && millis.is_multiple_of(u128::from(*size)))
            .copied()
            .unwrap_or(("s", 1_000));
        write!(f, "{}{unit}", millis / u128::from(size))
    }
}

/// A size in bytes: a whole number and `B`, `KiB`, `MiB` or `GiB` (or `KB`, `MB`, `GB`
/// in powers of 1,000).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Size(pub u64);

const SIZE_UNITS: [(&str, u64); 7] = [
    ("GiB", 1 << 30),
    ("MiB", 1 << 20),
    ("KiB", 1 << 10),
    ("GB", 1_000_000_000),
    ("MB", 1_000_000),
    ("KB", 1_000),
    ("B", 1),
];

impl FromStr for Size {
    type Err = UnitError;

    fn from_str(s: &str) -> Result<Self, UnitError> {
        let (n, unit) = split(s)?;
        let bytes = SIZE_UNITS
            .iter()
            .find(|(name, _)| *name == unit)
            .map(|(_, bytes)| *bytes)
            .ok_or_else(|| UnitError::SizeUnit(s.to_string()))?;
        n.checked_mul(bytes)
            .map(Size)
            .ok_or_else(|| UnitError::TooLarge(s.to_string()))
    }
}

impl fmt::Display for Size {
    /// In the largest power-of-1,024 unit that holds it exactly.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (unit, size) = [SIZE_UNITS[0], SIZE_UNITS[1], SIZE_UNITS[2], SIZE_UNITS[6]]
            .into_iter()
            .find(|(_, size)| self.0 > 0 && self.0.is_multiple_of(*size))
            .unwrap_or(("B", 1));
        write!(f, "{}{unit}", self.0 / size)
    }
}

/// `"30s"` → (30, "s"). Spaces around are allowed, not inside.
fn split(s: &str) -> Result<(u64, &str), UnitError> {
    let t = s.trim();
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return Err(UnitError::NoNumber(s.to_string()));
    }
    let n = t[..digits]
        .parse()
        .map_err(|_| UnitError::TooLarge(s.to_string()))?;
    Ok((n, &t[digits..]))
}

macro_rules! as_string {
    ($t:ty, $name:literal, $pattern:literal, $examples:expr, $description:literal) => {
        impl Serialize for $t {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                s.parse().map_err(de::Error::custom)
            }
        }

        impl JsonSchema for $t {
            fn schema_name() -> Cow<'static, str> {
                $name.into()
            }

            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "pattern": $pattern,
                    "examples": $examples,
                    "description": $description,
                })
            }
        }
    };
}

as_string!(
    Dur,
    "Duration",
    "^\\s*[0-9]+(ms|s|m|h|d)\\s*$",
    ["30s", "5m", "1h", "7d"],
    "A whole number and a unit: ms, s, m, h or d."
);
as_string!(
    Size,
    "Size",
    "^\\s*[0-9]+(B|KiB|MiB|GiB|KB|MB|GB)\\s*$",
    ["512KiB", "2MiB"],
    "A whole number and a unit: B, KiB, MiB or GiB (KB, MB and GB are powers of 1,000)."
);

#[cfg(test)]
mod tests {
    use super::*;

    fn dur(s: &str) -> Result<Duration, UnitError> {
        s.parse::<Dur>().map(|d| d.0)
    }

    #[test]
    fn each_failure_says_why() {
        let no_number = |s: &str| Err(UnitError::NoNumber(s.to_string()));
        assert_eq!(dur("s"), no_number("s"));
        assert_eq!(dur("-1s"), no_number("-1s"));
        assert_eq!(dur("1w"), Err(UnitError::DurationUnit("1w".to_string())));
        assert_eq!(
            "2mb".parse::<Size>(),
            Err(UnitError::SizeUnit("2mb".to_string()))
        );
        assert_eq!(
            dur("18446744073709551615d"),
            Err(UnitError::TooLarge("18446744073709551615d".to_string()))
        );
        assert_eq!(
            dur("99999999999999999999d"),
            Err(UnitError::TooLarge("99999999999999999999d".to_string()))
        );
    }

    #[test]
    fn durations_read_every_unit() {
        assert_eq!(dur("250ms"), Ok(Duration::from_millis(250)));
        assert_eq!(dur("30s"), Ok(Duration::from_secs(30)));
        assert_eq!(dur("5m"), Ok(Duration::from_secs(300)));
        assert_eq!(dur(" 2h "), Ok(Duration::from_secs(7200)));
        assert_eq!(dur("7d"), Ok(Duration::from_secs(7 * 86_400)));
        assert_eq!(dur("0s"), Ok(Duration::ZERO));
    }

    #[test]
    fn durations_refuse_what_is_not_one_number_and_a_unit() {
        for bad in ["", "s", "30", "30 s", "1.5h", "-1s", "1w", "1h30m", "30S"] {
            assert!(dur(bad).is_err(), "{bad:?}");
        }
        assert!(dur("99999999999999999999d").is_err());
        assert!(dur("18446744073709551615d").is_err());
    }

    #[test]
    fn durations_print_in_the_largest_exact_unit() {
        let show = |secs| Dur(Duration::from_secs(secs)).to_string();
        assert_eq!(show(120), "2m");
        assert_eq!(show(90), "90s");
        assert_eq!(show(72), "72s");
        assert_eq!(show(86_400 * 30), "30d");
        assert_eq!(show(0), "0s");
        assert_eq!(Dur(Duration::from_millis(1500)).to_string(), "1500ms");
    }

    #[test]
    fn sizes_read_both_kinds_of_unit_and_print_in_binary_ones() {
        assert_eq!("2MiB".parse(), Ok(Size(2 << 20)));
        assert_eq!("2MB".parse(), Ok(Size(2_000_000)));
        assert_eq!("10B".parse(), Ok(Size(10)));
        assert!("2mb".parse::<Size>().is_err());
        assert_eq!(Size(2 << 20).to_string(), "2MiB");
        assert_eq!(Size(1536).to_string(), "1536B");
        assert_eq!(Size(3 << 10).to_string(), "3KiB");
    }
}
