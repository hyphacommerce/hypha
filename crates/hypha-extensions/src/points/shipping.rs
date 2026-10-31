//! `shipping.rates` v1: the shipping rate callback, unchanged on the wire.
//! These types are that callback's body and answer, moved here from the HTTP host.

use core::time::Duration;

use hypha_core::{Address, CountryCode, Currency, Money, Scope, VariantId};
use serde::{Deserialize, Serialize};

use crate::{ExtensionPoint, OnFailure};

/// Prices a cart's shipping group with an app's rates.
pub struct ShippingRates;

impl ExtensionPoint for ShippingRates {
    const NAME: &'static str = "shipping.rates";
    const VERSION: u16 = 1;
    /// A provider's `timeout_ms` (100 to 10000, 3000 by default) takes its place.
    const TIMEOUT: Duration = Duration::from_secs(5);
    /// The app's options are hidden (`provider_failed`); a zone's backups show when no
    /// regular option is left.
    const ON_FAILURE: OnFailure = OnFailure::Fallback;
    /// The rate settings it answers for are shipping's. The destination's street and
    /// postal code are personal data besides (`customer_data:address`).
    const SCOPE: Scope = Scope::ShippingRead;
    const READ_ONLY: bool = true;
    type Request = RateCallbackRequest;
    type Response = RatesResponse;

    /// Amounts aren't negative, are in the request's currency, and estimates don't end
    /// before they start.
    fn check(request: &RateCallbackRequest, response: &RatesResponse) -> Result<(), String> {
        let currency = request.request.currency;
        for rate in &response.rates {
            if rate.amount.amount() < 0 {
                return Err(format!("the rate for {:?} is negative", rate.service));
            }
            if rate.amount.currency() != currency {
                return Err(format!(
                    "the rate for {:?} is in {}, not {currency}",
                    rate.service,
                    rate.amount.currency()
                ));
            }
            if let Some(e) = &rate.estimate
                && e.min_days > e.max_days
            {
                return Err(format!(
                    "the estimate for {:?} ends before it starts",
                    rate.service
                ));
            }
        }
        Ok(())
    }
}

/// What Hypha sends: `POST <callback_url>` with this body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RateCallbackRequest {
    /// `rreq_…`, also sent as `Hypha-Request-Id` and `Idempotency-Key`: for the app's
    /// logs and de-duplication.
    pub id: String,
    /// When Hypha sent it (RFC 3339, UTC).
    pub sent_at: String,
    pub request: RateRequestBody,
}

/// One shipping group to price.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RateRequestBody {
    /// The cart's currency: every rate must be in it.
    pub currency: Currency,
    /// The address of the location that would ship the group, when it has one.
    pub origin: Option<PostalAddress>,
    pub destination: PostalAddress,
    pub items: Vec<RateItemBody>,
    /// The group's lines, as priced.
    pub subtotal: Money,
}

/// One line of the group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RateItemBody {
    pub variant: VariantId,
    pub sku: Option<String>,
    pub quantity: u32,
    pub unit_price: Money,
    /// One unit's weight, when the variant has one.
    pub weight_grams: Option<u32>,
}

/// A postal address as Hypha sends it to an app. Personal fields (names, phone, street,
/// postal code, city, province) are left out without the app's `customer_data:name` /
/// `:phone` / `:address` scopes and listed in `redacted`: the ones every address has
/// (`last_name`, `line1`, `city`) are then absent, the optional ones `null`. Country and
/// company always come.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "openapi",
    derive(utoipa::ToSchema),
    schema(as = PostalAddressResponse)
)]
pub struct PostalAddress {
    pub first_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    pub company: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line1: Option<String>,
    pub line2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub country: CountryCode,
    pub phone: Option<String>,
    /// The personal fields left out for this app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted: Option<Vec<String>>,
}

impl From<&Address> for PostalAddress {
    fn from(a: &Address) -> Self {
        let p = a.parts();
        Self {
            first_name: p.first_name.clone(),
            last_name: Some(p.last_name.clone()),
            company: p.company.clone(),
            line1: Some(p.line1.clone()),
            line2: p.line2.clone(),
            city: Some(p.city.clone()),
            province: p.province.clone(),
            postal_code: p.postal_code.clone(),
            country: a.country(),
            phone: p.phone.clone(),
            redacted: None,
        }
    }
}

/// What the app answers with a 2xx.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RatesResponse {
    /// The services it can ship the group with; empty when none.
    pub rates: Vec<QuotedRate>,
}

