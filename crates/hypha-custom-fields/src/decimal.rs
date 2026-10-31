//! Exact decimal numbers for `decimal` values, measurements and the derived `scalar_num`
//! index column. No floats anywhere in Hypha: a decimal is an integer count of
//! units of `10^-scale`.

use std::{cmp::Ordering, fmt};

use thiserror::Error;

use crate::limits::{MAX_DECIMAL_DIGITS, MAX_DECIMAL_SCALE};

/// An exact decimal, `units × 10^-scale`, keeping the scale it was written with:
/// `"12.50"` stays `12.50` when shown again. Equality and order are by value, so
/// `12.5 == 12.50`.
#[derive(Debug, Clone, Copy)]
pub struct Decimal {
    units: i128,
    scale: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecimalError {
    #[error("{0:?} is not a decimal number like \"12.50\"")]
    Invalid(String),
    #[error("{0:?} has more than {MAX_DECIMAL_DIGITS} digits")]
    TooManyDigits(String),
    #[error("{0:?} has more than {MAX_DECIMAL_SCALE} digits after the point")]
    TooManyDecimals(String),
}

impl hypha_core::ErrorCode for DecimalError {
    fn code(&self) -> &'static str {
        match self {
            DecimalError::Invalid(_) => "invalid_number",
            DecimalError::TooManyDigits(_) | DecimalError::TooManyDecimals(_) => {
                "number_too_precise"
            }
        }
    }
}

/// `10^n` for the scales decimals use (at most 38, the most an `i128` holds).
fn pow10(n: u32) -> Option<i128> {
    10i128.checked_pow(n)
}

impl Decimal {
    pub const ZERO: Decimal = Decimal { units: 0, scale: 0 };

    /// `-?digits(.digits)?`, at most `MAX_DECIMAL_DIGITS` significant digits and
    /// `MAX_DECIMAL_SCALE` after the point. No exponent, no `+`, no spaces.
    pub fn parse(text: &str) -> Result<Decimal, DecimalError> {
        let invalid = || DecimalError::Invalid(text.to_string());
        let (negative, rest) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = match rest.split_once('.') {
            Some((w, f)) => (w, Some(f)),
            None => (rest, None),
        };
        let digits_only = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if !digits_only(whole) || fraction.is_some_and(|f| !digits_only(f)) {
            return Err(invalid());
        }
        let fraction = fraction.unwrap_or("");
        let scale = u32::try_from(fraction.len()).unwrap_or(u32::MAX);
        if scale > MAX_DECIMAL_SCALE {
            return Err(DecimalError::TooManyDecimals(text.to_string()));
        }
        let significant = format!("{whole}{fraction}");
        let significant = significant.trim_start_matches('0');
        if u32::try_from(significant.len()).unwrap_or(u32::MAX) > MAX_DECIMAL_DIGITS {
            return Err(DecimalError::TooManyDigits(text.to_string()));
        }
        let magnitude: i128 = if significant.is_empty() {
            0
        } else {
            significant.parse().map_err(|_| invalid())?
        };
        Ok(Decimal {
            units: if negative { -magnitude } else { magnitude },
            scale,
        })
    }

    pub fn from_i64(value: i64) -> Decimal {
        Decimal {
            units: value.into(),
            scale: 0,
        }
    }

    /// Digits after the point, as written (`"12.50"` has 2).
    pub fn scale(self) -> u32 {
        self.scale
    }

    /// The same value with no trailing zeros after the point: `12.50` is `12.5`, `3.0`
    /// is `3`. What the index stores, so equal values index equally.
    pub fn normalized(self) -> Decimal {
        let mut d = self;
        while d.scale > 0 && d.units % 10 == 0 {
            d.units /= 10;
            d.scale -= 1;
        }
        d
    }

    /// The exact product, or `None` if it doesn't fit.
    pub fn checked_mul(self, other: Decimal) -> Option<Decimal> {
        let d = Decimal {
            units: self.units.checked_mul(other.units)?,
            scale: self.scale.checked_add(other.scale)?,
        };
        pow10(d.scale)?;
        Some(d.normalized())
    }

    /// The whole part (toward zero) and the rest, in units of `10^-scale`.
    fn split(self) -> (i128, i128) {
        // `scale` is at most 38 (`checked_mul` and `parse` keep it so).
        let one = pow10(self.scale).unwrap_or(i128::MAX);
        (self.units / one, self.units % one)
    }
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Decimal {}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Decimal {
    /// Whole parts first, then the rests at a common scale: each rest is below
    /// `10^scale`, so neither step can overflow however large the units are.
    fn cmp(&self, other: &Self) -> Ordering {
        let (a_whole, a_rest) = self.split();
        let (b_whole, b_rest) = other.split();
        a_whole.cmp(&b_whole).then_with(|| {
            let scale = self.scale.max(other.scale);
            let widen =
                |rest: i128, from: u32| pow10(scale - from).and_then(|f| rest.checked_mul(f));
            match (widen(a_rest, self.scale), widen(b_rest, other.scale)) {
                (Some(a), Some(b)) => a.cmp(&b),
                // Not reachable for scales up to 38; compare by sign at least.
                _ => a_rest.signum().cmp(&b_rest.signum()),
            }
        })
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.units < 0 { "-" } else { "" };
        let digits = self.units.unsigned_abs().to_string();
        let scale = self.scale as usize;
        if scale == 0 {
            return write!(f, "{sign}{digits}");
        }
        let padded = format!("{digits:0>width$}", width = scale + 1);
        let (whole, fraction) = padded.split_at(padded.len() - scale);
        write!(f, "{sign}{whole}.{fraction}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::parse(s).unwrap()
    }

    #[test]
    fn keeps_the_scale_it_was_written_with() {
        assert_eq!(d("12.50").to_string(), "12.50");
        assert_eq!(d("-0.05").to_string(), "-0.05");
        assert_eq!(d("007").to_string(), "7");
    }

    #[test]
    fn equal_values_are_equal_whatever_their_scale() {
        assert_eq!(d("12.5"), d("12.500"));
        assert_eq!(d("12.50").normalized().to_string(), "12.5");
        assert_eq!(d("3.0").normalized().to_string(), "3");
    }

    #[test]
    fn orders_by_value() {
        assert!(d("1.05") < d("1.5"));
        assert!(d("-1.5") < d("-1.05"));
        assert!(d("-0.1") < d("0"));
        assert!(d("99999999999999999999") > d("9999999999.9999999999"));
    }

    #[test]
    fn rejects_what_is_not_plain_decimal_notation() {
        for bad in ["", "-", "1.", ".5", "+1", "1e3", " 1", "1,5", "--1", "0x1"] {
            assert_eq!(
                Decimal::parse(bad),
                Err(DecimalError::Invalid(bad.to_string())),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn at_most_twenty_significant_digits() {
        assert!(Decimal::parse(&"9".repeat(20)).is_ok());
        let long = "9".repeat(21);
        assert_eq!(
            Decimal::parse(&long),
            Err(DecimalError::TooManyDigits(long.clone()))
        );
        assert!(Decimal::parse(&format!("000{}", "9".repeat(20))).is_ok());
    }

    #[test]
    fn at_most_ten_digits_after_the_point() {
        assert!(Decimal::parse("0.0000000001").is_ok());
        assert_eq!(
            Decimal::parse("0.00000000001"),
            Err(DecimalError::TooManyDecimals("0.00000000001".to_string()))
        );
    }

    #[test]
    fn multiplies_exactly() {
        assert_eq!(
            d("1.5").checked_mul(d("28.349523125")).unwrap().to_string(),
            "42.5242846875"
        );
    }
}
