//! How values read in an email: money with its currency's minor units and a store-wide
//! format, times in the store's time zone. Templates get the formatted text next to the raw
//! value, so one that wants its own format can build it.

use hypha_core::{Currency, Money, Timestamp, Timezone, UnitCost};
use serde::Serialize;

/// Money as templates see it: `{{ order.total.formatted }}`, or the raw `amount` (minor
/// units) and `currency` code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MoneyValue {
    pub amount: i64,
    pub currency: &'static str,
    pub formatted: String,
}

impl MoneyValue {
    pub fn of(money: Money) -> Self {
        Self {
            amount: money.amount(),
            currency: money.currency().as_str(),
            formatted: format_money(money),
        }
    }
}

/// A moment as templates see it: `iso` (RFC 3339 with the zone's offset) and
/// `formatted` ("18 Oct 2026, 14:05"), both in the store's time zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TimeValue {
    pub iso: String,
    pub formatted: String,
}

impl TimeValue {
    /// `at` in `zone` (UTC without one).
    pub fn of(at: Timestamp, zone: Option<Timezone>) -> Self {
        let zoned = zoned(at, zone);
        Self {
            iso: zoned.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string(),
            formatted: zoned.strftime("%-d %b %Y, %H:%M").to_string(),
        }
    }
}

impl TimeValue {
    /// A calendar date kept as noon UTC (a purchase order's expected date), without a
    /// time: `iso` "2027-03-01", `formatted` "1 Mar 2027". Read in UTC, never the store's
    /// zone, which would move it to the next day east of UTC+12.
    pub fn date(at: Timestamp) -> Self {
        let zoned = zoned(at, None);
        Self {
            iso: zoned.strftime("%Y-%m-%d").to_string(),
            formatted: zoned.strftime("%-d %b %Y").to_string(),
        }
    }
}

/// A unit cost as templates see it: `formatted` in the currency's major units with up to
/// four more decimals than it has ("€0.0035", "€12.50"), `amount` the API's decimal
/// string of minor units ("0.35"), and the currency's code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CostValue {
    pub amount: String,
    pub currency: &'static str,
    pub formatted: String,
}

impl CostValue {
    pub fn of(cost: UnitCost) -> Self {
        Self {
            amount: cost.minor_units(),
            currency: cost.currency().as_str(),
            formatted: format_unit_cost(cost),
        }
    }
}

/// `€0.0035`, `€12.50`, `1,000 JPY`: like `format_money`, with the extra decimals a unit
/// cost has kept only where they aren't zero.
pub fn format_unit_cost(cost: UnitCost) -> String {
    let currency = cost.currency();
    let digits = u32::from(currency.minor_units());
    let places = digits + UnitCost::PLACES;
    let scaled = cost.scaled().unsigned_abs();
    let scale = 10u64.pow(places);
    let (whole, fraction) = (scaled / scale, scaled % scale);
    let mut number = group_thousands(whole);
    let width = usize::try_from(places).unwrap_or(0);
    let fraction = format!("{fraction:0width$}");
    // Keep the currency's own digits, and the extra ones up to the last that isn't zero.
    let keep = usize::try_from(digits).unwrap_or(0);
    let trimmed = fraction.trim_end_matches('0');
    let shown = &fraction[..trimmed.len().max(keep)];
    if !shown.is_empty() {
        number.push('.');
        number.push_str(shown);
    }
    match symbol(currency) {
        Some(symbol) => format!("{symbol}{number}"),
        None => format!("{number} {currency}"),
    }
}

fn zoned(at: Timestamp, zone: Option<Timezone>) -> jiff::Zoned {
    // Every `Timestamp` a store holds is within jiff's range (years -9999 to 9999); the
    // epoch stands in for one that isn't rather than failing an email.
    let instant =
        jiff::Timestamp::from_microsecond(at.unix_micros()).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    // The bundled database (not the system's) so the result is the same on every host.
    // A name it lacks (core's list is checked separately) reads as UTC.
    let tz = zone
        .and_then(|zone| {
            jiff::tz::TimeZoneDatabase::bundled()
                .get(zone.as_str())
                .ok()
        })
        .unwrap_or(jiff::tz::TimeZone::UTC);
    instant.to_zoned(tz)
}

/// The symbol written before the amount for the currencies whose symbol is unambiguous
/// in an email; every other currency is written by its code after the amount.
fn symbol(currency: Currency) -> Option<&'static str> {
    match currency.as_str() {
        "EUR" => Some("€"),
        "USD" => Some("$"),
        "GBP" => Some("£"),
        _ => None,
    }
}

