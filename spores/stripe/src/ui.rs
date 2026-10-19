//! The admin's view of a Stripe payment (design note 0030, §7): an `order.payment`
//! block with Stripe's status, the amounts and a link to the dashboard, and Capture,
//! Void and Refund. Those three stand for the payment points (`point`), so admins run
//! them through the payment's own routes (`POST /admin/payments/{id}/capture`, …), which
//! reach the provider, and show each only when the payment's status allows.

use hypha_spore::core::{Money, PaymentId};
use hypha_spore::payments::{HasPayments, Payment, PaymentRepo};
use hypha_spore::settings::HasSporeSettings;
use hypha_spore::ui::{SporeUi, Target, UiAction, UiBlock, UiCall, UiElement, UiError};
use serde_json::{Value, json};

use crate::amounts::from_stripe;
use crate::api::{StripeClient, dashboard_url};
use crate::provider::{ELEMENT_TAG, key};
use crate::{Settings, Stripe};

/// The block's ID.
pub const BLOCK: &str = "payment";

/// `12.50 EUR`, in the currency's own minor units.
pub fn format_money(money: Money) -> String {
    let digits = money.currency().minor_units();
    let amount = money.amount();
    let code = money.currency();
    if digits == 0 {
        return format!("{amount} {code}");
    }
    let scale = 10_i64.pow(u32::from(digits));
    let sign = if amount < 0 { "-" } else { "" };
    let abs = amount.unsigned_abs();
    let scale = scale.unsigned_abs();
    format!(
        "{sign}{}.{:0width$} {code}",
        abs / scale,
        abs % scale,
        width = usize::from(digits)
    )
}

fn card(children: Vec<Value>) -> Value {
    json!({ "version": 1, "root": { "type": "card", "title": "Stripe", "children": children } })
}

fn note(text: &str) -> Value {
    card(vec![
        json!({ "type": "text", "value": text, "variant": "supporting" }),
    ])
}

/// How a PaymentIntent's status reads, and its badge's tone.
fn status_badge(status: &str) -> (&str, &'static str) {
    match status {
        "requires_payment_method" => ("Awaiting payment", "neutral"),
        "requires_confirmation" => ("Awaiting confirmation", "neutral"),
        "requires_action" => ("Needs the buyer's action", "warning"),
        "processing" => ("Processing", "info"),
        "requires_capture" => ("Authorized", "info"),
        "canceled" => ("Canceled", "neutral"),
        "succeeded" => ("Succeeded", "success"),
        other => (other, "neutral"),
    }
}

impl<P: HasPayments + HasSporeSettings> SporeUi<P> for Stripe {
    fn blocks() -> Vec<UiBlock> {
        vec![UiBlock {
            id: BLOCK,
            target: Target::OrderPayment,
        }]
    }

    fn actions() -> Vec<UiAction> {
        let action = |id, label, point| UiAction {
            id,
            target: Target::Payment,
            label,
            input_schema: None,
            point: Some(point),
        };
        vec![
            action("capture", "Capture", "payments.capture"),
            action("void", "Void", "payments.void"),
            action("refund", "Refund", "payments.refund"),
        ]
    }

    fn elements() -> Vec<UiElement> {
        vec![UiElement {
            id: "payment",
            target: Target::CheckoutPayment,
            tag: ELEMENT_TAG,
            path: crate::element::PATH,
        }]
    }

