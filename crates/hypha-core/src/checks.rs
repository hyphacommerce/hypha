//! Messages on a cart: something wrong with it (an error, which stops checkout) or worth a
//! second look (a warning, which never does), each pointing at the part of the cart it's
//! about. Address formats, verification, the merchant's checkout rules and validation apps
//! all speak this one shape, so a storefront shows them alike.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::address::{AddressField, AddressVerdict, AddressWarning};
use crate::{AddressVerification, AppKey, CheckoutRuleId, Timestamp};

/// Whether a message stops checkout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Checkout refuses the cart until it's fixed.
    Error,
    /// Shown to the shopper; checkout goes on.
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where in the cart a message points: an RFC 9535 JSONPath into the cart's response as
/// the caller receives it, from a closed set, so a storefront knows which field to show it
/// under and UCP's `messages` take it as it is. Indexes count from zero in the response's
/// own arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum MessagePath {
    /// `$`: the cart as a whole.
    Cart,
    /// `$.email`
    Email,
    /// `$.shipping_address`
    ShippingAddress,
    /// `$.shipping_address.<field>`
    ShippingAddressField(AddressField),
    /// `$.shipping_groups[<i>]`: the group, and the option chosen for it.
    ShippingGroup(u32),
    /// `$.lines[<i>]`
    Line(u32),
}

impl MessagePath {
    /// Longer than any path of the set: refused before it's parsed.
    const MAX_LEN: usize = 64;
}

impl fmt::Display for MessagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessagePath::Cart => f.write_str("$"),
            MessagePath::Email => f.write_str("$.email"),
            MessagePath::ShippingAddress => f.write_str("$.shipping_address"),
            MessagePath::ShippingAddressField(field) => {
                write!(f, "$.shipping_address.{}", field.as_str())
            }
            MessagePath::ShippingGroup(i) => write!(f, "$.shipping_groups[{i}]"),
            MessagePath::Line(i) => write!(f, "$.lines[{i}]"),
        }
    }
}

impl FromStr for MessagePath {
    type Err = MessageError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || MessageError::Path(s.chars().take(Self::MAX_LEN).collect());
        if s.len() > Self::MAX_LEN {
            return Err(invalid());
        }
        match s {
            "$" => return Ok(MessagePath::Cart),
            "$.email" => return Ok(MessagePath::Email),
            "$.shipping_address" => return Ok(MessagePath::ShippingAddress),
            _ => {}
        }
        if let Some(field) = s.strip_prefix("$.shipping_address.") {
            return AddressField::from_name(field)
                .map(MessagePath::ShippingAddressField)
                .ok_or_else(invalid);
        }
        let indexed = |prefix: &str| {
            s.strip_prefix(prefix)
                .and_then(|rest| rest.strip_suffix(']'))
                .and_then(index)
        };
        if let Some(i) = indexed("$.shipping_groups[") {
            return Ok(MessagePath::ShippingGroup(i));
        }
        if let Some(i) = indexed("$.lines[") {
            return Ok(MessagePath::Line(i));
        }
        Err(invalid())
    }
}

/// An array index as JSONPath writes it: digits, no leading zero but `0` itself.
fn index(digits: &str) -> Option<u32> {
    let plain = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    if plain { digits.parse().ok() } else { None }
}

impl TryFrom<String> for MessagePath {
    type Error = MessageError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<MessagePath> for String {
    fn from(path: MessagePath) -> String {
        path.to_string()
    }
}

/// What a message is about, for code rather than people: snake_case, a letter first, at
/// most 64 characters. Built-in codes are a contract like error codes; a rule's is the
/// merchant's, an app's the app's.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MessageCode(Cow<'static, str>);

impl MessageCode {
    pub const MAX_LEN: usize = 64;

    pub fn new(code: &str) -> Result<Self, MessageError> {
        if !Self::is_valid(code) {
            return Err(MessageError::Code(
                code.chars().take(Self::MAX_LEN + 1).collect(),
            ));
        }
        Ok(Self(Cow::Owned(code.to_string())))
    }

    /// One of the codes this crate names, checked by a test rather than at run time.
    const fn builtin(code: &'static str) -> Self {
        Self(Cow::Borrowed(code))
    }

