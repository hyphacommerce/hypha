//! Values: `FieldValue::parse` is the one place a value is checked against its definition's
//! type and validations, reporting every problem with its path (`[3]` for a list's fourth
//! item; commands prefix the key, `custom.sizes[3]`).

use std::collections::HashSet;

use hypha_core::{Currency, Description, Money, Timestamp};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    decimal::Decimal,
    limits::{MAX_LIST_ITEMS, MAX_LONG_BYTES, MAX_TEXT_CHARS, MAX_URL_CHARS, MAX_VALUE_BYTES},
    types::{
        BaseType, Bound, FieldType, Measurement, Reference, ReferenceKind, URL_SCHEMES, Unit,
        Validations,
    },
};

/// The problem codes a value can have, stable like error codes.
pub mod codes {
    /// The JSON is the wrong kind (a number for a text field).
    pub const INVALID_TYPE: &str = "invalid_type";
    /// An empty string: remove the value with `null` instead.
    pub const BLANK: &str = "blank";
    pub const NOT_SINGLE_LINE: &str = "not_single_line";
    pub const TOO_SHORT: &str = "too_short";
    pub const TOO_LONG: &str = "too_long";
    pub const PATTERN_MISMATCH: &str = "pattern_mismatch";
    pub const BELOW_MIN: &str = "below_min";
    pub const ABOVE_MAX: &str = "above_max";
    pub const INVALID_NUMBER: &str = "invalid_number";
    pub const TOO_MANY_DECIMALS: &str = "too_many_decimals";
    pub const INVALID_DATE: &str = "invalid_date";
    pub const INVALID_DATE_TIME: &str = "invalid_date_time";
    pub const INVALID_URL: &str = "invalid_url";
    pub const SCHEME_NOT_ALLOWED: &str = "scheme_not_allowed";
    pub const DOMAIN_NOT_ALLOWED: &str = "domain_not_allowed";
    pub const INVALID_MONEY: &str = "invalid_money";
    pub const CURRENCY_NOT_ALLOWED: &str = "currency_not_allowed";
    pub const INVALID_MEASUREMENT: &str = "invalid_measurement";
    pub const UNIT_NOT_ALLOWED: &str = "unit_not_allowed";
    pub const UNKNOWN_CHOICE: &str = "unknown_choice";
    pub const INVALID_REFERENCE: &str = "invalid_reference";
    /// The reference's target doesn't exist (checked by the commands).
    pub const REFERENCE_NOT_FOUND: &str = "reference_not_found";
    /// Over a size limit (JSON, or any value serialised).
    pub const TOO_LARGE: &str = "too_large";
    pub const TOO_FEW_ITEMS: &str = "too_few_items";
    pub const TOO_MANY_ITEMS: &str = "too_many_items";
    pub const DUPLICATE_ITEM: &str = "duplicate_item";
}

/// One thing wrong with a value. `path` is relative to the field until a command
/// prefixes the key (`custom.sizes[3]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValueProblem {
    pub path: String,
    pub code: &'static str,
    pub message: String,
}

impl ValueProblem {
    pub fn new(path: impl Into<String>, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            code,
            message: message.into(),
        }
    }

    /// The same problem under `key`: `custom.sizes` + `[3]`.
    pub fn under(mut self, key: &str) -> Self {
        self.path = format!("{key}{}", self.path);
        self
    }
}

/// A value, typed by its definition.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Text(String),
    MultilineText(String),
    /// Sanitised HTML.
    RichText(String),
    Integer(i64),
    Decimal(Decimal),
    Boolean(bool),
    Date(jiff::civil::Date),
    DateTime(Timestamp),
    Url(String),
    Money(Money),
    Measurement(Measurement),
    /// The choice's key.
    Choice(String),
    Reference(Reference),
    Json(Value),
    List(Vec<FieldValue>),
}

/// How strictly a value is read: as sent (every validation, rich text sanitised), or as
/// stored (only its type: stored values passed their definition when written).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Input,
    Stored,
}

/// `"2026-10-20"`, exactly that shape.
pub(crate) fn parse_date(text: &str) -> Option<jiff::civil::Date> {
    let b = text.as_bytes();
    let shaped = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !shaped {
        return None;
    }
    text.parse().ok()
}

/// An RFC 3339 date and time with an offset (`2026-10-20T10:00:00+02:00`), as UTC.
pub(crate) fn parse_date_time(text: &str) -> Option<Timestamp> {
    if text.len() > 64 || !text.contains('T') {
        return None;
    }
    let parsed: jiff::Timestamp = text.parse().ok()?;
    Some(Timestamp::from_unix_micros(parsed.as_microsecond()))
}

/// A timestamp as RFC 3339 in UTC, `2026-10-20T08:00:00Z`.
pub(crate) fn format_date_time(at: Timestamp) -> String {
    jiff::Timestamp::from_microsecond(at.unix_micros())
        .map(|t| t.to_string())
        .unwrap_or_default()
}

/// Midnight UTC of `date`, in microseconds since the epoch: a date's index form.
pub(crate) fn date_micros(date: jiff::civil::Date) -> i64 {
    date.to_zoned(jiff::tz::TimeZone::UTC)
        .map(|z| z.timestamp().as_microsecond())
        .unwrap_or(0)
}

