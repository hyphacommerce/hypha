//! `address.verify` v1: the store's verification app says whether mail reaches an
//! address, what it would correct and what the address is. Asked when a cart's address is
//! set and when staff edit an order's; its answer is kept, so the same address is never
//! looked up twice.

use std::collections::BTreeMap;

use core::time::Duration;

use hypha_core::{
    AddressField, AddressPrecision, AddressVerdict, FieldStatus, Scope, SuggestedAddress,
    VerificationFlags, VerifyContext,
};
use serde::{Deserialize, Serialize};

use crate::{ExtensionPoint, OnFailure};

/// Verifies a shipping address.
pub struct AddressVerify;

impl ExtensionPoint for AddressVerify {
    const NAME: &'static str = "address.verify";
    const VERSION: u16 = 1;
    const TIMEOUT: Duration = Duration::from_secs(3);
    /// The address is kept as `unknown` and checkout goes on: a provider's outage isn't
    /// the shopper's problem.
    const ON_FAILURE: OnFailure = OnFailure::Fallback;
    /// The app reads the street.
    const SCOPE: Scope = Scope::CustomerDataAddress;
    const READ_ONLY: bool = true;
    type Request = AddressVerifyRequest;
    type Response = AddressVerifyResponse;

    /// A suggestion stays in the address's country and has its street and city; field
    /// statuses name address fields.
    fn check(
        request: &AddressVerifyRequest,
        response: &AddressVerifyResponse,
    ) -> Result<(), String> {
        if let Some(suggested) = &response.suggested {
            if !suggested
                .country
                .eq_ignore_ascii_case(&request.address.country)
            {
                return Err(format!(
                    "the suggested address is in {:?}, not {}",
                    suggested.country, request.address.country
                ));
            }
            if suggested.line1.trim().is_empty() || suggested.city.trim().is_empty() {
                return Err("the suggested address has no street or city".to_string());
            }
        }
        if let Some(field) = response
            .fields
            .keys()
            .find(|f| AddressField::from_name(f).is_none())
        {
            return Err(format!("{field:?} isn't an address field"));
        }
        Ok(())
    }
}

/// The address to verify: where it is, never who lives there (no names or phone).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VerifiedAddress {
    pub company: Option<String>,
    pub line1: String,
    pub line2: Option<String>,
    pub city: String,
    /// The region's code where the country lists regions (`CA`).
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub country: String,
}

/// How sure the provider is of one field: an object, so a later version can say more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FieldVerification {
    pub status: FieldStatus,
}

/// What Hypha sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddressVerifyRequest {
    pub address: VerifiedAddress,
    /// `cart` when a shopper sets it, `order` when staff edit an order's.
    pub context: VerifyContext,
}

/// The app's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddressVerifyResponse {
    pub verdict: AddressVerdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub precision: Option<AddressPrecision>,
    /// Per address field (`postal_code`), how sure the provider is.
    #[serde(default)]
    pub fields: BTreeMap<String, FieldVerification>,
    /// The corrected address, only when the provider is confident; same country.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub suggested: Option<SuggestedAddress>,
    /// What the address is; a flag left out is unknown.
    #[serde(default)]
    pub flags: VerificationFlags,
    /// The provider's own ID for the lookup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub reference: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn request() -> AddressVerifyRequest {
        AddressVerifyRequest {
            address: VerifiedAddress {
                company: None,
                line1: "1600 Amphitheatre Pkwy".to_string(),
                line2: None,
                city: "Mountain View".to_string(),
                province: Some("CA".to_string()),
                postal_code: Some("94043".to_string()),
                country: "US".to_string(),
            },
            context: VerifyContext::Cart,
        }
    }

    fn answer(value: serde_json::Value) -> AddressVerifyResponse {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn a_full_answer_parses() {
        let parsed = answer(json!({
            "verdict": "deliverable_with_fixes",
            "precision": "premise",
            "fields": {"postal_code": {"status": "corrected"}},
            "suggested": {"line1": "1600 Amphitheatre Pkwy", "city": "Mountain View",
                          "province": "CA", "postal_code": "94043-1351", "country": "US"},
            "flags": {"business": true},
            "reference": "r1"
        }));
        assert_eq!(parsed.verdict, AddressVerdict::DeliverableWithFixes);
        assert_eq!(parsed.flags.business, Some(true));
        assert_eq!(parsed.flags.po_box, None);
        assert_eq!(AddressVerify::check(&request(), &parsed), Ok(()));
    }

    #[test]
    fn a_verdict_alone_is_enough() {
        let parsed = answer(json!({"verdict": "deliverable"}));
        assert_eq!(parsed.suggested, None);
        assert_eq!(AddressVerify::check(&request(), &parsed), Ok(()));
    }

    #[test]
    fn a_suggestion_in_another_country_is_refused() {
        let parsed = answer(json!({"verdict": "deliverable_with_fixes",
            "suggested": {"line1": "1 Rue", "city": "Paris", "country": "FR"}}));
        assert_eq!(
            AddressVerify::check(&request(), &parsed),
            Err("the suggested address is in \"FR\", not US".to_string())
        );
    }

    #[test]
    fn a_suggestion_without_a_street_is_refused() {
        let parsed = answer(json!({"verdict": "deliverable_with_fixes",
            "suggested": {"line1": " ", "city": "Mountain View", "country": "US"}}));
        assert_eq!(
            AddressVerify::check(&request(), &parsed),
            Err("the suggested address has no street or city".to_string())
        );
    }

    #[test]
    fn a_status_for_something_not_a_field_is_refused() {
        let parsed = answer(json!({"verdict": "deliverable",
            "fields": {"planet": {"status": "confirmed"}}}));
        assert_eq!(
            AddressVerify::check(&request(), &parsed),
            Err("\"planet\" isn't an address field".to_string())
        );
    }

    #[test]
    fn the_request_names_its_context() {
        let sent = serde_json::to_value(request()).unwrap();
        assert_eq!(sent["context"], "cart");
        assert_eq!(sent["address"]["province"], "CA");
        assert_eq!(sent["address"].get("last_name"), None);
    }
}
