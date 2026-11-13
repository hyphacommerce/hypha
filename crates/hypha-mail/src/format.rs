//! How values read in an email: money with its currency's minor units, times in the store's
//! time zone, both in the email's language (`Formats`, a table generated from CLDR).
//! Templates get the formatted text next to the raw value, so one that wants its own format
//! can build it.

use hypha_core::{Currency, Language, Money, Timestamp, Timezone, UnitCost};
use serde::Serialize;

use crate::generated::formats::FORMATS;

/// One piece of a date pattern, as the generator tokenized CLDR's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Lit(&'static str),
    /// 1 to 31.
    Day,
    /// 01 to 31.
    Day2,
    /// 1 to 12.
    MonthNumber,
    /// 01 to 12.
    MonthNumber2,
    MonthShort,
    MonthLong,
    Year,
    /// 0 to 23.
    Hour,
    /// 00 to 23.
    Hour2,
    /// 00 to 59.
    Minute2,
}

/// How one language writes numbers, money and dates (`generated/formats.rs`, from CLDR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Formats {
    /// The language tag it's for.
    pub tag: &'static str,
    pub decimal: &'static str,
    /// Between groups of three digits.
    pub group: &'static str,
    pub minus: &'static str,
    /// Digits the integer part needs beyond the first group before it's grouped: 2 writes
    /// 1234 ungrouped and 12 345 grouped.
    pub min_grouping: u8,
    /// Whether the currency's sign goes before the number, and what separates them.
    pub sign_before: bool,
    pub sign_space: &'static str,
    /// Whether a currency without a symbol here is written by its code after the number
    /// with a space (English, as before), rather than where a symbol goes.
    pub codes_after: bool,
    /// The currencies written by a symbol, by code; every other one is written by its code.
    pub symbols: &'static [(&'static str, &'static str)],
    /// Month names as a date uses them, January first.
    pub months: [&'static str; 12],
    pub months_long: [&'static str; 12],
    /// A day: "18 Oct 2026".
    pub date: &'static [Part],
    /// A day and a time: "18 Oct 2026, 14:05".
    pub date_time: &'static [Part],
}

impl Formats {
    /// The formats of `language`: its own, else the tag without its region (`de` for
    /// `de-CH-1996`), else the language alone (`sr` for `sr-Cyrl`), else English.
    pub fn of(language: &Language) -> &'static Formats {
        let find = |tag: &str| FORMATS.iter().find(|f| f.tag == tag);
        let primary = language.as_str().split('-').next().unwrap_or_default();
        find(language.as_str())
            .or_else(|| language.without_region().and_then(|l| find(l.as_str())))
            .or_else(|| find(primary))
            .unwrap_or_else(Self::english)
    }

    /// English: what emails wrote before they had a language.
    pub fn english() -> &'static Formats {
        // The generator writes English first.
        &FORMATS[0]
    }

    fn symbol(&self, currency: Currency) -> Option<&'static str> {
        self.symbols
            .iter()
            .find(|(code, _)| *code == currency.as_str())
            .map(|(_, symbol)| *symbol)
    }

    /// `number` (unsigned, already written with this language's marks) with `currency`'s
    /// sign, and the minus in front when `negative`.
    fn with_currency(&self, number: &str, currency: Currency, negative: bool) -> String {
        let sign = if negative { self.minus } else { "" };
        let (affix, is_code) = match self.symbol(currency) {
            Some(symbol) => (symbol, false),
            None => (currency.as_str(), true),
        };
        if is_code && self.codes_after {
            return format!("{sign}{number} {affix}");
        }
        // CLDR's currency spacing: a sign of letters touching the number gets a no-break
        // space ("CHF\u{a0}12.50", never "CHF12.50").
        let touching = if self.sign_before {
            affix.chars().last()
        } else {
            affix.chars().next()
        };
        let space = match (self.sign_space, touching) {
            ("", Some(c)) if c.is_alphabetic() => "\u{a0}",
            (space, _) => space,
        };
        if self.sign_before {
            format!("{sign}{affix}{space}{number}")
        } else {
            format!("{sign}{number}{space}{affix}")
        }
    }

    /// `whole` with groups of three digits marked, from `min_grouping`.
    fn group_digits(&self, whole: u64) -> String {
        let digits = whole.to_string();
        if digits.len() < 4 + usize::from(self.min_grouping.saturating_sub(1)) {
            return digits;
        }
        let mut out = String::with_capacity(digits.len() + digits.len() / 3 * self.group.len());
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push_str(self.group);
            }
            out.push(c);
        }
        out
    }

    fn fill(&self, parts: &[Part], zoned: &jiff::Zoned) -> String {
        let month = usize::try_from(zoned.month()).unwrap_or(1).clamp(1, 12) - 1;
        let mut out = String::new();
        for part in parts {
            match part {
                Part::Lit(text) => out.push_str(text),
                Part::Day => out.push_str(&zoned.day().to_string()),
                Part::Day2 => out.push_str(&format!("{:02}", zoned.day())),
                Part::MonthNumber => out.push_str(&(month + 1).to_string()),
                Part::MonthNumber2 => out.push_str(&format!("{:02}", month + 1)),
                Part::MonthShort => out.push_str(self.months[month]),
                Part::MonthLong => out.push_str(self.months_long[month]),
                Part::Year => out.push_str(&zoned.year().to_string()),
                Part::Hour => out.push_str(&zoned.hour().to_string()),
                Part::Hour2 => out.push_str(&format!("{:02}", zoned.hour())),
                Part::Minute2 => out.push_str(&format!("{:02}", zoned.minute())),
            }
        }
        out
    }
}