/// `{"value": "1.5", "unit": "kg"}`, exactly those keys.
pub(crate) fn parse_measurement(json: &Value) -> Option<Measurement> {
    let object = json.as_object()?;
    if object.len() != 2 {
        return None;
    }
    let value = Decimal::parse(object.get("value")?.as_str()?).ok()?;
    let unit = Unit::parse(object.get("unit")?.as_str()?)?;
    Some(Measurement { value, unit })
}

pub(crate) fn measurement_json(m: Measurement) -> Value {
    json!({"value": m.value.to_string(), "unit": m.unit.as_str()})
}

/// What a JSON value is, for messages.
fn kind_of(json: &Value) -> &'static str {
    match json {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

struct Parser<'a> {
    validations: &'a Validations,
    mode: Mode,
    problems: Vec<ValueProblem>,
}

impl Parser<'_> {
    fn problem(&mut self, path: &str, code: &'static str, message: impl Into<String>) {
        self.problems.push(ValueProblem::new(path, code, message));
    }

    fn checking(&self) -> bool {
        self.mode == Mode::Input
    }

    fn wrong_type(&mut self, path: &str, expected: &str, json: &Value) {
        self.problem(
            path,
            codes::INVALID_TYPE,
            format!("expected {expected}, got {}", kind_of(json)),
        );
    }

    /// `min` / `max` against a value's bound form.
    fn range(&mut self, path: &str, value: Bound, shown: &str) {
        if !self.checking() {
            return;
        }
        if let Some(min) = self.validations.min
            && value.cmp_value(&min).is_lt()
        {
            self.problem(
                path,
                codes::BELOW_MIN,
                format!("{shown} is below the minimum, {}", show_bound(&min)),
            );
        }
        if let Some(max) = self.validations.max
            && value.cmp_value(&max).is_gt()
        {
            self.problem(
                path,
                codes::ABOVE_MAX,
                format!("{shown} is above the maximum, {}", show_bound(&max)),
            );
        }
    }

    /// Character length against `min_length` / `max_length` and the type's own cap.
    fn length(
        &mut self,
        path: &str,
        text: &str,
        cap_chars: Option<usize>,
        cap_bytes: Option<usize>,
    ) {
        let chars = text.chars().count();
        if let Some(cap) = cap_chars
            && chars > cap
        {
            self.problem(
                path,
                codes::TOO_LONG,
                format!("is longer than {cap} characters"),
            );
            return;
        }
        if let Some(cap) = cap_bytes
            && text.len() > cap
        {
            self.problem(path, codes::TOO_LONG, format!("is longer than {cap} bytes"));
            return;
        }
        if !self.checking() {
            return;
        }
        if let Some(max) = self.validations.max_length
            && chars > max as usize
        {
            self.problem(
                path,
                codes::TOO_LONG,
                format!("is longer than {max} characters"),
            );
        }
        if let Some(min) = self.validations.min_length
            && chars < min as usize
        {
            self.problem(
                path,
                codes::TOO_SHORT,
                format!("is shorter than {min} characters"),
            );
        }
    }

    fn string<'j>(&mut self, path: &str, json: &'j Value) -> Option<&'j str> {
        match json.as_str() {
            Some("") => {
                self.problem(
                    path,
                    codes::BLANK,
                    "is empty: send null to remove the value",
                );
                None
            }
            Some(s) => Some(s),
            None => {
                self.wrong_type(path, "a string", json);
                None
            }
        }
    }

    fn one(&mut self, base: BaseType, path: &str, json: &Value) -> Option<FieldValue> {
        match base {
            BaseType::Text => {
                let text = self.string(path, json)?;
                if text.contains(['\n', '\r']) {
                    self.problem(
                        path,
                        codes::NOT_SINGLE_LINE,
                        "has a line break: use multiline_text",
                    );
                    return None;
                }
                let before = self.problems.len();
                self.length(path, text, Some(MAX_TEXT_CHARS), None);
                if self.checking()
                    && let Some(pattern) = &self.validations.pattern
                    && !pattern.matches(text)
                {
                    self.problem(
                        path,
                        codes::PATTERN_MISMATCH,
                        format!("{text:?} doesn't match the pattern {:?}", pattern.as_str()),
                    );
                }
                (self.problems.len() == before).then(|| FieldValue::Text(text.to_string()))
            }
            BaseType::MultilineText => {
                let text = self.string(path, json)?;
                let before = self.problems.len();
                self.length(path, text, None, Some(MAX_LONG_BYTES));
                (self.problems.len() == before).then(|| FieldValue::MultilineText(text.to_string()))
            }
            BaseType::RichText => {
                let html = self.string(path, json)?;
                if html.len() > MAX_LONG_BYTES {
                    self.problem(
                        path,
                        codes::TOO_LONG,
                        format!("is longer than {MAX_LONG_BYTES} bytes"),
                    );
                    return None;
                }
                let clean = if self.checking() {
                    match Description::new(html) {
                        Ok(Some(d)) => d.as_html().to_string(),
                        Ok(None) => {
                            self.problem(path, codes::BLANK, "has no text once sanitised");
                            return None;
                        }
                        Err(_) => {
                            self.problem(
                                path,
                                codes::TOO_LONG,
                                format!("is longer than {MAX_LONG_BYTES} bytes"),
                            );
                            return None;
                        }
                    }
                } else {
                    html.to_string()
                };
                let before = self.problems.len();
                self.length(path, &clean, None, Some(MAX_LONG_BYTES));
                (self.problems.len() == before).then_some(FieldValue::RichText(clean))
            }
            BaseType::Integer => {
                let Some(n) = json.as_i64().filter(|_| json.is_i64() || json.is_u64()) else {
                    if json.is_number() {
                        self.problem(
                            path,
                            codes::INVALID_NUMBER,
                            format!("{json} is not a whole number"),
                        );
                    } else {
                        self.wrong_type(path, "a whole number", json);
                    }
                    return None;
                };
                if n.unsigned_abs() > Money::MAX_AMOUNT as u64 {
                    self.problem(
                        path,
                        codes::INVALID_NUMBER,
                        format!("{n} is beyond ±(2^53 − 1)"),
                    );
                    return None;
                }
                let before = self.problems.len();
                self.range(path, Bound::Integer(n), &n.to_string());
                (self.problems.len() == before).then_some(FieldValue::Integer(n))
            }
            BaseType::Decimal => {
                let Some(text) = json.as_str() else {
                    self.wrong_type(path, "a decimal string like \"12.50\"", json);
                    return None;
                };
                let d = match Decimal::parse(text) {
                    Ok(d) => d,
                    Err(e) => {
                        self.problem(path, codes::INVALID_NUMBER, e.to_string());
                        return None;
                    }
                };
                let before = self.problems.len();
                if self.checking()
                    && let Some(max_scale) = self.validations.max_scale
                    && d.normalized().scale() > max_scale
                {
                    self.problem(
                        path,
                        codes::TOO_MANY_DECIMALS,
                        format!("{text:?} has more than {max_scale} digits after the point"),
                    );
                }
                self.range(path, Bound::Decimal(d), text);
                (self.problems.len() == before).then_some(FieldValue::Decimal(d))
            }
            BaseType::Boolean => match json.as_bool() {
                Some(b) => Some(FieldValue::Boolean(b)),
                None => {
                    self.wrong_type(path, "true or false", json);
                    None
                }
            },
            BaseType::Date => {
                let Some(text) = json.as_str() else {
                    self.wrong_type(path, "a date like \"2026-10-20\"", json);
                    return None;
                };
                let Some(date) = parse_date(text) else {
                    self.problem(
                        path,
                        codes::INVALID_DATE,
                        format!("{text:?} is not a date like \"2026-10-20\""),
                    );
                    return None;
                };
                let before = self.problems.len();
                self.range(path, Bound::Date(date), text);
                (self.problems.len() == before).then_some(FieldValue::Date(date))
            }
            BaseType::DateTime => {
                let Some(text) = json.as_str() else {
                    self.wrong_type(path, "an RFC 3339 date and time", json);
                    return None;
                };
                let Some(at) = parse_date_time(text) else {
                    self.problem(
                        path,
                        codes::INVALID_DATE_TIME,
                        format!("{text:?} is not an RFC 3339 date and time like \"2026-10-20T10:00:00Z\""),
                    );
                    return None;
                };
                let before = self.problems.len();
                self.range(path, Bound::DateTime(at), text);
                (self.problems.len() == before).then_some(FieldValue::DateTime(at))
            }
            BaseType::Url => {
                let text = self.string(path, json)?;
                if text.chars().count() > MAX_URL_CHARS {
                    self.problem(
                        path,
                        codes::TOO_LONG,
                        format!("is longer than {MAX_URL_CHARS} characters"),
                    );
                    return None;
                }
                let Ok(url) = url::Url::parse(text) else {
                    self.problem(path, codes::INVALID_URL, format!("{text:?} is not a URL"));
                    return None;
                };
                if !URL_SCHEMES.contains(&url.scheme()) {
                    self.problem(
                        path,
                        codes::SCHEME_NOT_ALLOWED,
                        format!("{text:?} isn't https, http, mailto or tel"),
                    );
                    return None;
                }
                let before = self.problems.len();
                if self.checking() {
                    if let Some(schemes) = &self.validations.schemes
                        && !schemes.contains(url.scheme())
                    {
                        self.problem(
                            path,
                            codes::SCHEME_NOT_ALLOWED,
                            format!("{text:?} isn't one of the schemes allowed"),
                        );
                    }
                    if let Some(domains) = &self.validations.domains {
                        let host = url.host_str().unwrap_or("").to_lowercase();
                        let allowed = domains
                            .iter()
                            .any(|d| host == *d || host.ends_with(&format!(".{d}")));
                        if !allowed {
                            self.problem(
                                path,
                                codes::DOMAIN_NOT_ALLOWED,
                                format!("{text:?} isn't on one of the domains allowed"),
                            );
                        }
                    }
                }
                (self.problems.len() == before).then(|| FieldValue::Url(text.to_string()))
            }
            BaseType::Money => {
                let parsed = json.as_object().filter(|o| o.len() == 2).and_then(|o| {
                    let amount = o.get("amount")?;
                    let amount = amount
                        .as_i64()
                        .filter(|_| amount.is_i64() || amount.is_u64())?;
                    let currency = Currency::new(o.get("currency")?.as_str()?).ok()?;
                    Money::new(amount, currency).ok()
                });
                let Some(money) = parsed else {
                    self.problem(
                        path,
                        codes::INVALID_MONEY,
                        "expected {\"amount\": minor units within ±(2^53 − 1), \"currency\": \"EUR\"}",
                    );
                    return None;
                };
                let before = self.problems.len();
                if self.checking()
                    && let Some(currencies) = &self.validations.currencies
                    && !currencies.contains(money.currency().as_str())
                {
                    self.problem(
                        path,
                        codes::CURRENCY_NOT_ALLOWED,
                        format!("{} isn't one of the currencies allowed", money.currency()),
                    );
                }
                self.range(
                    path,
                    Bound::Integer(money.amount()),
                    &format!("{} {}", money.amount(), money.currency()),
                );
                (self.problems.len() == before).then_some(FieldValue::Money(money))
            }
            BaseType::Measurement => {
                let Some(m) = parse_measurement(json) else {
                    self.problem(
                        path,
                        codes::INVALID_MEASUREMENT,
                        "expected {\"value\": \"1.5\", \"unit\": \"kg\"} with a unit of g, kg, oz, lb, mm, cm, m, in, ft, ml, l or fl_oz",
                    );
                    return None;
                };
                let before = self.problems.len();
                if let Some(dimension) = self.validations.dimension
                    && m.unit.dimension() != dimension
                {
                    self.problem(
                        path,
                        codes::UNIT_NOT_ALLOWED,
                        format!("{} isn't a unit of {}", m.unit.as_str(), dimension.as_str()),
                    );
                    return None;
                }
                self.range(
                    path,
                    Bound::Measurement(m),
                    &format!("{} {}", m.value, m.unit.as_str()),
                );
                (self.problems.len() == before).then_some(FieldValue::Measurement(m))
            }
            BaseType::Choice => {
                let key = self.string(path, json)?;
                if self.checking() && !self.validations.choices.iter().any(|c| c.key == key) {
                    self.problem(
                        path,
                        codes::UNKNOWN_CHOICE,
                        format!("{key:?} isn't one of the choices"),
                    );
                    return None;
                }
                Some(FieldValue::Choice(key.to_string()))
            }
            BaseType::Reference => {
                let text = self.string(path, json)?;
                let found = match self.validations.kind {
                    Some(kind) => kind.parse_id(text).map(|id| Reference { kind, id }),
                    // Stored without the definition's kind at hand: any kind's prefix.
                    None => ReferenceKind::ALL
                        .into_iter()
                        .find_map(|kind| kind.parse_id(text).map(|id| Reference { kind, id })),
                };
                match found {
                    Some(r) => Some(FieldValue::Reference(r)),
                    None => {
                        let expected = self
                            .validations
                            .kind
                            .map_or("an ID".to_string(), |k| format!("a {} ID", k.as_str()));
                        self.problem(
                            path,
                            codes::INVALID_REFERENCE,
                            format!("{text:?} is not {expected}"),
                        );
                        None
                    }
                }
            }
            BaseType::Json => {
                let size = serde_json::to_string(json).map_or(usize::MAX, |s| s.len());
                if size > MAX_LONG_BYTES {
                    self.problem(
                        path,
                        codes::TOO_LARGE,
                        format!("is larger than {MAX_LONG_BYTES} bytes"),
                    );
                    return None;
                }
                Some(FieldValue::Json(json.clone()))
            }
        }
    }

    fn list(&mut self, base: BaseType, json: &Value) -> Option<FieldValue> {
        let Some(items) = json.as_array() else {
            self.wrong_type("", "a list", json);
            return None;
        };
        let v = self.validations;
        let cap = v.max_items.map_or(MAX_LIST_ITEMS, |m| m as usize);
        if items.len() > MAX_LIST_ITEMS || (self.checking() && items.len() > cap) {
            self.problem(
                "",
                codes::TOO_MANY_ITEMS,
                format!("has {} items; at most {cap}", items.len()),
            );
            return None;
        }
        let before = self.problems.len();
        if self.checking()
            && let Some(min) = v.min_items
            && items.len() < min as usize
        {
            self.problem(
                "",
                codes::TOO_FEW_ITEMS,
                format!("has {} items; at least {min}", items.len()),
            );
        }
        let mut values = Vec::with_capacity(items.len());
        let mut seen = HashSet::new();
        for (i, item) in items.iter().enumerate() {
            let path = format!("[{i}]");
            if let Some(value) = self.one(base, &path, item) {
                if self.checking() && v.unique_items && !seen.insert(value.item_key(true)) {
                    self.problem(&path, codes::DUPLICATE_ITEM, "is already in the list");
                }
                values.push(value);
            }
        }
        (self.problems.len() == before).then_some(FieldValue::List(values))
    }
}