    fn is_valid(code: &str) -> bool {
        let mut chars = code.chars();
        code.len() <= Self::MAX_LEN
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && !code.ends_with('_')
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for MessageCode {
    type Error = MessageError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(&s)
    }
}

impl From<MessageCode> for String {
    fn from(code: MessageCode) -> String {
        code.0.into_owned()
    }
}

/// Who said it: the storefront may show them alike, staff and apps tell them apart.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageSource {
    /// The address's country format.
    Format,
    /// The store's address verification app.
    Verification,
    /// One of the merchant's checkout rules.
    Rule { rule: CheckoutRuleId },
    /// A validation app.
    App {
        #[serde(with = "app_key")]
        app: AppKey,
    },
}

/// `AppKey` as its string.
mod app_key {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    use crate::AppKey;

    pub(super) fn serialize<S: Serializer>(key: &AppKey, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(key.as_str())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<AppKey, D::Error> {
        let raw = String::deserialize(d)?;
        AppKey::new(&raw).map_err(D::Error::custom)
    }
}

/// The data a client needs to word a built-in message or address problem itself, from its
/// code, rather than show the English `message`. Empty for a checkout rule's or an app's
/// message: those are worded by whoever wrote them. A contract like the codes: fields are
/// added, never renamed. Stored with messages (carts' app checks, orders' address checks),
/// so every field reads back absent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageParams {
    /// The address field the message or problem is about.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "field")]
    pub field: Option<AddressField>,
    /// A value in the country's shape (`invalid_format` on a postal code, where the
    /// country gives one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    /// The region the address names, as its code (`region_mismatch`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// That region's name, as the address data has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_name: Option<String>,
    /// The most characters the field takes (`too_long`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

impl MessageParams {
    /// Params naming only `field`.
    pub fn field(field: AddressField) -> Self {
        Self {
            field: Some(field),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// `AddressField` as its wire name.
mod field {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    use crate::AddressField;

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's `with` hands the field by reference"
    )]
    pub(super) fn serialize<S: Serializer>(
        field: &Option<AddressField>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match field {
            Some(f) => s.serialize_str(f.as_str()),
            None => s.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<AddressField>, D::Error> {
        let raw = Option::<String>::deserialize(d)?;
        raw.map(|name| {
            AddressField::from_name(&name)
                .ok_or_else(|| D::Error::custom(format!("{name:?} is not an address field")))
        })
        .transpose()
    }
}

/// One thing the shopper should know about their cart.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawMessage")]
pub struct Message {
    severity: Severity,
    code: MessageCode,
    path: MessagePath,
    message: String,
    source: MessageSource,
    /// Only built-in messages have any.
    #[serde(default, skip_serializing_if = "MessageParams::is_empty")]
    params: MessageParams,
}

impl Message {
    /// What an app may say in one message.
    pub const MAX_TEXT_LEN: usize = 500;

    /// The house number a country's addresses need: a warning, since some addresses
    /// rightly have none (a named building, a farm).
    pub const HOUSE_NUMBER_MISSING: MessageCode = MessageCode::builtin("house_number_missing");
    /// The verification app would correct the address; the cart carries its suggestion.
    pub const ADDRESS_CORRECTIONS_SUGGESTED: MessageCode =
        MessageCode::builtin("address_corrections_suggested");
    /// The building takes mail, but an apartment or unit is missing.
    pub const APARTMENT_MISSING: MessageCode = MessageCode::builtin("apartment_missing");
    /// The verification app found more than one place.
    pub const ADDRESS_AMBIGUOUS: MessageCode = MessageCode::builtin("address_ambiguous");
    /// The verification app found no place mail is delivered to.
    pub const ADDRESS_UNDELIVERABLE: MessageCode = MessageCode::builtin("address_undeliverable");
    /// A validation app whose failure blocks checkout couldn't be asked.
    pub const VALIDATION_UNAVAILABLE: MessageCode = MessageCode::builtin("validation_unavailable");

    /// `message` is plain text for the shopper: trimmed, 1 to `MAX_TEXT_LEN` characters.
    pub fn new(
        severity: Severity,
        code: MessageCode,
        path: MessagePath,
        message: &str,
        source: MessageSource,
    ) -> Result<Self, MessageError> {
        let message = message.trim();
        if message.is_empty() {
            return Err(MessageError::TextMissing);
        }
        if message.chars().count() > Self::MAX_TEXT_LEN {
            return Err(MessageError::TextTooLong {
                max: Self::MAX_TEXT_LEN,
            });
        }
        Ok(Self {
            severity,
            code,
            path,
            message: message.to_string(),
            source,
            params: MessageParams::default(),
        })
    }

    /// An address's format warning, on the cart's shipping address.
    pub fn format_warning(warning: &AddressWarning) -> Self {
        Self {
            severity: Severity::Warning,
            code: MessageCode::builtin(warning.code.as_str()),
            path: MessagePath::ShippingAddressField(warning.field),
            // The format's own sentence: short, and never quoting the address.
            message: warning.message.clone(),
            source: MessageSource::Format,
            params: MessageParams::field(warning.field),
        }
    }

    /// What a kept verification tells the shopper: a warning for each verdict but
    /// `deliverable` and `unknown` (a failure isn't the shopper's problem). Verification
    /// never blocks on its own; a checkout rule can make a verdict an error.
    pub fn verification_warning(verification: &AddressVerification) -> Option<Self> {
        let (code, path, message) = match verification.verdict {
            AddressVerdict::Deliverable | AddressVerdict::Unknown => return None,
            AddressVerdict::DeliverableWithFixes => (
                Self::ADDRESS_CORRECTIONS_SUGGESTED,
                MessagePath::ShippingAddress,
                "This address may have a mistake. Check the suggested address.",
            ),
            AddressVerdict::MissingSubpremise => (
                Self::APARTMENT_MISSING,
                MessagePath::ShippingAddressField(AddressField::Line2),
                "Add an apartment, suite or unit number.",
            ),
            AddressVerdict::Ambiguous => (
                Self::ADDRESS_AMBIGUOUS,
                MessagePath::ShippingAddress,
                "We couldn't pin this address down. Check the street, number and postal code.",
            ),
            AddressVerdict::Undeliverable => (
                Self::ADDRESS_UNDELIVERABLE,
                MessagePath::ShippingAddress,
                "We couldn't find this address. Check it before you continue.",
            ),
        };
        let params = match path {
            MessagePath::ShippingAddressField(field) => MessageParams::field(field),
            _ => MessageParams::default(),
        };
        Some(Self {
            severity: Severity::Warning,
            code,
            path,
            message: message.to_string(),
            source: MessageSource::Verification,
            params,
        })
    }

    /// What a validation app whose failure blocks checkout adds when it couldn't be asked:
    /// an error on the whole cart, since nobody knows what it would have said.
    pub fn validation_unavailable(app: &AppKey) -> Self {
        Self {
            severity: Severity::Error,
            code: Self::VALIDATION_UNAVAILABLE,
            path: MessagePath::Cart,
            message: "We can't check your order right now. Try again in a moment.".to_string(),
            source: MessageSource::App { app: app.clone() },
            params: MessageParams::default(),
        }
    }

    pub fn severity(&self) -> Severity {
        self.severity
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub fn code(&self) -> &MessageCode {
        &self.code
    }

    pub fn path(&self) -> MessagePath {
        self.path
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn source(&self) -> &MessageSource {
        &self.source
    }

    /// What a client needs to word it from `code`; empty but for built-in messages.
    pub fn params(&self) -> &MessageParams {
        &self.params
    }
}

/// When a validation app is asked: as the shopper goes through checkout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ValidationStep {
    /// The shipping address was set.
    Address,
    /// A shipping option was chosen.
    Shipping,
    /// The shopper is placing the order or starting its payment.
    Checkout,
}

impl ValidationStep {
    pub fn as_str(self) -> &'static str {
        match self {
            ValidationStep::Address => "address",
            ValidationStep::Shipping => "shipping",
            ValidationStep::Checkout => "checkout",
        }
    }
}

/// A validation app's last answer about a cart, kept with it: what it was asked (by the
/// digest of the request, whatever the step), its messages or why it couldn't answer, and
/// when. Stored as JSON with the cart, so a field is added, never renamed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppCheck {
    #[serde(with = "app_key")]
    pub app: AppKey,
    /// The digest of the request it answered: a cart it would be sent the same about isn't
    /// asked again.
    pub digest: String,
    /// What it said, each with the app as its source; none when it failed.
    #[serde(default)]
    pub messages: Vec<Message>,
    /// Why it couldn't answer (no answer, an error, an answer the point refuses); the
    /// extension call log has the details.
    #[serde(default)]
    pub failure: Option<String>,
    #[serde(with = "crate::time::serde_micros")]
    pub checked_at: Timestamp,
}

impl AppCheck {
    /// Whether this answer holds for a request with `digest`: the same question, answered.
    /// A failure never holds, so the app is asked again next time.
    pub fn answers(&self, digest: &str) -> bool {
        self.failure.is_none() && self.digest == digest
    }
}

#[derive(Deserialize)]
struct RawMessage {
    severity: Severity,
    code: MessageCode,
    path: MessagePath,
    message: String,
    source: MessageSource,
    /// Read leniently (`MessageParams::stored`): a param is help for wording a message,
    /// never a reason for the cart or order holding it not to load.
    #[serde(default)]
    params: serde_json::Value,
}

impl TryFrom<RawMessage> for Message {
    type Error = MessageError;

    fn try_from(raw: RawMessage) -> Result<Self, Self::Error> {
        let mut message = Message::new(raw.severity, raw.code, raw.path, &raw.message, raw.source)?;
        message.params = MessageParams::stored(raw.params);
        Ok(message)
    }
}

impl MessageParams {
    /// Params as a stored message holds them. One that no longer reads (a field since
    /// renamed or removed, a row from a later version rolled back) is dropped and the
    /// rest kept; anything but an object reads as none. Building params stays typed.
    fn stored(json: serde_json::Value) -> Self {
        let serde_json::Value::Object(map) = json else {
            return Self::default();
        };
        if let Ok(params) = serde_json::from_value(serde_json::Value::Object(map.clone())) {
            return params;
        }
        let mut params = Self::default();
        for (key, value) in map {
            let one = serde_json::Value::Object([(key, value)].into_iter().collect());
            let Ok(read) = serde_json::from_value::<Self>(one) else {
                continue;
            };
            params.field = params.field.or(read.field);
            params.example = params.example.or(read.example);
            params.region = params.region.or(read.region);
            params.region_name = params.region_name.or(read.region_name);
            params.max = params.max.or(read.max);
        }
        params
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum MessageError {
    #[error(
        "{0:?} is not a message code: lowercase letters, digits and underscores, a letter first, at most 64 characters"
    )]
    Code(String),
    #[error(
        "{0:?} is not a message path: \"$\", \"$.email\", \"$.shipping_address\", \"$.shipping_address.<field>\", \"$.shipping_groups[<i>]\" or \"$.lines[<i>]\""
    )]
    Path(String),
    #[error("a message needs text")]
    TextMissing,
    #[error("a message is at most {max} characters")]
    TextTooLong { max: usize },
}

impl crate::ErrorCode for MessageError {
    fn code(&self) -> &'static str {
        match self {
            MessageError::Code(_) => "invalid_message_code",
            MessageError::Path(_) => "invalid_message_path",
            MessageError::TextMissing | MessageError::TextTooLong { .. } => "invalid_message",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AddressWarningCode;
    use uuid::Uuid;

    fn path(s: &str) -> Result<MessagePath, MessageError> {
        s.parse()
    }

    fn warning() -> Message {
        Message::format_warning(&AddressWarning {
            field: AddressField::Line1,
            code: AddressWarningCode::HouseNumberMissing,
            message: "Add a house number".to_string(),
        })
    }

    #[test]
    fn every_path_of_the_set_reads_back() {
        let paths = [
            MessagePath::Cart,
            MessagePath::Email,
            MessagePath::ShippingAddress,
            MessagePath::ShippingAddressField(AddressField::PostalCode),
            MessagePath::ShippingGroup(0),
            MessagePath::Line(12),
        ];
        for p in paths {
            assert_eq!(path(&p.to_string()), Ok(p));
        }
    }

    #[test]
    fn a_field_path_is_written_as_jsonpath() {
        assert_eq!(
            MessagePath::ShippingAddressField(AddressField::Line1).to_string(),
            "$.shipping_address.line1"
        );
    }

    #[test]
    fn an_index_path_is_written_with_brackets() {
        assert_eq!(
            MessagePath::ShippingGroup(3).to_string(),
            "$.shipping_groups[3]"
        );
    }

    #[test]
    fn an_unknown_address_field_is_no_path() {
        assert_eq!(
            path("$.shipping_address.street"),
            Err(MessageError::Path("$.shipping_address.street".to_string()))
        );
    }

    #[test]
    fn a_path_outside_the_set_is_refused() {
        assert_eq!(
            path("$.payment_method"),
            Err(MessageError::Path("$.payment_method".to_string()))
        );
    }

    #[test]
    fn an_index_with_a_leading_zero_is_refused() {
        assert_eq!(
            path("$.lines[01]"),
            Err(MessageError::Path("$.lines[01]".to_string()))
        );
    }

    #[test]
    fn a_negative_index_is_refused() {
        assert_eq!(
            path("$.lines[-1]"),
            Err(MessageError::Path("$.lines[-1]".to_string()))
        );
    }

    #[test]
    fn an_index_past_u32_is_refused() {
        assert_eq!(
            path("$.lines[4294967296]"),
            Err(MessageError::Path("$.lines[4294967296]".to_string()))
        );
    }

    #[test]
    fn a_wildcard_is_refused() {
        assert_eq!(
            path("$.lines[*]"),
            Err(MessageError::Path("$.lines[*]".to_string()))
        );
    }

    #[test]
    fn a_code_is_snake_case() {
        assert_eq!(
            MessageCode::new("po_box_not_allowed").map(|c| c.to_string()),
            Ok("po_box_not_allowed".to_string())
        );
    }

    #[test]
    fn a_code_starts_with_a_letter() {
        assert_eq!(
            MessageCode::new("1st"),
            Err(MessageError::Code("1st".to_string()))
        );
    }

    #[test]
    fn a_code_has_no_capitals() {
        assert_eq!(
            MessageCode::new("PoBox"),
            Err(MessageError::Code("PoBox".to_string()))
        );
    }

    #[test]
    fn a_code_doesnt_end_with_an_underscore() {
        assert_eq!(
            MessageCode::new("po_box_"),
            Err(MessageError::Code("po_box_".to_string()))
        );
    }

    #[test]
    fn a_code_of_64_characters_is_taken() {
        assert!(MessageCode::new(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn a_code_of_65_characters_is_refused() {
        assert_eq!(
            MessageCode::new(&"a".repeat(65)),
            Err(MessageError::Code("a".repeat(65)))
        );
    }

    #[test]
    fn the_built_in_codes_are_valid() {
        let warning_codes = [AddressWarningCode::HouseNumberMissing];
        for code in warning_codes {
            assert!(MessageCode::is_valid(code.as_str()), "{}", code.as_str());
        }
        for code in [
            Message::HOUSE_NUMBER_MISSING,
            Message::ADDRESS_CORRECTIONS_SUGGESTED,
            Message::APARTMENT_MISSING,
            Message::ADDRESS_AMBIGUOUS,
            Message::ADDRESS_UNDELIVERABLE,
            Message::VALIDATION_UNAVAILABLE,
        ] {
            assert!(MessageCode::is_valid(code.as_str()), "{code}");
        }
    }

    fn verified(verdict: AddressVerdict) -> AddressVerification {
        AddressVerification {
            digest: "d".to_string(),
            verdict,
            precision: None,
            fields: Default::default(),
            suggested: None,
            suggested_digest: None,
            flags: Default::default(),
            app: None,
            reference: None,
            failure: None,
            checked_at: crate::Timestamp::from_unix_micros(0),
        }
    }

    #[test]
    fn a_deliverable_or_unknown_address_says_nothing() {
        assert_eq!(
            Message::verification_warning(&verified(AddressVerdict::Deliverable)),
            None
        );
        assert_eq!(
            Message::verification_warning(&verified(AddressVerdict::Unknown)),
            None
        );
    }

    #[test]
    fn each_other_verdict_warns_where_the_shopper_fixes_it() {
        let said: Vec<(String, String)> = [
            AddressVerdict::DeliverableWithFixes,
            AddressVerdict::MissingSubpremise,
            AddressVerdict::Ambiguous,
            AddressVerdict::Undeliverable,
        ]
        .into_iter()
        .filter_map(|v| Message::verification_warning(&verified(v)))
        .map(|m| (m.code().to_string(), m.path().to_string()))
        .collect();
        assert_eq!(
            said,
            [
                ("address_corrections_suggested", "$.shipping_address"),
                ("apartment_missing", "$.shipping_address.line2"),
                ("address_ambiguous", "$.shipping_address"),
                ("address_undeliverable", "$.shipping_address"),
            ]
            .map(|(c, p)| (c.to_string(), p.to_string()))
        );
    }

    #[test]
    fn a_verification_warning_never_blocks() {
        let message =
            Message::verification_warning(&verified(AddressVerdict::Undeliverable)).unwrap();
        assert_eq!(message.severity(), Severity::Warning);
        assert_eq!(message.source(), &MessageSource::Verification);
    }

    #[test]
    fn a_format_warning_reuses_the_warnings_code() {
        assert_eq!(warning().code(), &Message::HOUSE_NUMBER_MISSING);
    }

    #[test]
    fn a_format_warning_points_at_its_field() {
        assert_eq!(
            warning().path(),
            MessagePath::ShippingAddressField(AddressField::Line1)
        );
    }

    #[test]
    fn a_format_warning_never_blocks() {
        assert!(!warning().is_error());
    }

    #[test]
    fn text_is_trimmed() {
        let message = Message::new(
            Severity::Error,
            Message::HOUSE_NUMBER_MISSING,
            MessagePath::Cart,
            "  Add a house number  ",
            MessageSource::Format,
        );
        assert_eq!(
            message.map(|m| m.message().to_string()),
            Ok("Add a house number".to_string())
        );
    }

    #[test]
    fn blank_text_is_refused() {
        let message = Message::new(
            Severity::Error,
            Message::HOUSE_NUMBER_MISSING,
            MessagePath::Cart,
            "   ",
            MessageSource::Format,
        );
        assert_eq!(message, Err(MessageError::TextMissing));
    }

    #[test]
    fn text_of_500_characters_is_taken() {
        let message = Message::new(
            Severity::Error,
            Message::HOUSE_NUMBER_MISSING,
            MessagePath::Cart,
            &"é".repeat(500),
            MessageSource::Format,
        );
        assert!(message.is_ok());
    }

    #[test]
    fn text_of_501_characters_is_refused() {
        let message = Message::new(
            Severity::Error,
            Message::HOUSE_NUMBER_MISSING,
            MessagePath::Cart,
            &"é".repeat(501),
            MessageSource::Format,
        );
        assert_eq!(message, Err(MessageError::TextTooLong { max: 500 }));
    }

    #[test]
    fn a_message_serialises_in_the_wire_shape() {
        let rule = CheckoutRuleId::from_uuid(Uuid::from_u128(1));
        let message = Message::new(
            Severity::Error,
            MessageCode::new("po_box_not_allowed").unwrap(),
            MessagePath::ShippingGroup(0),
            "No PO boxes with DHL Express.",
            MessageSource::Rule { rule },
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&message).unwrap(),
            serde_json::json!({
                "severity": "error",
                "code": "po_box_not_allowed",
                "path": "$.shipping_groups[0]",
                "message": "No PO boxes with DHL Express.",
                "source": { "type": "rule", "rule": rule.to_string() },
            })
        );
    }

    #[test]
    fn a_format_warning_names_its_field_in_params() {
        assert_eq!(
            serde_json::to_value(warning()).unwrap()["params"],
            serde_json::json!({ "field": "line1" })
        );
    }

    #[test]
    fn a_missing_apartment_names_line_two_in_params() {
        let message =
            Message::verification_warning(&verified(AddressVerdict::MissingSubpremise)).unwrap();
        assert_eq!(message.params(), &MessageParams::field(AddressField::Line2));
    }

    #[test]
    fn a_whole_address_verdict_has_no_params() {
        let message = Message::verification_warning(&verified(AddressVerdict::Ambiguous)).unwrap();
        assert!(message.params().is_empty());
    }

    #[test]
    fn a_stored_built_in_message_keeps_its_params() {
        let json = serde_json::to_string(&warning()).unwrap();
        assert_eq!(serde_json::from_str::<Message>(&json).unwrap(), warning());
    }

    #[test]
    fn a_message_stored_without_params_reads_back_with_none() {
        let json = r#"{"severity":"warning","code":"house_number_missing","path":"$.shipping_address.line1","message":"Add a house number","source":{"type":"format"}}"#;
        let message = serde_json::from_str::<Message>(json).unwrap();
        assert!(message.params().is_empty());
    }

    #[test]
    fn a_stored_param_naming_no_field_is_dropped_and_the_message_loads() {
        let json = r#"{"severity":"warning","code":"x","path":"$","message":"m","source":{"type":"format"},"params":{"field":"street","max":40}}"#;
        let message = serde_json::from_str::<Message>(json).unwrap();
        assert_eq!(
            message.params(),
            &MessageParams {
                max: Some(40),
                ..MessageParams::default()
            }
        );
    }

    #[test]
    fn stored_params_that_arent_an_object_read_as_none() {
        for params in ["\"field\"", "[1]", "null", r#"{"max":"forty"}"#] {
            let json = format!(
                r#"{{"severity":"warning","code":"x","path":"$","message":"m","source":{{"type":"format"}},"params":{params}}}"#
            );
            let message = serde_json::from_str::<Message>(&json).unwrap();
            assert!(message.params().is_empty(), "{params}");
        }
    }

    #[test]
    fn a_stored_message_reads_back() {
        let message = Message::new(
            Severity::Warning,
            MessageCode::new("alcohol_restricted").unwrap(),
            MessagePath::Line(1),
            "Can't ship this here.",
            MessageSource::App {
                app: AppKey::new("app_rules").unwrap(),
            },
        )
        .unwrap();
        let json = serde_json::to_string(&message).unwrap();
        assert_eq!(serde_json::from_str::<Message>(&json).unwrap(), message);
    }

    fn app() -> AppKey {
        AppKey::new("app_rules").unwrap()
    }

    fn check(failure: Option<&str>) -> AppCheck {
        AppCheck {
            app: app(),
            digest: "d1".to_string(),
            messages: Vec::new(),
            failure: failure.map(str::to_string),
            checked_at: crate::Timestamp::from_unix_micros(0),
        }
    }

    #[test]
    fn an_answer_holds_for_the_same_request() {
        assert!(check(None).answers("d1"));
    }

    #[test]
    fn an_answer_doesnt_hold_for_another_request() {
        assert!(!check(None).answers("d2"));
    }

    #[test]
    fn a_failure_never_holds() {
        assert!(!check(Some("timeout")).answers("d1"));
    }

    #[test]
    fn a_kept_answer_reads_back() {
        let mut kept = check(None);
        kept.messages.push(
            Message::new(
                Severity::Error,
                MessageCode::new("alcohol_restricted").unwrap(),
                MessagePath::ShippingAddressField(AddressField::PostalCode),
                "We can't ship alcohol here.",
                MessageSource::App { app: app() },
            )
            .unwrap(),
        );
        let json = serde_json::to_string(&kept).unwrap();
        assert_eq!(serde_json::from_str::<AppCheck>(&json).unwrap(), kept);
    }

    #[test]
    fn an_unavailable_app_blocks_on_the_whole_cart() {
        let m = Message::validation_unavailable(&app());
        assert_eq!(
            (m.severity(), m.path(), m.code().as_str()),
            (Severity::Error, MessagePath::Cart, "validation_unavailable")
        );
        assert_eq!(m.source(), &MessageSource::App { app: app() });
    }

    #[test]
    fn steps_have_stable_names() {
        assert_eq!(
            [
                ValidationStep::Address,
                ValidationStep::Shipping,
                ValidationStep::Checkout
            ]
            .map(ValidationStep::as_str),
            ["address", "shipping", "checkout"]
        );
    }

    #[test]
    fn a_stored_message_with_a_bad_path_doesnt_read() {
        let json = r#"{"severity":"error","code":"x","path":"$.nope","message":"m","source":{"type":"format"}}"#;
        assert!(serde_json::from_str::<Message>(json).is_err());
    }
}
