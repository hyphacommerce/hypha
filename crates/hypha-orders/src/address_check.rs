//! An order's address check: the verification its cart kept (or the one asked again when
//! staff edited the address), the address warnings it was placed with, and who marked
//! the address valid. Stored as JSON with the order, so its shape is storage's: a field
//! is added, never renamed.

use hypha_core::{AddressVerification, AddressWarning, Message, MessagePath, Timestamp};
use serde::{Deserialize, Serialize};

/// Who looked at the address and said it's fine, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkedValid {
    /// The actor's prefixed ID (`usr_…`).
    pub by: String,
    #[serde(with = "hypha_core::time::serde_micros")]
    pub at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OrderAddressCheck {
    #[serde(default)]
    pub verification: Option<AddressVerification>,
    /// The cart's warnings about the address when the order was placed (or the format's,
    /// when staff edited it).
    #[serde(default)]
    pub warnings: Vec<Message>,
    #[serde(default)]
    pub marked_valid: Option<MarkedValid>,
}

impl OrderAddressCheck {
    /// What a cart had to say about its address when it was checked out: its
    /// verification, and its warnings that point at the address (a line's or a shipping
    /// group's are about something else).
    pub fn placed(verification: Option<AddressVerification>, messages: &[Message]) -> Self {
        Self {
            verification,
            warnings: messages
                .iter()
                .filter(|m| !m.is_error() && is_about_the_address(m.path()))
                .cloned()
                .collect(),
            marked_valid: None,
        }
    }

    /// The check of an address staff set: its verification, and the warnings a cart
    /// would show for it (the format's and the verification's), as when it's placed.
    pub fn edited(
        verification: Option<AddressVerification>,
        format_warnings: &[AddressWarning],
    ) -> Self {
        let messages: Vec<Message> = format_warnings
            .iter()
            .map(Message::format_warning)
            .chain(
                verification
                    .as_ref()
                    .and_then(Message::verification_warning),
            )
            .collect();
        Self::placed(verification, &messages)
    }

    /// Whether staff should look at the address before shipping it: the provider
    /// couldn't place it (or not as one place), or it was placed with a warning about it,
    /// and nobody has marked it valid since.
    pub fn needs_review(&self) -> bool {
        self.marked_valid.is_none()
            && (self
                .verification
                .as_ref()
                .is_some_and(AddressVerification::needs_review)
                || !self.warnings.is_empty())
    }
}

fn is_about_the_address(path: MessagePath) -> bool {
    matches!(
        path,
        MessagePath::ShippingAddress | MessagePath::ShippingAddressField(_)
    )
}

#[cfg(test)]
mod tests {
    use hypha_core::{
        AddressField, AddressVerdict, AddressWarning, AddressWarningCode, MessageCode,
        MessageSource, Severity,
    };

    use super::*;

    fn verification(verdict: AddressVerdict) -> AddressVerification {
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
            checked_at: Timestamp::from_unix_micros(0),
        }
    }

    fn house_number() -> Message {
        Message::format_warning(&AddressWarning {
            field: AddressField::Line1,
            code: AddressWarningCode::HouseNumberMissing,
            message: "Add a house number".to_string(),
        })
    }

    fn on_a_line() -> Message {
        Message::new(
            Severity::Warning,
            MessageCode::new("rule").unwrap(),
            MessagePath::Line(0),
            "Fragile.",
            MessageSource::Format,
        )
        .unwrap()
    }

    #[test]
    fn only_the_warnings_about_the_address_are_kept() {
        let check = OrderAddressCheck::placed(None, &[house_number(), on_a_line()]);
        assert_eq!(check.warnings, vec![house_number()]);
    }

    #[test]
    fn a_deliverable_address_without_warnings_needs_no_review() {
        let check = OrderAddressCheck::placed(
            Some(verification(AddressVerdict::Deliverable)),
            &[on_a_line()],
        );
        assert!(!check.needs_review());
    }

    #[test]
    fn an_undeliverable_verdict_needs_review() {
        let check =
            OrderAddressCheck::placed(Some(verification(AddressVerdict::Undeliverable)), &[]);
        assert!(check.needs_review());
    }

    #[test]
    fn an_address_warning_needs_review() {
        assert!(OrderAddressCheck::placed(None, &[house_number()]).needs_review());
    }

    #[test]
    fn an_edited_address_keeps_the_verifications_warning() {
        let check =
            OrderAddressCheck::edited(Some(verification(AddressVerdict::MissingSubpremise)), &[]);
        assert_eq!(check.warnings.len(), 1);
        assert!(check.needs_review());
    }

    #[test]
    fn marked_valid_needs_no_review() {
        let mut check = OrderAddressCheck::placed(None, &[house_number()]);
        check.marked_valid = Some(MarkedValid {
            by: "system".to_string(),
            at: Timestamp::from_unix_micros(1),
        });
        assert!(!check.needs_review());
    }

    #[test]
    fn it_round_trips_through_json() {
        let check = OrderAddressCheck {
            verification: Some(verification(AddressVerdict::Ambiguous)),
            warnings: vec![house_number()],
            marked_valid: Some(MarkedValid {
                by: "system".to_string(),
                at: Timestamp::from_unix_micros(7),
            }),
        };
        let json = serde_json::to_string(&check).unwrap();
        assert_eq!(
            serde_json::from_str::<OrderAddressCheck>(&json).unwrap(),
            check
        );
    }
}
