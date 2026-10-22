//! The payment provider `spore_stripe`: what checkout and staff's payment actions call. Its
//! settings arrive in the request (`store_settings`, filled by the host from what the
//! merchant saved).
//!
//! Results don't come back here: Stripe tells the webhook (`webhook.rs`), which records
//! them. A capture Stripe completes is still reported there (`payment_intent.succeeded`):
//! a provider has no ports to record with.

use hypha_spore::SporeEnv;
use hypha_spore::core::Money;
use hypha_spore::payments::{
    ActionAnswer, ActionRequest, NextAction, PaymentAction, PaymentError, PaymentProvider,
    PaymentRequest, ProviderKey, ProviderState, ProviderStatus, Started, StatusRequest,
    no_settings,
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
        if let Some(reference) = intent_of(payment.attempt.as_deref(), payment.reference.as_deref())
        {
            match client.retrieve_payment_intent(reference).await {
                Ok(intent) => return usable(intent),
                // Not at this account (the merchant switched Stripe accounts, or test
                // keys to live ones): nothing to resume, so the payment gets one here.
                // Only Stripe's own "no such object" (an `api_base` that isn't Stripe's
                // must not make intents silently). Money a shopper then
                // pays on the old account's intent isn't seen by Hypha (that account's
                // webhooks are signed with another secret): switching accounts with
                // open payments is for staff to reconcile in the old dashboard.
                Err(e) if e.is_missing() => {}
                Err(e) => return Err(Self::failed(stripe_reason(&e))),
            }
        }
        let key = payment.payment.to_string();
        let params = [
            intent_params(payment, settings.capture_mode)?,
            intent_params(payment, settings.capture_mode.other())?,
        ];
        let intent = match create(&client, &params, &key).await {
            Ok(intent) => intent,
            // Stripe keeps a key's answer for 24 hours, a 500 included, so every start of
            // this payment would get that 500 again. A second key gets past it. Safe: an
            // intent the failed request may have made never reached the shopper (its
            // client secret was in the answer that failed), so it can't be paid.
            Err(e) if e.status >= 500 => create(&client, &params, &format!("{key}:again"))
                .await
                .map_err(|e| Self::failed(stripe_reason(&e)))?,
            Err(e) => return Err(Self::failed(stripe_reason(&e))),
        };
        usable(intent)
    }
}

/// Creates a payment's intent under `key` with `params` (in the merchant's capture mode,
/// then in the other). A key used with other parameters is the merchant having switched
/// the capture mode since the first start: the same request in the other mode is the
/// first one, which Stripe replays with the intent it made then.
async fn create(
    client: &StripeClient<'_, SporeEnv>,
    params: &[Vec<(&'static str, Form)>; 2],
    key: &str,
) -> Result<PaymentIntent, StripeError> {
    match client.create_payment_intent(&params[0], key).await {
        Err(e) if e.kind == "idempotency_error" => {
            client.create_payment_intent(&params[1], key).await
        }
        answer => answer,
    }
}

/// The payment's PaymentIntent: its attempt, else its reference (a payment from before
/// attempts were kept), whichever names one (staff may have reported another attempt;
/// staff's text or an operation's own ID may have replaced the reference).
fn intent_of<'a>(attempt: Option<&'a str>, reference: Option<&'a str>) -> Option<&'a str> {
    let intent = |id: &&str| id.starts_with("pi_");
    attempt.filter(intent).or(reference.filter(intent))
}

/// An intent the shopper can still pay, or why not.
fn usable(intent: PaymentIntent) -> Result<PaymentIntent, PaymentError> {
    match intent.status.as_str() {
        "canceled" => Err(StripeProvider::failed(
            "this payment's PaymentIntent was canceled at Stripe: the payment can't go on",
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
    // A cart's payment has no order yet: its metadata names the cart, and the order is
    // Hypha's once the money is committed.
    let mut metadata: Vec<(&'static str, Form)> = vec![
        ("hypha_payment", payment.payment.to_string().into()),
        ("hypha_cart", payment.cart.to_string().into()),
    ];
    if let Some(order) = payment.order {
        metadata.push(("hypha_order", order.to_string().into()));
    }
    let mut params: Vec<(&'static str, Form)> = vec![
        ("amount", amount.into()),
        ("currency", currency.to_ascii_lowercase().into()),
        ("capture_method", capture.into()),
        (
            "automatic_payment_methods",
            Form::Map(vec![("enabled", "true".into())]),
        ),
        ("receipt_email", payment.customer_email.as_str().into()),
        ("metadata", Form::Map(metadata)),
    ];
    if let Some(number) = payment.order_number {
        params.push(("description", format!("Order {number}").into()));
    }
    Ok(params)
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
            reference: Some(intent.id.clone()),
            attempt: Some(intent.id),
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
        // The attempt the payment collects through, else its reference (a payment from
        // before attempts were kept): staff's text or an operation's own ID may have
        // replaced the reference.
        let Some(reference) = intent_of(request.attempt.as_deref(), request.reference.as_deref())
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
                        Form::Map(
                            [("hypha_payment", request.payment.to_string().into())]
                                .into_iter()
                                .chain(request.order.map(|o| ("hypha_order", o.to_string().into())))
                                .collect(),
                        ),
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

    /// Where the payment's PaymentIntent is: checkout asks when the shopper's browser says
    /// they paid before the webhook arrived.
    async fn status(&self, request: &StatusRequest) -> Result<ProviderStatus, PaymentError> {
        let settings = Settings::from_map(&request.store_settings);
        let Some(secret_key) = &settings.secret_key else {
            return Err(Self::failed(
                "Stripe isn't set up: add the keys in Apps → Stripe",
            ));
        };
        let Some(reference) = intent_of(request.attempt.as_deref(), request.reference.as_deref())
        else {
            return Ok(ProviderStatus {
                state: ProviderState::Open,
                reference: None,
                attempt: None,
            });
        };
        let intent = self
            .client(secret_key)
            .retrieve_payment_intent(reference)
            .await
            .map_err(|e| Self::failed(stripe_reason(&e)))?;
        Ok(ProviderStatus {
            state: intent_state(&intent.status),
            reference: Some(intent.id.clone()),
            attempt: Some(intent.id),
        })
    }
}

/// A PaymentIntent's status as a payment's state: `requires_capture` holds the money,
/// `processing` is collecting it, `succeeded` has it; the `requires_…` steps before
/// wait for the shopper.
fn intent_state(status: &str) -> ProviderState {
    match status {
        "requires_capture" => ProviderState::Authorized,
        "processing" => ProviderState::Processing,
        "succeeded" => ProviderState::Paid,
        "canceled" => ProviderState::Canceled,
        _ => ProviderState::Open,
    }
}