fn show_bound(bound: &Bound) -> String {
    match bound {
        Bound::Integer(n) => n.to_string(),
        Bound::Decimal(d) => d.to_string(),
        Bound::Date(d) => d.to_string(),
        Bound::DateTime(t) => format_date_time(*t),
        Bound::Measurement(m) => format!("{} {}", m.value, m.unit.as_str()),
    }
}

impl FieldValue {
    /// Checks `json` against the field's type and validations, as sent by a caller.
    /// Every problem is reported, each with its path relative to the field (`""`, or
    /// `[3]` for a list's fourth item).
    pub fn parse(
        field_type: FieldType,
        validations: &Validations,
        json: &Value,
    ) -> Result<FieldValue, Vec<ValueProblem>> {
        Self::read(field_type, validations, json, Mode::Input)
    }

    /// Reads a stored value by its type alone (stored values passed their definition when
    /// written; validations may have changed since only in ways they still pass). Rich text
    /// isn't sanitised again.
    pub fn from_stored(
        field_type: FieldType,
        json: &Value,
    ) -> Result<FieldValue, Vec<ValueProblem>> {
        Self::read(field_type, &Validations::none(), json, Mode::Stored)
    }

    fn read(
        field_type: FieldType,
        validations: &Validations,
        json: &Value,
        mode: Mode,
    ) -> Result<FieldValue, Vec<ValueProblem>> {
        let mut parser = Parser {
            validations,
            mode,
            problems: Vec::new(),
        };
        let parsed = if field_type.is_list() {
            parser.list(field_type.base(), json)
        } else {
            parser.one(field_type.base(), "", json)
        };
        if let Some(value) = &parsed
            && mode == Mode::Input
        {
            let size = value.to_json().to_string().len();
            if size > MAX_VALUE_BYTES {
                parser.problem(
                    "",
                    codes::TOO_LARGE,
                    format!("is larger than {MAX_VALUE_BYTES} bytes"),
                );
            }
        }
        match parsed {
            Some(value) if parser.problems.is_empty() => Ok(value),
            _ => Err(parser.problems),
        }
    }

