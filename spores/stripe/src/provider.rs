//! The payment provider `spore_stripe` (design note 0027, PA5; 0030, SP1): what
//! checkout and staff's payment actions call. Its settings arrive in the request
//! (`store_settings`, filled by the host from what the merchant saved).
//!
//! Results don't come back here: Stripe tells the webhook (`webhook.rs`), which records
//! them. A capture Stripe completes is still reported there (`payment_intent.succeeded`):
//! a provider has no ports to record with.

use hypha_spore::SporeEnv;
use hypha_spore::core::Money;
use hypha_spore::payments::{
    ActionAnswer, ActionRequest, NextAction, PaymentAction, PaymentError, PaymentProvider,
    PaymentRequest, ProviderKey, Started, no_settings,
};
use serde_json::{Map, Value, json};

use crate::amounts::{AmountError, to_stripe};
use crate::api::{Form, PaymentIntent, StripeClient, StripeError};
use crate::{CaptureMode, Settings};

/// The storefront element a payment mounts (`<stripe-payment>`).
pub const ELEMENT_TAG: &str = "stripe-payment";

/// Stripe as a payment provider.
pub struct StripeProvider {
    pub env: SporeEnv,
    pub api_base: String,
}

/// The provider's key, `spore_stripe`.
pub fn key() -> ProviderKey {
    ProviderKey::new("spore_stripe").unwrap_or_else(|_| unreachable!("a spore's key"))
}

impl StripeProvider {
    fn client<'a>(&'a self, secret_key: &'a str) -> StripeClient<'a, SporeEnv> {
        StripeClient {
            http: &self.env,
            base: &self.api_base,
            key: secret_key,
        }
    }

    fn failed(reason: impl Into<String>) -> PaymentError {
        PaymentError::ProviderFailed {
            provider: key(),
            reason: reason.into(),
        }
    }

    /// The PaymentIntent for a payment, one whatever happens. A payment started before
    /// keeps its intent's ID as its reference, so a later start (the shopper back on the
    /// order page, days later) resumes that intent. A first start creates one with the
    /// payment's ID as the idempotency key, so a start asked again before the reference
    /// was kept is answered with the same intent (Stripe keeps keys for 24 hours).
    async fn intent_for(
        &self,
        payment: &PaymentRequest,
        settings: &Settings,
        secret_key: &str,
    ) -> Result<PaymentIntent, PaymentError> {
        let client = self.client(secret_key);
        if let Some(reference) = payment
            .reference
            .as_deref()
            .filter(|r| r.starts_with("pi_"))
        {
            let intent = client
                .retrieve_payment_intent(reference)
                .await
                .map_err(|e| Self::failed(stripe_reason(&e)))?;
            return usable(intent);
        }
        let key = payment.payment.to_string();
        let created = client
            .create_payment_intent(&intent_params(payment, settings.capture_mode)?, &key)
            .await;
        let intent = match created {
            Ok(intent) => intent,
            // The key was used with other parameters: the merchant switched the capture
            // mode since the first start. The same request in the other mode is the
            // first one, which Stripe replays with the intent it made then.
            Err(e) if e.kind == "idempotency_error" => client
                .create_payment_intent(
                    &intent_params(payment, settings.capture_mode.other())?,
                    &key,
                )
                .await
                .map_err(|e| Self::failed(stripe_reason(&e)))?,
            Err(e) => return Err(Self::failed(stripe_reason(&e))),
        };
        usable(intent)
    }
}

/// An intent the shopper can still pay, or why not.
fn usable(intent: PaymentIntent) -> Result<PaymentIntent, PaymentError> {
    match intent.status.as_str() {
        "canceled" => Err(StripeProvider::failed(
            "this payment's PaymentIntent was canceled: start a new payment",
        )),
        _ => Ok(intent),
    }
}

/// What a payment's PaymentIntent is created with: the same for every start of one
/// payment except the capture mode, which the merchant may switch.
fn intent_params(
    payment: &PaymentRequest,
    capture_mode: CaptureMode,
) -> Result<Vec<(&'static str, Form)>, PaymentError> {
    let currency = payment.amount.currency().to_string();
    let amount = to_stripe(payment.amount.amount(), &currency)
        .map_err(|e: AmountError| StripeProvider::failed(e.to_string()))?;
    let capture = match capture_mode {
        CaptureMode::Automatic => "automatic",
        CaptureMode::Manual => "manual",
    };
    Ok(vec![
        ("amount", amount.into()),
        ("currency", currency.to_ascii_lowercase().into()),
        ("capture_method", capture.into()),
        (
            "automatic_payment_methods",
            Form::Map(vec![("enabled", "true".into())]),
        ),
        (
            "description",
            format!("Order {}", payment.order_number).into(),
        ),
        ("receipt_email", payment.customer_email.as_str().into()),
        (
            "metadata",
            Form::Map(vec![
                ("hypha_payment", payment.payment.to_string().into()),
                ("hypha_order", payment.order.to_string().into()),
                ("hypha_order_number", payment.order_number.into()),
            ]),
        ),
    ])
}

