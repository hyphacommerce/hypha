//! Hypha's amounts are ISO 4217 minor units (`Money`: JPY has none, KWD three). Stripe's
//! mostly agree (<https://docs.stripe.com/currencies>), with exceptions handled here:
//!
//! - ISK and UGX: ISO says no minor units, but Stripe still takes them as two-decimal
//!   amounts ending in `00` ("5 ISK is 500"), so Hypha's amount is multiplied by 100.
//! - Three-decimal currencies (BHD, JOD, KWD, OMR, TND): the same minor units, but Stripe
//!   only charges multiples of 10 (its last digit must be 0).
//! - Zero-decimal currencies (JPY, KRW, …) and HUF/TWD (two-decimal for charges): as is.

use thiserror::Error;

const HUNDREDFOLD: [&str; 2] = ["ISK", "UGX"];
const THREE_DECIMAL: [&str; 5] = ["BHD", "JOD", "KWD", "OMR", "TND"];

/// An amount Stripe can't take.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AmountError {
    #[error("{0} isn't an amount in minor units")]
    Negative(i64),
    #[error("Stripe charges {0} only in multiples of 10 minor units")]
    NotTens(String),
    #[error("{0} {1} is too large for Stripe")]
    TooLarge(i64, String),
}

/// Hypha's minor units to Stripe's `amount` for `currency` (an ISO code, any case).
pub fn to_stripe(amount: i64, currency: &str) -> Result<i64, AmountError> {
    let code = currency.to_ascii_uppercase();
    if amount < 0 {
        return Err(AmountError::Negative(amount));
    }
    if HUNDREDFOLD.contains(&code.as_str()) {
        return amount
            .checked_mul(100)
            .ok_or(AmountError::TooLarge(amount, code));
    }
    if THREE_DECIMAL.contains(&code.as_str()) && amount % 10 != 0 {
        return Err(AmountError::NotTens(code));
    }
    Ok(amount)
}

/// Stripe's `amount` back to Hypha's minor units (rounding a fraction of ISK/UGX down).
pub fn from_stripe(amount: i64, currency: &str) -> i64 {
    if HUNDREDFOLD.contains(&currency.to_ascii_uppercase().as_str()) {
        amount.div_euclid(100)
    } else {
        amount
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_currencies_pass_as_they_are() {
        assert_eq!(to_stripe(1250, "eur"), Ok(1250));
        assert_eq!(to_stripe(500, "JPY"), Ok(500));
        assert_eq!(to_stripe(0, "EUR"), Ok(0));
    }

    #[test]
    fn isk_and_ugx_are_hundredfold_both_ways() {
        assert_eq!(to_stripe(5, "ISK"), Ok(500));
        assert_eq!(from_stripe(500, "isk"), 5);
        assert_eq!(from_stripe(550, "UGX"), 5);
    }

    #[test]
    fn three_decimal_currencies_take_tens_only() {
        assert_eq!(to_stripe(1230, "KWD"), Ok(1230));
        assert_eq!(
            to_stripe(1234, "kwd"),
            Err(AmountError::NotTens("KWD".to_string()))
        );
    }

    #[test]
    fn a_negative_amount_is_refused() {
        assert_eq!(to_stripe(-1, "EUR"), Err(AmountError::Negative(-1)));
    }
}