    /// The value as the API shows it and storage keeps it.
    pub fn to_json(&self) -> Value {
        match self {
            FieldValue::Text(s)
            | FieldValue::MultilineText(s)
            | FieldValue::RichText(s)
            | FieldValue::Url(s)
            | FieldValue::Choice(s) => json!(s),
            FieldValue::Integer(n) => json!(n),
            FieldValue::Decimal(d) => json!(d.to_string()),
            FieldValue::Boolean(b) => json!(b),
            FieldValue::Date(d) => json!(d.to_string()),
            FieldValue::DateTime(t) => json!(format_date_time(*t)),
            FieldValue::Money(m) => {
                json!({"amount": m.amount(), "currency": m.currency().as_str()})
            }
            FieldValue::Measurement(m) => measurement_json(*m),
            FieldValue::Reference(r) => json!(r.to_string()),
            FieldValue::Json(v) => v.clone(),
            FieldValue::List(items) => {
                Value::Array(items.iter().map(FieldValue::to_json).collect())
            }
        }
    }

    /// The `scalar_text` index column: text, choice, url and reference lowercased, booleans
    /// as `"true"`/`"false"`, integers as their digits (so `unique` integers share text's
    /// unique index). `None` for lists and other types.
    pub fn scalar_text(&self) -> Option<String> {
        match self {
            FieldValue::Text(s) | FieldValue::Url(s) | FieldValue::Choice(s) => {
                Some(s.to_lowercase())
            }
            FieldValue::Reference(r) => Some(r.to_string()),
            FieldValue::Boolean(b) => Some(b.to_string()),
            FieldValue::Integer(n) => Some(n.to_string()),
            _ => None,
        }
    }