    async fn block(&self, id: &str, call: UiCall<'_>, ports: &P) -> Result<Value, UiError> {
        if id != BLOCK {
            return Err(UiError::NotFound);
        }
        let Some(order) = call.context.order else {
            return Ok(note("No order given."));
        };
        let payment = self.payment(ports, order, call.context.payment).await?;
        let Some(payment) = payment else {
            return Ok(note("This order has no Stripe payment."));
        };
        let Some(intent) = payment.reference().filter(|r| r.starts_with("pi_")) else {
            return Ok(note("Not started at Stripe yet."));
        };
        let settings: Settings = self
            .env
            .settings(ports)
            .await
            .map_err(|e| UiError::Failed(e.to_string()))?
            .unwrap_or_default();
        let Some(secret_key) = settings.secret_key.as_deref() else {
            return Ok(card(vec![json!({
                "type": "banner", "tone": "warning",
                "title": "Can't show the Stripe payment",
                "description": "Add the Stripe keys in Apps → Stripe."
            })]));
        };
        let stripe = StripeClient {
            http: &self.env,
            base: &self.api_base,
            key: secret_key,
        };
        let found = match stripe.retrieve_payment_intent(intent).await {
            Ok(found) => found,
            Err(e) => {
                return Ok(card(vec![json!({
                    "type": "banner", "tone": "warning",
                    "title": "Can't show the Stripe payment",
                    "description": format!("Stripe: {}", e.message)
                })]));
            }
        };
        let (label, tone) = status_badge(&found.status);
        let currency = payment.amount().currency();
        let money = |stripe_amount: i64| {
            Money::new(from_stripe(stripe_amount, &currency.to_string()), currency)
                .map(format_money)
                .unwrap_or_default()
        };
        let mut items = vec![json!({ "label": "Amount", "value": money(found.amount) })];
        if found.amount_received > 0 {
            items.push(json!({ "label": "Received", "value": money(found.amount_received) }));
        }
        if payment.refunded().amount() > 0 {
            items.push(json!({ "label": "Refunded", "value": format_money(payment.refunded()) }));
        }
        let capture = match found.capture_method.as_deref() {
            Some("manual") => "Manual",
            _ => "Automatic",
        };
        items.push(json!({ "label": "Capture", "value": capture }));
        items.push(json!({ "label": "PaymentIntent", "value": found.id }));
        let mut children = vec![
            json!({
                "type": "row", "align": "between",
                "children": [
                    { "type": "text", "value": "Status", "weight": "semibold" },
                    { "type": "badge", "value": label, "tone": tone }
                ]
            }),
            json!({ "type": "keyvalue", "items": items }),
            json!({
                "type": "link", "label": "Open in Stripe",
                "href": dashboard_url(&found.id, found.livemode || settings.is_live()),
                "external": true
            }),
        ];
        let mut buttons = Vec::new();
        if found.status == "requires_capture" {
            buttons.push(json!({ "type": "button", "label": "Capture", "action": "capture", "variant": "primary" }));
            buttons.push(json!({ "type": "button", "label": "Void", "action": "void", "variant": "secondary" }));
        }
        if found.status == "succeeded" && payment.refunded().amount() < payment.amount().amount() {
            buttons.push(json!({ "type": "button", "label": "Refund", "action": "refund", "variant": "destructive" }));
        }
        if !buttons.is_empty() {
            children.push(json!({ "type": "row", "gap": 2, "children": buttons }));
        }
        Ok(card(children))
    }
}

impl Stripe {
    /// The block's payment: the one named, when it's this order's and Stripe's; else the
    /// order's first Stripe payment.
    async fn payment<P: HasPayments>(
        &self,
        ports: &P,
        order: hypha_spore::core::OrderId,
        named: Option<PaymentId>,
    ) -> Result<Option<Payment>, UiError> {
        let failed = |e: hypha_spore::core::RepoError| UiError::Failed(e.to_string());
        let ours = |p: &Payment| p.order() == order && p.provider() == &key();
        if let Some(id) = named {
            return Ok(ports.payments().get(id).await.map_err(failed)?.filter(ours));
        }
        Ok(ports
            .payments()
            .list_for_order(order)
            .await
            .map_err(failed)?
            .into_iter()
            .find(ours))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hypha_spore::core::Currency;

    #[test]
    fn money_reads_in_its_currencys_minor_units() {
        let eur = Money::new(1250, Currency::EUR).unwrap();
        assert_eq!(format_money(eur), "12.50 EUR");
        let jpy = Money::new(500, Currency::new("JPY").unwrap()).unwrap();
        assert_eq!(format_money(jpy), "500 JPY");
    }
}
