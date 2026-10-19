//! An order cancelled cancels its open PaymentIntents at Stripe (`order.cancelled`), so
//! a shopper can't pay an order that's gone and an authorization isn't left holding the
//! buyer's money. Hypha voids the order's pending payments itself; this tells Stripe.
//! Delivered at least once, so it's idempotent: an intent that's already canceled or
//! paid is left as it is.

use hypha_spore::SporeEnv;
use hypha_spore::core::{Event, EventHandler, OrderId, StoredEvent};
use hypha_spore::orders::OrderCancelled;
use hypha_spore::payments::{HasPayments, PaymentRepo};
use hypha_spore::settings::HasSporeSettings;

use crate::Settings;
use crate::api::StripeClient;
use crate::provider::key;

/// The handler.
pub struct CancelIntents {
    pub env: SporeEnv,
    pub api_base: String,
}

/// The PaymentIntent statuses Stripe still lets cancel.
const CANCELABLE: [&str; 4] = [
    "requires_payment_method",
    "requires_confirmation",
    "requires_action",
    "requires_capture",
];

impl<P: HasPayments + HasSporeSettings> EventHandler<P> for CancelIntents {
    fn name(&self) -> &'static str {
        "spore_stripe_cancel_intents"
    }

    fn wants(&self, event_type: &str) -> bool {
        event_type == OrderCancelled::TYPE
    }

    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String> {
        let order = OrderId::from_uuid(event.subject_id);
        let settings: Settings = self
            .env
            .settings(ports)
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        // Not set up: nothing was ever started at Stripe.
        let Some(secret_key) = settings.secret_key.as_deref() else {
            return Ok(());
        };
        let stripe = StripeClient {
            http: &self.env,
            base: &self.api_base,
            key: secret_key,
        };
        let payments = ports
            .payments()
            .list_for_order(order)
            .await
            .map_err(|e| e.to_string())?;
        for payment in payments.iter().filter(|p| p.provider() == &key()) {
            let Some(intent) = payment.reference().filter(|r| r.starts_with("pi_")) else {
                continue;
            };
            let found = match stripe.retrieve_payment_intent(intent).await {
                Ok(found) => found,
                // Gone at Stripe (another account's keys now): nothing to cancel.
                Err(e) if e.is_refusal() => continue,
                Err(e) => return Err(e.message),
            };
            if !CANCELABLE.contains(&found.status.as_str()) {
                continue;
            }
            let idempotency_key = format!("{}:cancel-with-order", payment.id());
            match stripe.cancel_payment_intent(intent, &idempotency_key).await {
                Ok(_) => {}
                // Paid or canceled meanwhile: Stripe's webhook reports what happened.
                Err(e) if e.is_refusal() => {}
                Err(e) => return Err(e.message),
            }
        }
        Ok(())
    }
}