/// What staff or a shopper is told when Stripe failed.
fn stripe_reason(e: &StripeError) -> String {
    match e.is_refusal() {
        true => format!("Stripe refused: {}", e.message),
        false => e.message.clone(),
    }
}

impl PaymentProvider for StripeProvider {
    fn key(&self) -> ProviderKey {
        key()
    }

    /// The keys are the spore's settings (sealed, store-wide), so a method takes none.
    fn check_settings(&self, settings: &Value) -> Result<(), PaymentError> {
        no_settings(&key(), settings)
    }

    async fn start(&self, payment: &PaymentRequest) -> Result<Started, PaymentError> {
        let settings = Settings::from_map(&payment.store_settings);
        let (Some(secret_key), Some(publishable_key)) =
            (&settings.secret_key, &settings.publishable_key)
        else {
            return Err(Self::failed(
                "Stripe isn't set up: add the keys in Apps → Stripe",
            ));
        };
        let intent = self.intent_for(payment, &settings, secret_key).await?;
        let mut props = Map::new();
        props.insert("client_secret".to_string(), json!(intent.client_secret));
        props.insert("publishable_key".to_string(), json!(publishable_key));
        props.insert("return_url".to_string(), json!(payment.return_url));
        props.insert("payment_id".to_string(), json!(payment.payment.to_string()));
        Ok(Started {
            next_action: NextAction::ClientWidget {
                element: ELEMENT_TAG.to_string(),
                props,
            },
            reference: Some(intent.id),
        })
    }

    /// Capture, void and refund: Stripe's "no" is a refusal staff see, not a failure.
    async fn act(&self, request: &ActionRequest) -> Result<ActionAnswer, PaymentError> {
        let settings = Settings::from_map(&request.store_settings);
        let Some(secret_key) = &settings.secret_key else {
            return Ok(ActionAnswer::Refused {
                reason: "Stripe isn't set up: add the keys in Apps → Stripe".to_string(),
            });
        };
        let Some(reference) = request
            .reference
            .as_deref()
            .filter(|r| r.starts_with("pi_"))
        else {
            return Ok(ActionAnswer::Refused {
                reason: "This payment has no Stripe PaymentIntent.".to_string(),
            });
        };
        // Stripe keeps a key's first answer, refusals included: Hypha's key is new per
        // staff attempt and repeated on its retries, so a retry replays what Stripe did
        // and a second press isn't answered with the first one's refusal. Stripe's keys
        // are account-wide: the payment and the action make it ours.
        let idempotency_key = format!(
            "{}:{}:{}",
            request.payment, request.action, request.idempotency_key
        );
        let amount =
            |money: Money| to_stripe(money.amount(), &money.currency().to_string()).map(Form::from);
        let stripe = self.client(secret_key);
        // The ID and whether Stripe's answer says it's done (a capture `succeeded`, a cancel
        // `canceled`, a refund `succeeded`): then Hypha records it at once.
        let done: Result<(String, bool), StripeError> = match request.action {
            PaymentAction::Capture => {
                let params = match request.amount.map(amount).transpose() {
                    Ok(Some(form)) => vec![("amount_to_capture", form)],
                    Ok(None) => Vec::new(),
                    Err(e) => {
                        return Ok(ActionAnswer::Refused {
                            reason: e.to_string(),
                        });
                    }
                };
                stripe
                    .capture_payment_intent(reference, &params, &idempotency_key)
                    .await
                    .map(|intent| (intent.id, intent.status == "succeeded"))
            }
            PaymentAction::Void => stripe
                .cancel_payment_intent(reference, &idempotency_key)
                .await
                .map(|intent| (intent.id, intent.status == "canceled")),
            PaymentAction::Refund => {
                let mut params: Vec<(&str, Form)> = vec![
                    ("payment_intent", reference.into()),
                    (
                        "metadata",
                        Form::Map(vec![
                            ("hypha_payment", request.payment.to_string().into()),
                            ("hypha_order", request.order.to_string().into()),
                        ]),
                    ),
                ];
                match request.amount.map(amount).transpose() {
                    Ok(Some(form)) => params.push(("amount", form)),
                    Ok(None) => {}
                    Err(e) => {
                        return Ok(ActionAnswer::Refused {
                            reason: e.to_string(),
                        });
                    }
                }
                stripe
                    .create_refund(&params, &idempotency_key)
                    .await
                    .map(|refund| (refund.id, refund.status == "succeeded"))
            }
        };
        match done {
            Ok((reference, true)) => Ok(ActionAnswer::Completed {
                reference: Some(reference),
            }),
            Ok((reference, false)) => Ok(ActionAnswer::Accepted {
                reference: Some(reference),
            }),
            Err(e) if e.is_refusal() => Ok(ActionAnswer::Refused { reason: e.message }),
            Err(e) => Err(Self::failed(e.message)),
        }
    }
}