    /// The `scalar_num` index column, exact: integers, decimals, money in minor units,
    /// measurements in their dimension's base unit (g, mm, ml), dates (midnight UTC) and
    /// times as microseconds since the epoch. `None` for lists and other types.
    pub fn scalar_num(&self) -> Option<Decimal> {
        match self {
            FieldValue::Integer(n) => Some(Decimal::from_i64(*n)),
            FieldValue::Decimal(d) => Some(d.normalized()),
            FieldValue::Money(m) => Some(Decimal::from_i64(m.amount())),
            FieldValue::Measurement(m) => m.in_base().map(Decimal::normalized),
            FieldValue::Date(d) => Some(Decimal::from_i64(date_micros(*d))),
            FieldValue::DateTime(t) => Some(Decimal::from_i64(t.unix_micros())),
            _ => None,
        }
    }

    /// What `unique` compares: the lowercased text, URL or the integer's digits.
    pub fn unique_key(&self) -> Option<String> {
        match self {
            FieldValue::Text(_) | FieldValue::Url(_) | FieldValue::Integer(_) => self.scalar_text(),
            _ => None,
        }
    }

    /// A list's items; `None` for a single value.
    pub fn items(&self) -> Option<&[FieldValue]> {
        match self {
            FieldValue::List(items) => Some(items),
            _ => None,
        }
    }

    /// What list operations compare an item by: its JSON, with text lowercased when
    /// `case_insensitive` (lists with `unique_items`).
    pub fn item_key(&self, case_insensitive: bool) -> String {
        match self {
            FieldValue::Text(s) if case_insensitive => format!("\"{}\"", s.to_lowercase()),
            other => other.to_json().to_string(),
        }
    }

