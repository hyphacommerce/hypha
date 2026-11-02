//! The address points.
//!
//! - `address.verify` v1: the store's verification app says whether mail reaches an
//!   address, what it would correct and what the address is. Asked when a cart's address
//!   is set and when staff edit an order's; its answer is kept, so the same address is
//!   never looked up twice.
//! - `address.suggest` v1 and `address.resolve` v1: the store's autocomplete app offers
//!   addresses while someone types one, then turns the one picked into its parts. Called
//!   through the API, so the provider's keys stay with the app.

use std::collections::BTreeMap;

use core::time::Duration;

use hypha_core::{
    AddressField, AddressPrecision, AddressVerdict, CountryCode, FieldStatus, Scope,
    SuggestedAddress, VerificationFlags, VerifyContext,
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

/// At most this many suggestions per answer (as many as a dropdown shows at once).
pub const MAX_SUGGESTIONS: u8 = 5;

/// The longest suggestion ID or session an app or a client may send: providers' place IDs
/// are opaque and some are long.
pub const MAX_SUGGESTION_ID: usize = 512;

/// The longest label or description of a suggestion, in characters.
pub const MAX_SUGGESTION_TEXT: usize = 300;

/// Offers addresses for what someone has typed so far.
pub struct AddressSuggest;

impl ExtensionPoint for AddressSuggest {
    const NAME: &'static str = "address.suggest";
    const VERSION: u16 = 1;
    /// Asked on keystrokes: a slow answer is worse than none.
    const TIMEOUT: Duration = Duration::from_secs(1);
    /// No suggestions: the person types the address.
    const ON_FAILURE: OnFailure = OnFailure::Fallback;
    /// The app reads the street being typed.
    const SCOPE: Scope = Scope::CustomerDataAddress;
    const READ_ONLY: bool = true;
    type Request = AddressSuggestRequest;
    type Response = AddressSuggestResponse;

    /// No more suggestions than asked for, each with an ID and a label of sensible size.
    fn check(
        request: &AddressSuggestRequest,
        response: &AddressSuggestResponse,
    ) -> Result<(), String> {
        if response.suggestions.len() > usize::from(request.limit.min(MAX_SUGGESTIONS)) {
            return Err(format!(
                "{} suggestions, more than the {} asked for",
                response.suggestions.len(),
                request.limit.min(MAX_SUGGESTIONS)
            ));
        }
        for s in &response.suggestions {
            if s.id.is_empty() || s.id.len() > MAX_SUGGESTION_ID {
                return Err(format!(
                    "a suggestion's id is empty or over {MAX_SUGGESTION_ID} bytes"
                ));
            }
            if s.label.trim().is_empty() {
                return Err("a suggestion has no label".to_string());
            }
            let long = |t: &str| t.chars().count() > MAX_SUGGESTION_TEXT;
            if long(&s.label) || s.description.as_deref().is_some_and(long) {
                return Err(format!(
                    "a suggestion's label or description is over {MAX_SUGGESTION_TEXT} characters"
                ));
            }
        }
        Ok(())
    }
}

/// What Hypha sends while someone types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddressSuggestRequest {
    /// What's typed so far (3 to 200 characters).
    pub query: String,
    /// The country the form is for: suggestions stay in it.
    #[cfg_attr(feature = "openapi", schema(value_type = String, example = "HR"))]
    pub country: CountryCode,
    /// The client's token for one address being typed, the same until one is picked, for
    /// providers that bill per session.
    pub session: String,
    /// How many suggestions to answer at most (1 to 5).
    pub limit: u8,
}

/// The app's answer: what to show in the dropdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddressSuggestResponse {
    #[serde(default)]
    pub suggestions: Vec<AddressSuggestion>,
}

/// One address offered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddressSuggestion {
    /// The app's own ID for it, sent back to `address.resolve` when it's picked.
    pub id: String,
    /// The main line shown ("Ilica 1").
    pub label: String,
    /// The rest, shown smaller ("10000 Zagreb, Croatia").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub description: Option<String>,
}

/// Turns a picked suggestion into an address.
pub struct AddressResolve;

impl ExtensionPoint for AddressResolve {
    const NAME: &'static str = "address.resolve";
    const VERSION: u16 = 1;
    const TIMEOUT: Duration = Duration::from_secs(1);
    /// The person types the address instead.
    const ON_FAILURE: OnFailure = OnFailure::Fallback;
    const SCOPE: Scope = Scope::CustomerDataAddress;
    const READ_ONLY: bool = true;
    type Request = AddressResolveRequest;
    type Response = AddressResolveResponse;