/// One rate the app quotes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct QuotedRate {
    /// The service code a shipping option names (`express`).
    pub service: String,
    pub name: String,
    /// Not negative, in the request's currency.
    pub amount: Money,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub estimate: Option<RateEstimate>,
    /// The app's quote reference, copied to the order's shipping line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub reference: Option<String>,
}

/// Business days from shipping to delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RateEstimate {
    pub min_days: u16,
    /// At least `min_days`.
    pub max_days: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> RateCallbackRequest {
        let money = |amount| Money::new(amount, Currency::EUR).unwrap();
        RateCallbackRequest {
            id: "rreq_1".to_string(),
            sent_at: "2026-10-15T10:00:00Z".to_string(),
            request: RateRequestBody {
                currency: Currency::EUR,
                origin: None,
                destination: PostalAddress {
                    first_name: None,
                    last_name: Some("Horvat".to_string()),
                    company: None,
                    line1: Some("Ilica 1".to_string()),
                    line2: None,
                    city: Some("Zagreb".to_string()),
                    province: None,
                    postal_code: None,
                    country: CountryCode::new("HR").unwrap(),
                    phone: None,
                    redacted: None,
                },
                items: vec![RateItemBody {
                    variant: VariantId::from_uuid(uuid::Uuid::from_u128(1)),
                    sku: None,
                    quantity: 2,
                    unit_price: money(1000),
                    weight_grams: Some(350),
                }],
                subtotal: money(2000),
            },
        }
    }

    fn rates(json: &str) -> RatesResponse {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn the_request_keeps_the_callbacks_wire_format() {
        // The rate callback's body, field for field and in that order.
        assert_eq!(
            serde_json::to_string(&request()).unwrap(),
            r#"{"id":"rreq_1","sent_at":"2026-10-15T10:00:00Z","request":{"currency":"EUR","origin":null,"destination":{"first_name":null,"last_name":"Horvat","company":null,"line1":"Ilica 1","line2":null,"city":"Zagreb","province":null,"postal_code":null,"country":"HR","phone":null},"items":[{"variant":"var_00000000000000000000000000000001","sku":null,"quantity":2,"unit_price":{"amount":1000,"currency":"EUR"},"weight_grams":350}],"subtotal":{"amount":2000,"currency":"EUR"}}}"#
        );
    }

    #[test]
    fn an_answer_may_leave_out_the_estimate_and_reference() {
        let parsed = rates(
            r#"{"rates":[{"service":"post","name":"Post","amount":{"amount":300,"currency":"EUR"}}]}"#,
        );
        assert_eq!(parsed.rates[0].estimate, None);
        assert_eq!(parsed.rates[0].reference, None);
        assert_eq!(ShippingRates::check(&request(), &parsed), Ok(()));
    }

    #[test]
    fn a_negative_rate_is_refused() {
        let parsed = rates(
            r#"{"rates":[{"service":"x","name":"X","amount":{"amount":-1,"currency":"EUR"}}]}"#,
        );
        assert_eq!(
            ShippingRates::check(&request(), &parsed),
            Err("the rate for \"x\" is negative".to_string())
        );
    }

    #[test]
    fn a_free_rate_delivered_on_one_day_is_fine() {
        let parsed = rates(
            r#"{"rates":[{"service":"x","name":"X","amount":{"amount":0,"currency":"EUR"},"estimate":{"min_days":2,"max_days":2}}]}"#,
        );
        assert_eq!(ShippingRates::check(&request(), &parsed), Ok(()));
    }

    #[test]
    fn a_rate_in_another_currency_is_refused() {
        let parsed = rates(
            r#"{"rates":[{"service":"x","name":"X","amount":{"amount":1,"currency":"USD"}}]}"#,
        );
        assert_eq!(
            ShippingRates::check(&request(), &parsed),
            Err("the rate for \"x\" is in USD, not EUR".to_string())
        );
    }

    #[test]
    fn an_estimate_ending_before_it_starts_is_refused() {
        let parsed = rates(
            r#"{"rates":[{"service":"x","name":"X","amount":{"amount":1,"currency":"EUR"},"estimate":{"min_days":3,"max_days":2}}]}"#,
        );
        assert_eq!(
            ShippingRates::check(&request(), &parsed),
            Err("the estimate for \"x\" ends before it starts".to_string())
        );
    }
}
