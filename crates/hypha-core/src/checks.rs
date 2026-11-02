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
use crate::{AddressVerification, AppKey, CheckoutRuleId};

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

/// One thing the shopper should know about their cart.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawMessage")]
pub struct Message {
    severity: Severity,
    code: MessageCode,
    path: MessagePath,
    message: String,
    source: MessageSource,
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
        Some(Self {
            severity: Severity::Warning,
            code,
            path,
            message: message.to_string(),
            source: MessageSource::Verification,
        })
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
}

#[derive(Deserialize)]
struct RawMessage {
    severity: Severity,
    code: MessageCode,
    path: MessagePath,
    message: String,
    source: MessageSource,
}

impl TryFrom<RawMessage> for Message {
    type Error = MessageError;

    fn try_from(raw: RawMessage) -> Result<Self, Self::Error> {
        Message::new(raw.severity, raw.code, raw.path, &raw.message, raw.source)
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

    #[test]
    fn a_stored_message_with_a_bad_path_doesnt_read() {
        let json = r#"{"severity":"error","code":"x","path":"$.nope","message":"m","source":{"type":"format"}}"#;
        assert!(serde_json::from_str::<Message>(json).is_err());
    }
}