    /// The references it holds: itself, or a list's items.
    pub fn references(&self) -> Vec<Reference> {
        match self {
            FieldValue::Reference(r) => vec![*r],
            FieldValue::List(items) => items.iter().flat_map(FieldValue::references).collect(),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{BaseType, RawValidations};

    use super::*;

    fn ty(name: &str) -> FieldType {
        FieldType::parse(name).unwrap()
    }

    fn rules(name: &str, raw: Value) -> Validations {
        let raw: RawValidations = serde_json::from_value(raw).unwrap();
        Validations::new(ty(name), raw).unwrap()
    }

    fn parse(name: &str, raw: Value, value: Value) -> Result<FieldValue, Vec<ValueProblem>> {
        FieldValue::parse(ty(name), &rules(name, raw), &value)
    }

    fn problem(path: &str, code: &'static str, message: &str) -> Vec<ValueProblem> {
        vec![ValueProblem::new(path, code, message)]
    }

    #[test]
    fn text_is_one_line() {
        assert_eq!(
            parse("text", json!({}), json!("a\nb")),
            Err(problem(
                "",
                codes::NOT_SINGLE_LINE,
                "has a line break: use multiline_text"
            ))
        );
    }

    #[test]
    fn text_is_at_most_255_characters() {
        assert!(parse("text", json!({}), json!("é".repeat(255))).is_ok());
        assert_eq!(
            parse("text", json!({}), json!("é".repeat(256))),
            Err(problem(
                "",
                codes::TOO_LONG,
                "is longer than 255 characters"
            ))
        );
    }

    #[test]
    fn an_empty_string_is_blank() {
        assert_eq!(
            parse("text", json!({}), json!("")),
            Err(problem(
                "",
                codes::BLANK,
                "is empty: send null to remove the value"
            ))
        );
    }

    #[test]
    fn text_respects_min_and_max_length() {
        let raw = json!({"min_length": 2, "max_length": 3});
        assert!(parse("text", raw.clone(), json!("ab")).is_ok());
        assert!(parse("text", raw.clone(), json!("abc")).is_ok());
        assert_eq!(
            parse("text", raw.clone(), json!("a")),
            Err(problem(
                "",
                codes::TOO_SHORT,
                "is shorter than 2 characters"
            ))
        );
        assert_eq!(
            parse("text", raw, json!("abcd")),
            Err(problem("", codes::TOO_LONG, "is longer than 3 characters"))
        );
    }

    #[test]
    fn a_pattern_must_match_the_whole_text() {
        let raw = json!({"pattern": "[A-Z]-\\d+"});
        assert!(parse("text", raw.clone(), json!("A-1001")).is_ok());
        assert_eq!(
            parse("text", raw, json!("xA-1001")),
            Err(problem(
                "",
                codes::PATTERN_MISMATCH,
                "\"xA-1001\" doesn't match the pattern \"[A-Z]-\\\\d+\""
            ))
        );
    }

    #[test]
    fn multiline_text_is_at_most_16_kib() {
        assert!(parse("multiline_text", json!({}), json!("a\n".repeat(8192))).is_ok());
        assert_eq!(
            parse(
                "multiline_text",
                json!({}),
                json!("a".repeat(16 * 1024 + 1))
            ),
            Err(problem("", codes::TOO_LONG, "is longer than 16384 bytes"))
        );
    }

    #[test]
    fn rich_text_is_sanitised() {
        assert_eq!(
            parse(
                "rich_text",
                json!({}),
                json!("<p onclick=\"x()\">Hi</p><script>x()</script>")
            ),
            Ok(FieldValue::RichText("<p>Hi</p>".to_string()))
        );
    }

    #[test]
    fn rich_text_with_nothing_left_is_blank() {
        assert_eq!(
            parse("rich_text", json!({}), json!("<script>x()</script>")),
            Err(problem("", codes::BLANK, "has no text once sanitised"))
        );
    }

    #[test]
    fn integers_are_whole_numbers() {
        assert_eq!(
            parse("integer", json!({}), json!(1.5)),
            Err(problem(
                "",
                codes::INVALID_NUMBER,
                "1.5 is not a whole number"
            ))
        );
        assert_eq!(
            parse("integer", json!({}), json!("1")),
            Err(problem(
                "",
                codes::INVALID_TYPE,
                "expected a whole number, got a string"
            ))
        );
    }

    #[test]
    fn integers_stay_within_two_to_the_53() {
        let max = Money::MAX_AMOUNT;
        assert_eq!(
            parse("integer", json!({}), json!(max)),
            Ok(FieldValue::Integer(max))
        );
        assert_eq!(
            parse("integer", json!({}), json!(-max)),
            Ok(FieldValue::Integer(-max))
        );
        assert_eq!(
            parse("integer", json!({}), json!(max + 1)),
            Err(problem(
                "",
                codes::INVALID_NUMBER,
                "9007199254740992 is beyond ±(2^53 − 1)"
            ))
        );
    }

    #[test]
    fn integer_bounds_are_inclusive() {
        let raw = json!({"min": 0, "max": 10});
        assert!(parse("integer", raw.clone(), json!(0)).is_ok());
        assert!(parse("integer", raw.clone(), json!(10)).is_ok());
        assert_eq!(
            parse("integer", raw.clone(), json!(-1)),
            Err(problem("", codes::BELOW_MIN, "-1 is below the minimum, 0"))
        );
        assert_eq!(
            parse("integer", raw, json!(11)),
            Err(problem("", codes::ABOVE_MAX, "11 is above the maximum, 10"))
        );
    }

    #[test]
    fn decimals_are_strings_kept_as_written() {
        let value = parse("decimal", json!({}), json!("12.50")).unwrap();
        assert_eq!(value.to_json(), json!("12.50"));
        assert_eq!(
            value.scalar_num().map(|d| d.to_string()),
            Some("12.5".to_string())
        );
        assert_eq!(
            parse("decimal", json!({}), json!(12.5)),
            Err(problem(
                "",
                codes::INVALID_TYPE,
                "expected a decimal string like \"12.50\", got a number"
            ))
        );
    }

    #[test]
    fn max_scale_counts_digits_that_matter() {
        let raw = json!({"max_scale": 1});
        assert!(parse("decimal", raw.clone(), json!("12.50")).is_ok());
        assert_eq!(
            parse("decimal", raw, json!("12.05")),
            Err(problem(
                "",
                codes::TOO_MANY_DECIMALS,
                "\"12.05\" has more than 1 digits after the point"
            ))
        );
    }

    #[test]
    fn decimal_bounds_compare_exactly() {
        let raw = json!({"min": "0.1"});
        assert!(parse("decimal", raw.clone(), json!("0.10")).is_ok());
        assert_eq!(
            parse("decimal", raw, json!("0.0999")),
            Err(problem(
                "",
                codes::BELOW_MIN,
                "0.0999 is below the minimum, 0.1"
            ))
        );
    }

    #[test]
    fn booleans_are_true_or_false() {
        assert_eq!(
            parse("boolean", json!({}), json!(true)),
            Ok(FieldValue::Boolean(true))
        );
        assert_eq!(
            parse("boolean", json!({}), json!("true")),
            Err(problem(
                "",
                codes::INVALID_TYPE,
                "expected true or false, got a string"
            ))
        );
    }

    #[test]
    fn dates_are_year_month_day() {
        let value = parse("date", json!({}), json!("2026-10-20")).unwrap();
        assert_eq!(value.to_json(), json!("2026-10-20"));
        assert_eq!(
            value.scalar_num().map(|d| d.to_string()),
            Some("1792454400000000".to_string())
        );
        for bad in [
            "2026-02-30",
            "2026-1-20",
            "20261020",
            "2026-10-20T00:00:00Z",
        ] {
            assert_eq!(
                parse("date", json!({}), json!(bad)),
                Err(problem(
                    "",
                    codes::INVALID_DATE,
                    &format!("{bad:?} is not a date like \"2026-10-20\"")
                ))
            );
        }
    }

    #[test]
    fn date_times_are_stored_in_utc() {
        let value = parse("date_time", json!({}), json!("2026-10-20T10:00:00+02:00")).unwrap();
        assert_eq!(value.to_json(), json!("2026-10-20T08:00:00Z"));
    }

    #[test]
    fn a_date_time_needs_an_offset() {
        assert_eq!(
            parse("date_time", json!({}), json!("2026-10-20T10:00:00")),
            Err(problem(
                "",
                codes::INVALID_DATE_TIME,
                "\"2026-10-20T10:00:00\" is not an RFC 3339 date and time like \"2026-10-20T10:00:00Z\""
            ))
        );
    }

    #[test]
    fn date_bounds() {
        let raw = json!({"min": "2026-01-01"});
        assert!(parse("date", raw.clone(), json!("2026-01-01")).is_ok());
        assert_eq!(
            parse("date", raw, json!("2025-12-31")),
            Err(problem(
                "",
                codes::BELOW_MIN,
                "2025-12-31 is below the minimum, 2026-01-01"
            ))
        );
    }

    #[test]
    fn urls_have_a_known_scheme() {
        assert!(parse("url", json!({}), json!("mailto:a@example.com")).is_ok());
        assert_eq!(
            parse("url", json!({}), json!("javascript:alert(1)")),
            Err(problem(
                "",
                codes::SCHEME_NOT_ALLOWED,
                "\"javascript:alert(1)\" isn't https, http, mailto or tel"
            ))
        );
    }

    #[test]
    fn urls_may_be_held_to_schemes_and_domains() {
        let raw = json!({"schemes": ["https"], "domains": ["example.com"]});
        assert!(parse("url", raw.clone(), json!("https://cdn.example.com/a")).is_ok());
        assert_eq!(
            parse("url", raw.clone(), json!("http://example.com")),
            Err(problem(
                "",
                codes::SCHEME_NOT_ALLOWED,
                "\"http://example.com\" isn't one of the schemes allowed"
            ))
        );
        assert_eq!(
            parse("url", raw, json!("https://badexample.com")),
            Err(problem(
                "",
                codes::DOMAIN_NOT_ALLOWED,
                "\"https://badexample.com\" isn't on one of the domains allowed"
            ))
        );
    }

    #[test]
    fn urls_are_at_most_2048_characters() {
        let long = format!("https://example.com/{}", "a".repeat(2048));
        assert_eq!(
            parse("url", json!({}), json!(long)),
            Err(problem(
                "",
                codes::TOO_LONG,
                "is longer than 2048 characters"
            ))
        );
    }

    #[test]
    fn money_is_minor_units_and_a_currency() {
        let value = parse(
            "money",
            json!({}),
            json!({"amount": 1999, "currency": "EUR"}),
        )
        .unwrap();
        assert_eq!(value.to_json(), json!({"amount": 1999, "currency": "EUR"}));
        assert_eq!(value.scalar_num(), Some(Decimal::from_i64(1999)));
        assert_eq!(
            parse(
                "money",
                json!({}),
                json!({"amount": "19.99", "currency": "EUR"})
            )
            .unwrap_err()[0]
                .code,
            codes::INVALID_MONEY
        );
    }

    #[test]
    fn money_may_be_held_to_currencies_and_bounds() {
        let raw = json!({"currencies": ["EUR"], "min": 0});
        assert_eq!(
            parse("money", raw, json!({"amount": -1, "currency": "USD"})),
            Err(vec![
                ValueProblem::new(
                    "",
                    codes::CURRENCY_NOT_ALLOWED,
                    "USD isn't one of the currencies allowed"
                ),
                ValueProblem::new("", codes::BELOW_MIN, "-1 USD is below the minimum, 0"),
            ])
        );
    }

    #[test]
    fn measurements_index_in_base_units_exactly() {
        let raw = json!({"dimension": "weight"});
        let value = parse("measurement", raw, json!({"value": "1.5", "unit": "lb"})).unwrap();
        assert_eq!(
            value.scalar_num().map(|d| d.to_string()),
            Some("680.388555".to_string())
        );
    }

    #[test]
    fn a_measurement_is_in_its_dimension() {
        assert_eq!(
            parse(
                "measurement",
                json!({"dimension": "weight"}),
                json!({"value": "1", "unit": "cm"})
            ),
            Err(problem(
                "",
                codes::UNIT_NOT_ALLOWED,
                "cm isn't a unit of weight"
            ))
        );
    }

    #[test]
    fn measurement_bounds_convert_units() {
        let raw = json!({"dimension": "weight", "max": {"value": "1", "unit": "kg"}});
        assert!(
            parse(
                "measurement",
                raw.clone(),
                json!({"value": "1000", "unit": "g"})
            )
            .is_ok()
        );
        assert_eq!(
            parse("measurement", raw, json!({"value": "2.21", "unit": "lb"})),
            Err(problem(
                "",
                codes::ABOVE_MAX,
                "2.21 lb is above the maximum, 1 kg"
            ))
        );
    }

    #[test]
    fn choices_store_keys() {
        let raw = json!({"choices": [{"key": "dry", "label": "Dry skin"}]});
        assert_eq!(
            parse("choice", raw.clone(), json!("dry")),
            Ok(FieldValue::Choice("dry".to_string()))
        );
        assert_eq!(
            parse("choice", raw, json!("Dry skin")),
            Err(problem(
                "",
                codes::UNKNOWN_CHOICE,
                "\"Dry skin\" isn't one of the choices"
            ))
        );
    }

    #[test]
    fn references_are_ids_of_their_kind() {
        let id = hypha_core::ProductId::from_uuid(uuid::Uuid::from_u128(7)).to_string();
        let raw = json!({"kind": "product"});
        assert!(parse("reference", raw.clone(), json!(id)).is_ok());
        let variant = hypha_core::VariantId::from_uuid(uuid::Uuid::from_u128(7)).to_string();
        assert_eq!(
            parse("reference", raw, json!(variant)),
            Err(problem(
                "",
                codes::INVALID_REFERENCE,
                &format!("{variant:?} is not a product ID")
            ))
        );
    }

    #[test]
    fn json_is_at_most_16_kib() {
        assert!(parse("json", json!({}), json!({"a": [1, 2]})).is_ok());
        assert_eq!(
            parse("json", json!({}), json!("a".repeat(16 * 1024))),
            Err(problem("", codes::TOO_LARGE, "is larger than 16384 bytes"))
        );
    }

    #[test]
    fn list_problems_name_their_items() {
        assert_eq!(
            parse("list.integer", json!({"max": 5}), json!([1, 9, "x"])),
            Err(vec![
                ValueProblem::new("[1]", codes::ABOVE_MAX, "9 is above the maximum, 5"),
                ValueProblem::new(
                    "[2]",
                    codes::INVALID_TYPE,
                    "expected a whole number, got a string"
                ),
            ])
        );
    }

    #[test]
    fn lists_respect_min_and_max_items() {
        let raw = json!({"min_items": 2, "max_items": 3});
        assert_eq!(
            parse("list.text", raw.clone(), json!(["a"])),
            Err(problem("", codes::TOO_FEW_ITEMS, "has 1 items; at least 2"))
        );
        assert_eq!(
            parse("list.text", raw, json!(["a", "b", "c", "d"])),
            Err(problem("", codes::TOO_MANY_ITEMS, "has 4 items; at most 3"))
        );
    }

    #[test]
    fn lists_hold_at_most_128_items() {
        let items: Vec<i64> = (0..129).collect();
        assert_eq!(
            parse("list.integer", json!({}), json!(items)),
            Err(problem(
                "",
                codes::TOO_MANY_ITEMS,
                "has 129 items; at most 128"
            ))
        );
    }

    #[test]
    fn unique_items_ignore_case_for_text() {
        assert_eq!(
            parse(
                "list.text",
                json!({"unique_items": true}),
                json!(["VIP", "vip"])
            ),
            Err(problem(
                "[1]",
                codes::DUPLICATE_ITEM,
                "is already in the list"
            ))
        );
        assert!(parse("list.text", json!({}), json!(["VIP", "vip"])).is_ok());
    }

    #[test]
    fn index_forms() {
        let text = parse("text", json!({}), json!("Wool")).unwrap();
        assert_eq!(text.scalar_text(), Some("wool".to_string()));
        assert_eq!(text.unique_key(), Some("wool".to_string()));
        let flag = parse("boolean", json!({}), json!(false)).unwrap();
        assert_eq!(flag.scalar_text(), Some("false".to_string()));
        assert_eq!(flag.unique_key(), None);
        let list = parse("list.text", json!({}), json!(["a"])).unwrap();
        assert_eq!((list.scalar_text(), list.scalar_num()), (None, None));
    }

    #[test]
    fn stored_values_are_read_by_type_alone() {
        let value = json!("dry");
        assert_eq!(
            FieldValue::from_stored(FieldType::single(BaseType::Choice), &value),
            Ok(FieldValue::Choice("dry".to_string()))
        );
    }
}