    /// The address is in the form's country and has its street and city.
    fn check(
        request: &AddressResolveRequest,
        response: &AddressResolveResponse,
    ) -> Result<(), String> {
        let address = &response.address;
        if !address
            .country
            .eq_ignore_ascii_case(request.country.as_str())
        {
            return Err(format!(
                "the address is in {:?}, not {}",
                address.country, request.country
            ));
        }
        if address.line1.trim().is_empty() || address.city.trim().is_empty() {
            return Err("the address has no street or city".to_string());
        }
        Ok(())
    }
}

/// What Hypha sends when a suggestion is picked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddressResolveRequest {
    /// The suggestion's `id`, as `address.suggest` answered it.
    pub id: String,
    /// The country the form is for: the address must be in it.
    #[cfg_attr(feature = "openapi", schema(value_type = String, example = "HR"))]
    pub country: CountryCode,
    /// The same session the suggestions were asked in.
    pub session: String,
}

/// The app's answer: the address's parts, which then go through the country's format
/// check like any typed address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddressResolveResponse {
    pub address: SuggestedAddress,
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

    fn suggest_request(limit: u8) -> AddressSuggestRequest {
        AddressSuggestRequest {
            query: "Ilica".to_string(),
            country: CountryCode::new("HR").unwrap(),
            session: "s1".to_string(),
            limit,
        }
    }

    fn suggestions(n: usize) -> AddressSuggestResponse {
        AddressSuggestResponse {
            suggestions: (0..n)
                .map(|i| AddressSuggestion {
                    id: format!("p{i}"),
                    label: format!("Ilica {i}"),
                    description: Some("10000 Zagreb".to_string()),
                })
                .collect(),
        }
    }

    #[test]
    fn as_many_suggestions_as_asked_for_pass() {
        assert_eq!(
            AddressSuggest::check(&suggest_request(3), &suggestions(3)),
            Ok(())
        );
    }

    #[test]
    fn more_suggestions_than_asked_for_are_refused() {
        assert_eq!(
            AddressSuggest::check(&suggest_request(3), &suggestions(4)),
            Err("4 suggestions, more than the 3 asked for".to_string())
        );
    }

    #[test]
    fn no_more_than_five_whatever_is_asked() {
        assert_eq!(
            AddressSuggest::check(&suggest_request(9), &suggestions(6)),
            Err("6 suggestions, more than the 5 asked for".to_string())
        );
    }

    #[test]
    fn a_suggestion_without_a_label_is_refused() {
        let mut answer = suggestions(1);
        answer.suggestions[0].label = " ".to_string();
        assert_eq!(
            AddressSuggest::check(&suggest_request(5), &answer),
            Err("a suggestion has no label".to_string())
        );
    }

    #[test]
    fn a_suggestion_without_an_id_is_refused() {
        let mut answer = suggestions(1);
        answer.suggestions[0].id = String::new();
        assert!(AddressSuggest::check(&suggest_request(5), &answer).is_err());
    }

    #[test]
    fn no_suggestions_is_an_answer() {
        let parsed: AddressSuggestResponse = serde_json::from_value(json!({})).unwrap();
        assert_eq!(AddressSuggest::check(&suggest_request(5), &parsed), Ok(()));
    }

    fn resolve_request() -> AddressResolveRequest {
        AddressResolveRequest {
            id: "p1".to_string(),
            country: CountryCode::new("HR").unwrap(),
            session: "s1".to_string(),
        }
    }

    #[test]
    fn a_resolved_address_in_the_forms_country_passes() {
        let parsed: AddressResolveResponse = serde_json::from_value(json!({"address":
            {"line1": "Ilica 1", "city": "Zagreb", "postal_code": "10000", "country": "hr"}}))
        .unwrap();
        assert_eq!(AddressResolve::check(&resolve_request(), &parsed), Ok(()));
    }

    #[test]
    fn a_resolved_address_in_another_country_is_refused() {
        let parsed: AddressResolveResponse = serde_json::from_value(json!({"address":
            {"line1": "Slovenska 1", "city": "Ljubljana", "country": "SI"}}))
        .unwrap();
        assert_eq!(
            AddressResolve::check(&resolve_request(), &parsed),
            Err("the address is in \"SI\", not HR".to_string())
        );
    }

    #[test]
    fn a_resolved_address_without_a_city_is_refused() {
        let parsed: AddressResolveResponse = serde_json::from_value(json!({"address":
            {"line1": "Ilica 1", "city": "", "country": "HR"}}))
        .unwrap();
        assert_eq!(
            AddressResolve::check(&resolve_request(), &parsed),
            Err("the address has no street or city".to_string())
        );
    }
}
