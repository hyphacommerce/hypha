//! `payments.start`, `payments.capture`, `payments.void` and `payments.refund` v1
//! (design note 0029, §3.1): how a payment app collects an order's money. Results the
//! app learns later (a card authorized, a refund settled) come back through
//! `POST /admin/payments/{id}/events`, not through these answers.

use core::time::Duration;

use hypha_core::{
    ChannelId, CountryCode, Email, MarketId, Money, OrderId, PaymentId, PaymentMethodId, Scope,
};
use serde::{Deserialize, Serialize};

use crate::{ExtensionPoint, OnFailure};

/// Checkout starts a payment with a method whose provider is the app.
pub struct PaymentsStart;

impl ExtensionPoint for PaymentsStart {
    const NAME: &'static str = "payments.start";
    const VERSION: u16 = 1;
    const TIMEOUT: Duration = Duration::from_secs(15);
    /// `app_unavailable`: the order isn't placed.
    const ON_FAILURE: OnFailure = OnFailure::FailClosed;
    /// A payment app reports what happened through `POST /admin/payments/{id}/events`,
    /// which takes `payments:write`: without it, it can't do its job.
    const SCOPE: Scope = Scope::PaymentsWrite;
    type Request = PaymentStartRequest;
    type Response = PaymentStartResponse;

    /// A redirect goes to `https://` (or `http://localhost`, for development), and a
    /// widget names a custom element (a dash in it, as HTML requires).
    fn check(_: &PaymentStartRequest, response: &PaymentStartResponse) -> Result<(), String> {
        match &response.next_action {
            NextAction::Redirect { url } => {
                let local = url.starts_with("http://localhost") || url.starts_with("http://127.");
                if url.starts_with("https://") || local {
                    Ok(())
                } else {
                    Err(format!("the redirect {url:?} isn't https"))
                }
            }
            NextAction::ClientWidget { element, .. } => {
                let valid = element.contains('-')
                    && element.starts_with(|c: char| c.is_ascii_lowercase())
                    && element
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-._".contains(c));
                if valid {
                    Ok(())
                } else {
                    Err(format!("{element:?} isn't a custom element's name"))
                }
            }
            NextAction::Instructions { .. } => Ok(()),
        }
    }
}

/// What Hypha sends to start a payment. Settings for the method stay with the app (it
/// keeps its own, design note 0029, §4.2): the method's ID is enough to find them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PaymentStartRequest {
    /// The payment Hypha created for this attempt (`pay_…`), also the call's
    /// `Idempotency-Key`: a retried start must not charge twice.
    pub payment: PaymentId,
    pub order: OrderId,
    /// The order's human number (`1001`), for the provider's statement.
    pub order_number: i64,
    /// The payment method the buyer chose (`pm_…`), whose provider is this app.
    pub method: PaymentMethodId,
    /// What to collect: the order's total, in its currency.
    pub amount: Money,
    /// Where the order was placed (its sales channel), to whom (its market) and
    /// shipped to (its country).
    pub channel: ChannelId,
    pub market: MarketId,
    pub country: CountryCode,
    /// Where the storefront wants the buyer back after a redirect, when it said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub return_url: Option<String>,
    /// The buyer's email (for the provider's receipt). Personal data (design note 0029,
    /// AP10): absent unless the app's grant has `customer_data:email`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub customer_email: Option<Email>,
}

/// The app's answer to a start: what the buyer does next.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PaymentStartResponse {
    pub next_action: NextAction,
    /// The provider's ID for the payment (a PaymentIntent's `pi_…`), kept on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub reference: Option<String>,
}

/// What a storefront does after starting a payment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NextAction {
    /// Send the buyer to the provider's page.
    Redirect { url: String },
    /// Mount the app's storefront element (a custom element, design note 0029, §7)
    /// with these properties (a Stripe client secret, …), and wait for its
    /// `hypha:payment-confirmed` event.
    ClientWidget {
        element: String,
        /// Any JSON object: the element's own properties.
        #[cfg_attr(
            feature = "openapi",
            schema(value_type = std::collections::HashMap<String, serde_json::Value>)
        )]
        props: serde_json::Map<String, serde_json::Value>,
    },
    /// Show these words (a bank transfer's details); the money comes later.
    Instructions { text: String },
}

/// Staff capture an authorized payment.
pub struct PaymentsCapture;
/// Staff void a payment that wasn't captured.
pub struct PaymentsVoid;
/// Staff refund some or all of a captured payment.
pub struct PaymentsRefund;

macro_rules! payment_action {
    ($point:ident, $name:literal) => {
        impl ExtensionPoint for $point {
            const NAME: &'static str = $name;
            const VERSION: u16 = 1;
            const TIMEOUT: Duration = Duration::from_secs(15);
            /// `app_unavailable`: nothing changes on the payment.
            const ON_FAILURE: OnFailure = OnFailure::FailClosed;
            const SCOPE: Scope = Scope::PaymentsWrite;
            type Request = PaymentActionRequest;
            type Response = PaymentActionResponse;
        }
    };
}

