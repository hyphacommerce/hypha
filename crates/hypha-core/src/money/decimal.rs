//! Exact decimal strings for values kept as scaled integers (`UnitCost`, `ExchangeRate`):
//! JSON numbers would pass through floats in most clients.

/// `text` as an integer scaled by 10^`places`: digits, then optionally `.` and 1 to
/// `places` digits. No sign, exponent, blanks, or leading zero before another digit, so
/// every value has one spelling with its trailing zeros dropped. `None` for anything else,
/// or past `max` once scaled.
pub(crate) fn parse(text: &str, places: u32, max: i64) -> Option<i64> {
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (text, ""),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || (whole.len() > 1 && whole.starts_with('0')) {
        return None;
    }
    if text.contains('.') && !digits(fraction) {
        return None;
    }
    let fraction_len = u32::try_from(fraction.len()).ok()?;
    if fraction_len > places {
        return None;
    }
    let scale = 10_i64.checked_pow(places)?;
    let whole: i64 = whole.parse().ok()?;
    let fraction: i64 = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<i64>().ok()? * 10_i64.pow(places - fraction_len)
    };
    let value = whole.checked_mul(scale)?.checked_add(fraction)?;
    (value <= max).then_some(value)
}

/// `value` (scaled by 10^`places`, not negative) as `parse` reads it, trailing zeros
/// dropped: `35` at 4 places is `"0.0035"`, `10_500_000` is `"1050"`.
pub(crate) fn format(value: i64, places: u32) -> String {
    let scale = 10_i64.pow(places);
    let whole = value / scale;
    let fraction = value % scale;
    if fraction == 0 {
        return whole.to_string();
    }
    let width = usize::try_from(places).unwrap_or(0);
    let digits = format!("{fraction:0width$}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

/// `numerator / denominator` rounded half away from zero, as every money division here
/// rounds; `denominator` is positive.
pub fn div_round(numerator: i128, denominator: i128) -> i128 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder.abs() * 2 >= denominator {
        quotient + numerator.signum()
    } else {
        quotient
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_whole_and_fractional_values() {
        assert_eq!(parse("0", 4, i64::MAX), Some(0));
        assert_eq!(parse("1050", 4, i64::MAX), Some(10_500_000));
        assert_eq!(parse("0.35", 4, i64::MAX), Some(3_500));
        assert_eq!(parse("0.0035", 4, i64::MAX), Some(35));
        assert_eq!(parse("12.5000", 4, i64::MAX), Some(125_000));
    }

    #[test]
    fn refuses_more_places_than_allowed() {
        assert_eq!(parse("0.00001", 4, i64::MAX), None);
        assert_eq!(parse("1.123456789", 8, i64::MAX), None);
    }

    #[test]
    fn refuses_anything_but_plain_digits() {
        for text in [
            "", ".5", "5.", "-1", "+1", "1e3", " 1", "1 ", "1,5", "01", "00", "0x1", "1..2",
        ] {
            assert_eq!(parse(text, 4, i64::MAX), None, "{text:?}");
        }
    }

    #[test]
    fn refuses_past_the_maximum() {
        assert_eq!(parse("1", 4, 10_000), Some(10_000));
        assert_eq!(parse("1.0001", 4, 10_000), None);
        assert_eq!(parse("99999999999999999999", 4, i64::MAX), None);
    }

    #[test]
    fn formats_without_trailing_zeros() {
        assert_eq!(format(0, 4), "0");
        assert_eq!(format(35, 4), "0.0035");
        assert_eq!(format(3_500, 4), "0.35");
        assert_eq!(format(10_500_000, 4), "1050");
        assert_eq!(format(117_230_000, 8), "1.1723");
    }

    #[test]
    fn format_reads_back_through_parse() {
        for value in [
            0,
            1,
            9,
            10,
            35,
            3_500,
            10_000,
            10_500_001,
            9_007_199_254_740_991,
        ] {
            assert_eq!(parse(&format(value, 4), 4, i64::MAX), Some(value));
        }
    }

    #[test]
    fn div_round_rounds_half_away_from_zero() {
        assert_eq!(div_round(5, 2), 3);
        assert_eq!(div_round(-5, 2), -3);
        assert_eq!(div_round(4, 3), 1);
        assert_eq!(div_round(-4, 3), -1);
        assert_eq!(div_round(6, 4), 2);
    }
}