/// `€12.50`, `$1,234.00`, `1,234.00 RSD`, `-€0.05`, `1,000 JPY`: the currency's minor
/// units after a `.`, thousands grouped with `,` (the store-wide format for now).
pub fn format_money(money: Money) -> String {
    let currency = money.currency();
    let digits = currency.minor_units() as u32;
    let amount = money.amount();
    let units = amount.unsigned_abs();
    let scale = 10u64.pow(digits);
    let (whole, fraction) = (units / scale, units % scale);
    let mut number = group_thousands(whole);
    if digits > 0 {
        number.push('.');
        number.push_str(&format!("{fraction:0width$}", width = digits as usize));
    }
    let sign = if amount < 0 { "-" } else { "" };
    match symbol(currency) {
        Some(symbol) => format!("{sign}{symbol}{number}"),
        None => format!("{sign}{number} {currency}"),
    }
}

fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn money(amount: i64, code: &str) -> String {
        format_money(Money::new(amount, Currency::new(code).unwrap()).unwrap())
    }

    fn cost(minor: &str, code: &str) -> String {
        format_unit_cost(UnitCost::parse(minor, Currency::new(code).unwrap()).unwrap())
    }

    #[test]
    fn unit_costs_keep_their_extra_decimals_only_when_they_matter() {
        assert_eq!(cost("0.35", "EUR"), "€0.0035");
        assert_eq!(cost("1050", "EUR"), "€10.50");
        assert_eq!(cost("1266.6667", "EUR"), "€12.666667");
        assert_eq!(cost("0", "USD"), "$0.00");
        assert_eq!(cost("123456789", "RSD"), "1,234,567.89 RSD");
        assert_eq!(cost("1000", "JPY"), "1,000 JPY");
        assert_eq!(cost("1000.5", "JPY"), "1,000.5 JPY");
    }

    #[test]
    fn a_date_is_the_utc_day_whatever_the_stores_zone() {
        // Noon UTC on 1 March 2027: already 2 March in Auckland, still 1 March here.
        let noon = Timestamp::from_unix_micros(1_803_902_400_000_000);
        let day = TimeValue::date(noon);
        assert_eq!(
            (day.iso.as_str(), day.formatted.as_str()),
            ("2027-03-01", "1 Mar 2027")
        );
    }

    #[test]
    fn common_currencies_get_their_symbol_first() {
        assert_eq!(money(1250, "EUR"), "€12.50");
        assert_eq!(money(123_400, "USD"), "$1,234.00");
        assert_eq!(money(5, "GBP"), "£0.05");
    }

    #[test]
    fn other_currencies_get_their_code_after() {
        assert_eq!(money(123_400, "RSD"), "1,234.00 RSD");
        assert_eq!(money(0, "CHF"), "0.00 CHF");
    }

    #[test]
    fn minor_units_follow_the_currency() {
        assert_eq!(money(1000, "JPY"), "1,000 JPY");
        assert_eq!(money(1_234_567, "KWD"), "1,234.567 KWD");
    }

    #[test]
    fn thousands_are_grouped_at_every_boundary() {
        assert_eq!(money(99_999, "EUR"), "€999.99");
        assert_eq!(money(100_000, "EUR"), "€1,000.00");
        assert_eq!(money(12_345_678_900, "EUR"), "€123,456,789.00");
    }

    #[test]
    fn negative_amounts_keep_the_sign_in_front() {
        assert_eq!(money(-5, "EUR"), "-€0.05");
        assert_eq!(money(-123_400, "RSD"), "-1,234.00 RSD");
    }

    #[test]
    fn money_keeps_its_raw_amount_and_currency() {
        let value = MoneyValue::of(Money::new(1250, Currency::EUR).unwrap());
        assert_eq!(
            value,
            MoneyValue {
                amount: 1250,
                currency: "EUR",
                formatted: "€12.50".to_string()
            }
        );
    }

    /// 2026-10-18 12:05:00 UTC.
    const AT: Timestamp = Timestamp::from_unix_micros(1_792_325_100_000_000);

    #[test]
    fn times_are_in_utc_without_a_zone() {
        assert_eq!(
            TimeValue::of(AT, None),
            TimeValue {
                iso: "2026-10-18T12:05:00+00:00".to_string(),
                formatted: "18 Oct 2026, 12:05".to_string()
            }
        );
    }

    #[test]
    fn times_are_in_the_stores_zone_with_its_offset() {
        let belgrade = Timezone::new("Europe/Belgrade").ok();
        assert_eq!(
            TimeValue::of(AT, belgrade),
            TimeValue {
                iso: "2026-10-18T14:05:00+02:00".to_string(),
                formatted: "18 Oct 2026, 14:05".to_string()
            }
        );
        // Winter time: one hour ahead.
        let january = Timestamp::from_unix_micros(1_767_225_600_000_000);
        assert_eq!(
            TimeValue::of(january, belgrade).iso,
            "2026-01-01T01:00:00+01:00"
        );
    }

    #[test]
    fn single_digit_days_have_no_leading_zero() {
        let first = Timestamp::from_unix_micros(1_767_225_600_000_000);
        assert_eq!(TimeValue::of(first, None).formatted, "1 Jan 2026, 00:00");
    }
}