/// Which way a language is written: `rtl` for the scripts that run right to left, else
/// `ltr`. What the layout's `dir` gets.
pub fn direction(language: &Language) -> &'static str {
    const RTL_LANGUAGES: [&str; 10] =
        ["ar", "ckb", "dv", "fa", "he", "ps", "sd", "syr", "ug", "ur"];
    const RTL_SCRIPTS: [&str; 6] = ["Arab", "Hebr", "Syrc", "Thaa", "Nkoo", "Adlm"];
    let mut subtags = language.as_str().split('-');
    let primary = subtags.next().unwrap_or_default();
    let script = subtags.next().filter(|s| s.len() == 4);
    let rtl = match script {
        Some(script) => RTL_SCRIPTS.contains(&script),
        None => RTL_LANGUAGES.contains(&primary) || primary == "yi",
    };
    if rtl { "rtl" } else { "ltr" }
}

/// Money as templates see it: `{{ order.total.formatted }}`, or the raw `amount` (minor
/// units) and `currency` code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MoneyValue {
    pub amount: i64,
    pub currency: &'static str,
    pub formatted: String,
}

impl MoneyValue {
    pub fn of(money: Money, formats: &Formats) -> Self {
        Self {
            amount: money.amount(),
            currency: money.currency().as_str(),
            formatted: format_money(money, formats),
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
    /// `at` in `zone` (UTC without one), written the way `formats` writes a day and time.
    pub fn of(at: Timestamp, zone: Option<Timezone>, formats: &Formats) -> Self {
        let zoned = zoned(at, zone);
        Self {
            iso: zoned.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string(),
            formatted: formats.fill(formats.date_time, &zoned),
        }
    }

    /// A calendar date kept as noon UTC (a purchase order's expected date), without a
    /// time: `iso` "2027-03-01", `formatted` "1 Mar 2027". Read in UTC, never the store's
    /// zone, which would move it to the next day east of UTC+12.
    pub fn date(at: Timestamp, formats: &Formats) -> Self {
        let zoned = zoned(at, None);
        Self {
            iso: zoned.strftime("%Y-%m-%d").to_string(),
            formatted: formats.fill(formats.date, &zoned),
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
    pub fn of(cost: UnitCost, formats: &Formats) -> Self {
        Self {
            amount: cost.minor_units(),
            currency: cost.currency().as_str(),
            formatted: format_unit_cost(cost, formats),
        }
    }
}

/// `€0.0035`, `€12.50`, `1,000 JPY`: like `format_money`, with the extra decimals a unit
/// cost has kept only where they aren't zero.
pub fn format_unit_cost(cost: UnitCost, formats: &Formats) -> String {
    let currency = cost.currency();
    let digits = u32::from(currency.minor_units());
    let places = digits + UnitCost::PLACES;
    let scaled = cost.scaled().unsigned_abs();
    let scale = 10u64.pow(places);
    let (whole, fraction) = (scaled / scale, scaled % scale);
    let mut number = formats.group_digits(whole);
    let width = usize::try_from(places).unwrap_or(0);
    let fraction = format!("{fraction:0width$}");
    // Keep the currency's own digits, and the extra ones up to the last that isn't zero.
    let keep = usize::try_from(digits).unwrap_or(0);
    let trimmed = fraction.trim_end_matches('0');
    let shown = &fraction[..trimmed.len().max(keep)];
    if !shown.is_empty() {
        number.push_str(formats.decimal);
        number.push_str(shown);
    }
    formats.with_currency(&number, currency, false)
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

/// `€12.50`, `$1,234.00`, `1,234.00 RSD`, `-€0.05`, `1,000 JPY` in English; `1.234,56 €`
/// in German: the currency's minor units after the language's decimal mark, thousands
/// grouped, the sign where the language puts it.
pub fn format_money(money: Money, formats: &Formats) -> String {
    let currency = money.currency();
    let digits = u32::from(currency.minor_units());
    let amount = money.amount();
    let units = amount.unsigned_abs();
    let scale = 10u64.pow(digits);
    let (whole, fraction) = (units / scale, units % scale);
    let mut number = formats.group_digits(whole);
    if digits > 0 {
        number.push_str(formats.decimal);
        let width = usize::try_from(digits).unwrap_or(0);
        number.push_str(&format!("{fraction:0width$}"));
    }
    formats.with_currency(&number, currency, amount < 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn money(amount: i64, code: &str) -> String {
        format_money(
            Money::new(amount, Currency::new(code).unwrap()).unwrap(),
            Formats::english(),
        )
    }

    fn cost(minor: &str, code: &str) -> String {
        format_unit_cost(
            UnitCost::parse(minor, Currency::new(code).unwrap()).unwrap(),
            Formats::english(),
        )
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
        let day = TimeValue::date(noon, Formats::english());
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
        let value = MoneyValue::of(Money::new(1250, Currency::EUR).unwrap(), Formats::english());
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
            TimeValue::of(AT, None, Formats::english()),
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
            TimeValue::of(AT, belgrade, Formats::english()),
            TimeValue {
                iso: "2026-10-18T14:05:00+02:00".to_string(),
                formatted: "18 Oct 2026, 14:05".to_string()
            }
        );
        // Winter time: one hour ahead.
        let january = Timestamp::from_unix_micros(1_767_225_600_000_000);
        assert_eq!(
            TimeValue::of(january, belgrade, Formats::english()).iso,
            "2026-01-01T01:00:00+01:00"
        );
    }

    #[test]
    fn single_digit_days_have_no_leading_zero() {
        let first = Timestamp::from_unix_micros(1_767_225_600_000_000);
        assert_eq!(
            TimeValue::of(first, None, Formats::english()).formatted,
            "1 Jan 2026, 00:00"
        );
    }

    fn lang(tag: &str) -> &'static Formats {
        Formats::of(&Language::new(tag).unwrap())
    }

    fn money_in(tag: &str, amount: i64, code: &str) -> String {
        format_money(
            Money::new(amount, Currency::new(code).unwrap()).unwrap(),
            lang(tag),
        )
    }

    /// 2026-11-13 09:30:00 UTC.
    const NOV_13: Timestamp = Timestamp::from_unix_micros(1_794_562_200_000_000);

    #[test]
    fn german_puts_the_euro_after_with_a_no_break_space() {
        assert_eq!(money_in("de", 123_456, "EUR"), "1.234,56\u{a0}€");
        assert_eq!(money_in("de", -5, "EUR"), "-0,05\u{a0}€");
        assert_eq!(money_in("de", 123_400, "RSD"), "1.234,00\u{a0}RSD");
    }

    #[test]
    fn german_dates_have_the_day_first_and_its_month_abbreviated() {
        assert_eq!(
            TimeValue::of(NOV_13, None, lang("de")).formatted,
            "13. Nov. 2026, 09:30"
        );
        assert_eq!(
            TimeValue::date(NOV_13, lang("de")).formatted,
            "13. Nov. 2026"
        );
    }

    #[test]
    fn regional_german_writes_its_own_marks() {
        // Austria: the sign first; Switzerland: an apostrophe between groups and francs by
        // their code where the sign goes.
        assert_eq!(money_in("de-AT", 123_456, "EUR"), "€\u{a0}1\u{a0}234,56");
        assert_eq!(money_in("de-CH", 123_456, "CHF"), "CHF\u{a0}1'234.56");
        assert_eq!(
            TimeValue::date(NOV_13, lang("de-AT")).formatted,
            "13. Nov. 2026"
        );
    }

    #[test]
    fn serbian_latin_writes_dinars_by_code_and_dates_with_a_final_dot() {
        assert_eq!(money_in("sr-Latn", 123_456, "RSD"), "1.234,56\u{a0}RSD");
        assert_eq!(
            TimeValue::of(NOV_13, None, lang("sr-Latn")).formatted,
            "13. nov 2026. 09:30"
        );
    }

    #[test]
    fn some_languages_group_only_from_five_digits() {
        assert_eq!(money_in("pl", 123_456, "PLN"), "1234,56\u{a0}zł");
        assert_eq!(money_in("pl", 1_234_567, "PLN"), "12\u{a0}345,67\u{a0}zł");
    }

    #[test]
    fn a_language_without_its_own_table_falls_back_to_its_tag_then_english() {
        assert_eq!(lang("de-DE").tag, "de");
        assert_eq!(lang("sr-Latn-RS").tag, "sr-Latn");
        assert_eq!(lang("sr-Cyrl").tag, "sr");
        assert_eq!(lang("en-US").tag, "en");
        assert_eq!(lang("ja").tag, "en");
    }

    #[test]
    fn english_is_the_first_table_and_its_own_tag() {
        assert_eq!(Formats::english().tag, "en");
        assert_eq!(lang("en"), Formats::english());
    }

    #[test]
    fn every_table_has_a_distinct_tag_and_twelve_named_months() {
        for (i, f) in FORMATS.iter().enumerate() {
            assert!(
                FORMATS[..i].iter().all(|g| g.tag != f.tag),
                "{} twice",
                f.tag
            );
            assert!(Language::new(f.tag).is_ok(), "{}", f.tag);
            assert!(f.months.iter().chain(&f.months_long).all(|m| !m.is_empty()));
        }
    }

    #[test]
    fn right_to_left_follows_the_script_then_the_language() {
        let dir = |tag: &str| direction(&Language::new(tag).unwrap());
        assert_eq!(dir("ar"), "rtl");
        assert_eq!(dir("he-IL"), "rtl");
        assert_eq!(dir("sr-Latn"), "ltr");
        assert_eq!(dir("en"), "ltr");
        assert_eq!(dir("az-Arab"), "rtl");
        assert_eq!(dir("ku-Latn"), "ltr");
    }
}