payment_action!(PaymentsCapture, "payments.capture");
payment_action!(PaymentsVoid, "payments.void");
payment_action!(PaymentsRefund, "payments.refund");

/// What Hypha sends to capture, void or refund a payment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PaymentActionRequest {
    /// The payment (`pay_…`).
    pub payment: PaymentId,
    pub order: OrderId,
    /// How much: for a capture or refund, at most what's left; absent for the whole
    /// (a void never has one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub amount: Option<Money>,
    /// The provider's ID for the payment, as the app gave it on start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub reference: Option<String>,
    /// The attempt the payment collects through, when the app's reports named one
    /// (`attempt` on `POST /admin/payments/{id}/events`): what to act on, where a
    /// capture's own ID may have replaced `reference`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub attempt: Option<String>,
}

/// Whether the provider took the action. Accepted isn't done: the payment moves when
/// the app reports the result (`POST /admin/payments/{id}/events`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PaymentActionResponse {
    Accepted {
        /// The provider's ID for this capture or refund, when it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "openapi", schema(required = false))]
        reference: Option<String>,
    },
    /// The provider said no; `reason` is shown to staff.
    Refused { reason: String },
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn start() -> PaymentStartRequest {
        PaymentStartRequest {
            payment: PaymentId::from_uuid(Uuid::from_u128(1)),
            order: OrderId::from_uuid(Uuid::from_u128(2)),
            order_number: 1001,
            method: PaymentMethodId::from_uuid(Uuid::from_u128(3)),
            amount: Money::new(2700, hypha_core::Currency::EUR).unwrap(),
            channel: ChannelId::from_uuid(Uuid::from_u128(4)),
            market: MarketId::from_uuid(Uuid::from_u128(5)),
            country: CountryCode::new("HR").unwrap(),
            return_url: None,
            customer_email: None,
        }
    }

    fn answer(value: serde_json::Value) -> PaymentStartResponse {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn a_start_without_personal_data_leaves_the_email_out() {
        let sent = serde_json::to_value(start()).unwrap();
        assert_eq!(sent.get("customer_email"), None);
        assert_eq!(sent["amount"], json!({"amount": 2700, "currency": "EUR"}));
        assert_eq!(sent["payment"], "pay_00000000000000000000000000000001");
    }

    #[test]
    fn next_actions_are_tagged_by_type() {
        let widget = answer(json!({
            "next_action": {"type": "client_widget", "element": "stripe-payment",
                            "props": {"client_secret": "pi_1_secret"}},
            "reference": "pi_1"
        }));
        assert_eq!(widget.reference.as_deref(), Some("pi_1"));
        assert!(matches!(
            widget.next_action,
            NextAction::ClientWidget { .. }
        ));
        assert_eq!(PaymentsStart::check(&start(), &widget), Ok(()));
        let text = answer(json!({"next_action": {"type": "instructions", "text": "IBAN"}}));
        assert_eq!(
            text.next_action,
            NextAction::Instructions {
                text: "IBAN".to_string()
            }
        );
    }

    #[test]
    fn a_redirect_must_be_https() {
        let plain =
            answer(json!({"next_action": {"type": "redirect", "url": "http://pay.example"}}));
        assert_eq!(
            PaymentsStart::check(&start(), &plain),
            Err("the redirect \"http://pay.example\" isn't https".to_string())
        );
        let secure =
            answer(json!({"next_action": {"type": "redirect", "url": "https://pay.example"}}));
        assert_eq!(PaymentsStart::check(&start(), &secure), Ok(()));
    }

    #[test]
    fn a_widget_must_be_a_custom_element() {
        let bad = answer(
            json!({"next_action": {"type": "client_widget", "element": "div",
                                                 "props": {}}}),
        );
        assert_eq!(
            PaymentsStart::check(&start(), &bad),
            Err("\"div\" isn't a custom element's name".to_string())
        );
    }

    #[test]
    fn widget_props_are_an_object() {
        let parsed: Result<PaymentStartResponse, _> = serde_json::from_value(json!({
            "next_action": {"type": "client_widget", "element": "x-pay", "props": [1]}
        }));
        assert!(parsed.is_err());
    }

    #[test]
    fn action_answers_are_tagged_by_status() {
        let accepted: PaymentActionResponse =
            serde_json::from_value(json!({"status": "accepted"})).unwrap();
        assert_eq!(
            accepted,
            PaymentActionResponse::Accepted { reference: None }
        );
        let refused: PaymentActionResponse =
            serde_json::from_value(json!({"status": "refused", "reason": "expired"})).unwrap();
        assert_eq!(
            refused,
            PaymentActionResponse::Refused {
                reason: "expired".to_string()
            }
        );
        assert_eq!(
            (
                PaymentsCapture::NAME,
                PaymentsVoid::NAME,
                PaymentsRefund::NAME
            ),
            ("payments.capture", "payments.void", "payments.refund")
        );
    }
}
