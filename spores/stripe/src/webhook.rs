//! Stripe's webhooks, `POST /spores/stripe/webhook` (public: Stripe has no Hypha
//! credentials). Verified with the store's signing secret, then each event reported as a
//! payment event in-process (`record_payment_event`, as the spore's app) with an
//! idempotency key per report, so a redelivery never counts twice. Refunds are reported
//! one per Stripe refund, its `re_…` as the reference (Hypha counts each refund once by
//! it), so two `charge.refunded` events, however they race, can't count one twice.
//!
//! Answers: 200 when handled (reported, ignored, or refused for good: retrying won't
//! help), 400 for a request that isn't Stripe's, 500 when Hypha couldn't record it now
//! (Stripe delivers it again).

use hypha_spore::checkout::CommandError as CheckoutError;
use hypha_spore::core::{
    ActingAs, Actor, Clock, Ctx, IdempotencyRequest, PaymentId, Scope, Timestamp,
};
use hypha_spore::http::axum::body::Bytes;
use hypha_spore::http::axum::extract::State;
use hypha_spore::http::axum::http::{HeaderMap, StatusCode};
use hypha_spore::http::{ApiError, SharedState, Storage, spore, utoipa};
use hypha_spore::order_actions::{
    CommandError, PaymentEvent, PaymentEventKind, record_payment_event,
};
use hypha_spore::payments::{PaymentError, PaymentStatus};
use serde::Serialize;
use serde_json::Value;

use crate::amounts::from_stripe;
use crate::api::{Refund, StripeClient};
use crate::{Settings, Stripe, signature};

/// A payment event to record, with the key it's recorded under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub payment: PaymentId,
    pub kind: PaymentEventKind,
    /// In Hypha's minor units, for a refund.
    pub amount: Option<i64>,
    /// The PaymentIntent's ID; for a refund, the refund's (`re_…`).
    pub reference: String,
    /// The PaymentIntent it's about: the attempt core checks it against.
    pub intent: String,
    pub occurred_at: Option<Timestamp>,
    /// The event's ID, or for a refund `stripe-refund:<re_…>`.
    pub key: String,
}

/// What a delivery came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct WebhookOutcome {
    /// `reported`, `ignored` or `rejected`.
    status: &'static str,
    /// Why it was ignored or rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    /// How many payment events it reported.
    reported: usize,
}

impl WebhookOutcome {
    fn ignored(reason: impl Into<String>) -> Self {
        Self {
            status: "ignored",
            reason: Some(reason.into()),
            reported: 0,
        }
    }

    fn rejected(reason: impl Into<String>) -> Self {
        Self {
            status: "rejected",
            reason: Some(reason.into()),
            reported: 0,
        }
    }
}

/// An event's fields used here.
pub struct StripeEvent {
    pub id: String,
    pub kind: String,
    pub created: i64,
    pub object: serde_json::Map<String, Value>,
}

impl StripeEvent {
    pub fn parse(body: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(body).ok()?;
        Some(Self {
            id: value.get("id")?.as_str()?.to_string(),
            kind: value.get("type")?.as_str()?.to_string(),
            created: value.get("created").and_then(Value::as_i64).unwrap_or(0),
            object: value.get("data")?.get("object")?.as_object()?.clone(),
        })
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.object.get(key).and_then(Value::as_str)
    }

    fn occurred_at(&self) -> Option<Timestamp> {
        (self.created > 0).then(|| Timestamp::from_unix_micros(self.created * 1_000_000))
    }

    /// The Hypha payment a PaymentIntent event is about (its metadata).
    fn payment(&self) -> Option<PaymentId> {
        self.object
            .get("metadata")?
            .get("hypha_payment")?
            .as_str()?
            .parse()
            .ok()
    }
}

/// What a PaymentIntent event reports, or why it's ignored. (`charge.refunded` needs
/// Stripe's refunds: `refund_reports`.)
pub fn intent_reports(event: &StripeEvent) -> Result<Vec<Report>, String> {
    let (Some(payment), Some(intent)) = (event.payment(), event.text("id")) else {
        return Err("not a Hypha payment".to_string());
    };
    let report = |kind| Report {
        payment,
        kind,
        amount: None,
        reference: intent.to_string(),
        intent: intent.to_string(),
        occurred_at: event.occurred_at(),
        key: event.id.clone(),
    };
    match event.kind.as_str() {
        "payment_intent.succeeded" => Ok(vec![report(PaymentEventKind::Paid)]),
        // Taken and being collected (SEPA Direct Debit: days; design note 0031, Q3).
        "payment_intent.processing" => Ok(vec![report(PaymentEventKind::Processing)]),
        "payment_intent.amount_capturable_updated" => {
            match event
                .object
                .get("amount_capturable")
                .and_then(Value::as_i64)
            {
                Some(n) if n > 0 => Ok(vec![report(PaymentEventKind::Authorized)]),
                _ => Err("nothing capturable".to_string()),
            }
        }
        // Hypha's `failed` is final; Stripe's attempt usually isn't: a declined intent
        // goes back to `requires_payment_method` and the shopper may try another card.
        // Only an intent that can't take money any more failed for Hypha.
        "payment_intent.payment_failed" => match event.text("status") {
            Some("canceled") => Ok(vec![report(PaymentEventKind::Failed)]),
            _ => Err("the shopper may still pay this intent".to_string()),
        },
        "payment_intent.canceled" => Ok(vec![report(PaymentEventKind::Voided)]),
        other => Err(format!("{other} isn't reported")),
    }
}

/// Every refund of the charge that gave money back (or will: `pending`), oldest first,
/// each under its own ID. Hypha counts each once by that ID, so the ones it has already
/// change nothing; the one that completes a fully refunded charge is `refunded`.
pub fn refund_reports(
    payment: PaymentId,
    intent: &str,
    charge: &serde_json::Map<String, Value>,
    refunds: &[Refund],
) -> Result<Vec<Report>, String> {
    let currency = charge
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let whole = charge.get("amount").and_then(Value::as_i64).unwrap_or(0);
    let refunded = charge.get("refunded").and_then(Value::as_bool) == Some(true);
    let counted: Vec<&Refund> = refunds
        .iter()
        .filter(|r| r.status == "succeeded" || r.status == "pending")
        .collect();
    if counted.is_empty() {
        return Err("no refund gave money back".to_string());
    }
    let mut running = 0;
    Ok(counted
        .iter()
        .enumerate()
        .map(|(i, refund)| {
            running += refund.amount;
            let completes = i == counted.len() - 1 && refunded && running >= whole;
            Report {
                payment,
                kind: match completes {
                    true => PaymentEventKind::Refunded,
                    false => PaymentEventKind::PartiallyRefunded,
                },
                amount: Some(from_stripe(refund.amount, currency)),
                reference: refund.id.clone(),
                intent: intent.to_string(),
                occurred_at: (refund.created > 0)
                    .then(|| Timestamp::from_unix_micros(refund.created * 1_000_000)),
                key: format!("stripe-refund:{}", refund.id),
            }
        })
        .collect())
}

/// What Stripe's webhook reports to the store.
#[utoipa::path(
    post,
    path = "/spores/stripe/webhook",
    request_body(content = String, description = "Stripe's event, as Stripe signed it", content_type = "application/json"),
    responses(
        (status = 200, description = "Handled: reported, ignored, or refused for good", body = WebhookOutcome),
        (status = 400, description = "Not Stripe's: a bad signature, no signing secret set, not an event", body = WebhookOutcome),
        (status = 500, description = "Couldn't be recorded now; Stripe delivers it again", body = WebhookOutcome),
    ),
    security(()),
)]
pub async fn receive<S: Storage>(
    State(state): State<SharedState<S>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, hypha_spore::http::axum::Json<WebhookOutcome>), ApiError> {
    use hypha_spore::http::axum::Json;
    let answer = |status, outcome| Ok((status, Json(outcome)));
    let stripe = spore::<Stripe, S>(&state)?;
    let ports = state.public_ports().await?;
    let settings: Settings = stripe
        .env
        .settings(&ports)
        .await
        .map_err(ApiError::internal)?
        .unwrap_or_default();
    let Some(secret) = settings.webhook_secret.as_deref() else {
        return answer(
            StatusCode::BAD_REQUEST,
            WebhookOutcome::rejected("no signing secret set"),
        );
    };
    let now = stripe.env.now().unix_micros() / 1_000_000;
    let signed = headers.get(signature::HEADER).and_then(|v| v.to_str().ok());
    if !signature::verify(secret, signed, &body, now) {
        return answer(
            StatusCode::BAD_REQUEST,
            WebhookOutcome::rejected("bad signature"),
        );
    }
    let Some(event) = StripeEvent::parse(&body) else {
        return answer(
            StatusCode::BAD_REQUEST,
            WebhookOutcome::rejected("not an event"),
        );
    };

    let reports = if event.kind.starts_with("payment_intent.") {
        intent_reports(&event)
    } else if event.kind == "charge.refunded" {
        let Some(secret_key) = settings.secret_key.as_deref() else {
            return answer(StatusCode::OK, WebhookOutcome::ignored("no secret key set"));
        };
        let Some(intent) = event.text("payment_intent") else {
            return answer(
                StatusCode::OK,
                WebhookOutcome::ignored("a charge without a PaymentIntent"),
            );
        };
        let client = StripeClient {
            http: &stripe.env,
            base: &stripe.api_base,
            key: secret_key,
        };
        let found = async {
            let intent = client.retrieve_payment_intent(intent).await?;
            let refunds = client.list_refunds(&intent.id).await?;
            Ok::<_, crate::api::StripeError>((intent, refunds))
        }
        .await;
        match found {
            // No such intent at Stripe (not one of this store's), or a request Stripe
            // will never take: retrying can't help. A later refund of the charge reports
            // every refund again, each counted once.
            Err(e) if e.is_missing() || (e.status == 400 && e.kind == "invalid_request_error") => {
                Err(e.message)
            }
            // What can pass (Stripe down or rate limited, a key rotated or without
            // refunds:read until the merchant saves the new one): 500, so Stripe delivers
            // it again and the refund isn't lost.
            Err(e) => {
                hypha_spore::tracing::warn!(event = %event.id, "reading a refund at Stripe failed: {}", e.message);
                return answer(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    WebhookOutcome::rejected("Stripe couldn't be read; try again"),
                );
            }
            Ok((intent, refunds)) => match intent
                .metadata
                .get("hypha_payment")
                .and_then(|p| p.parse().ok())
            {
                Some(payment) => refund_reports(payment, &intent.id, &event.object, &refunds),
                None => Err("not a Hypha payment".to_string()),
            },
        }
    } else {
        Err(format!("{} isn't handled", event.kind))
    };
    let reports = match reports {
        Ok(reports) => reports,
        Err(reason) => return answer(StatusCode::OK, WebhookOutcome::ignored(reason)),
    };

    // As the spore's app: its events name it as their actor.
    let app = stripe
        .env
        .app_id(&ports)
        .await
        .map_err(ApiError::internal)?;
    let Some(app) = app else {
        return answer(
            StatusCode::INTERNAL_SERVER_ERROR,
            WebhookOutcome::rejected("the spore's app isn't set up"),
        );
    };
    ports.act_as(Actor::App(app));
    let ctx = Ctx::new(Actor::App(app), vec![Scope::PaymentsWrite]);
    let mut reported = 0;
    let mut refused: Option<String> = None;
    for report in reports {
        let input = PaymentEvent {
            payment: report.payment,
            kind: report.kind,
            amount: report.amount,
            reference: Some(report.reference.clone()),
            attempt: Some(report.intent.clone()),
            occurred_at: report.occurred_at,
            idempotency: Some(IdempotencyRequest {
                fingerprint: fingerprint(&report),
                key: report.key.clone(),
            }),
        };
        // Core refuses a report about another intent than the payment's (it names its
        // attempt), in the transaction that writes: money another intent took for a
        // payment past pending goes back; its decline, cancel or refund isn't the
        // payment's.
        let unwanted = match record_payment_event(&ctx, &ports, &stripe.env, input).await {
            Ok(done) => {
                reported += 1;
                // A cart's money committed: its checkout completes now, though the
                // shopper never came back (design note 0031, §1.5). A storage failure is
                // tried again with this event; a checkout that can't complete gave the
                // money back already.
                match state
                    .complete_paid_checkout(&ports, &ctx, &done.value.payment)
                    .await
                {
                    Ok(_) => {}
                    // Gave the money back (a line sold out, the total changed): done.
                    Err(e) if e.gives_money_back() => {
                        let code = hypha_spore::core::ErrorCode::code(&e);
                        hypha_spore::tracing::warn!(event = %event.id, payment = %report.payment, "the paid checkout couldn't complete, so its money went back: {code}");
                        refused
                            .get_or_insert_with(|| format!("checkout couldn't complete: {code}"));
                    }
                    // Anything that may pass (storage, a shipping provider down, a race):
                    // Stripe delivers the event again, and the checkout is tried again.
                    Err(CheckoutError::Repo(_) | CheckoutError::ShippingUnavailable(_))
                    | Err(CheckoutError::Cart(_) | CheckoutError::Inventory(_)) => {
                        hypha_spore::tracing::warn!(event = %event.id, payment = %report.payment, "completing the checkout failed for now; Stripe will deliver it again");
                        return answer(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            WebhookOutcome::rejected("couldn't complete the checkout; try again"),
                        );
                    }
                    Err(e) => {
                        let code = hypha_spore::core::ErrorCode::code(&e);
                        hypha_spore::tracing::warn!(event = %event.id, payment = %report.payment, "the paid checkout couldn't complete: {code}");
                        refused
                            .get_or_insert_with(|| format!("checkout couldn't complete: {code}"));
                    }
                }
                None
            }
            // Storage failed: Stripe delivers it again, and what went through already
            // changes nothing then.
            Err(CommandError::Repo(e)) => {
                hypha_spore::tracing::warn!(event = %event.id, "recording a Stripe event failed: {e}");
                return answer(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    WebhookOutcome::rejected("couldn't record it; try again"),
                );
            }
            // Taken or held at Stripe after Hypha voided it (the order was cancelled
            // while the shopper confirmed, and the cancel came too late at Stripe).
            Err(CommandError::Payment(PaymentError::InvalidTransition { from, .. }))
                if matches!(
                    report.kind,
                    PaymentEventKind::Paid
                        | PaymentEventKind::Authorized
                        | PaymentEventKind::Processing
                ) && matches!(from, PaymentStatus::Voided | PaymentStatus::Failed) =>
            {
                Some(format!("payment {from}"))
            }
            // Another intent's money for a payment past pending: a second collection.
            Err(CommandError::Payment(PaymentError::ReferenceMismatch { status, .. }))
                if matches!(
                    report.kind,
                    PaymentEventKind::Paid
                        | PaymentEventKind::Authorized
                        | PaymentEventKind::Processing
                ) =>
            {
                Some(format!("payment {status} through another PaymentIntent"))
            }
            // Another intent's decline, cancel or refund: not the payment's.
            Err(CommandError::Payment(PaymentError::ReferenceMismatch { .. })) => {
                refused
                    .get_or_insert_with(|| "another PaymentIntent than the payment's".to_string());
                None
            }
            // A move the payment can't make (voided already, …): retrying won't help.
            Err(e) => {
                let code = hypha_spore::core::ErrorCode::code(&e);
                hypha_spore::tracing::warn!(event = %event.id, payment = %report.payment, "Hypha refused a Stripe report: {code}");
                refused.get_or_insert_with(|| code.to_string());
                None
            }
        };
        // The money goes back (a payment refunded, an authorization released). Not done
        // (Stripe down, no key): 500, so Stripe delivers it again and it's tried again.
        if let Some(why) = unwanted {
            match undo_unwanted(stripe, &settings, &report).await {
                Undo::Done => {
                    hypha_spore::tracing::warn!(
                        event = %event.id,
                        payment = %report.payment,
                        intent = %report.reference,
                        "Stripe took or held money Hypha doesn't want ({why}): given back at Stripe"
                    );
                    refused.get_or_insert_with(|| format!("{why}: given back at Stripe"));
                }
                // Stripe said no (a disputed charge, …): asking again won't change that, so
                // it's acknowledged, and staff give it back by hand.
                Undo::Refused(reason) => {
                    hypha_spore::tracing::error!(
                        event = %event.id,
                        payment = %report.payment,
                        intent = %report.reference,
                        "Stripe took or held money Hypha doesn't want ({why}), and refused to give it back: {reason}; do it in Stripe's dashboard"
                    );
                    refused.get_or_insert_with(|| {
                        format!("{why}: Stripe refused to give it back: {reason}")
                    });
                }
                Undo::Failed => {
                    return answer(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        WebhookOutcome::rejected(
                            "couldn't give the money back at Stripe; try again",
                        ),
                    );
                }
            }
        }
    }
    match (reported, refused) {
        (0, Some(code)) => answer(StatusCode::OK, WebhookOutcome::ignored(code)),
        _ => answer(
            StatusCode::OK,
            WebhookOutcome {
                status: "reported",
                reason: None,
                reported,
            },
        ),
    }
}

/// How giving money back at Stripe went.
#[derive(Debug, PartialEq, Eq)]
enum Undo {
    /// Given back (now, or by an earlier try).
    Done,
    /// Stripe said no, which it will say again: staff must look.
    Refused(String),
    /// Not done for now (Stripe down, no key): worth trying again.
    Failed,
}

/// Gives back what Stripe took or holds for a Hypha payment that's voided or failed (an
/// order cancelled while the shopper confirmed): a payment refunded, an authorization's
/// intent cancelled, once each (keyed by the payment). An intent already canceled or a
/// charge already refunded counts as done.
async fn undo_unwanted(stripe: &Stripe, settings: &Settings, report: &Report) -> Undo {
    let Some(secret_key) = settings.secret_key.as_deref() else {
        return Undo::Failed;
    };
    let client = StripeClient {
        http: &stripe.env,
        base: &stripe.api_base,
        key: secret_key,
    };
    let done = match report.kind {
        PaymentEventKind::Authorized => {
            let key = format!("{}:{}:cancel-after-void", report.payment, report.reference);
            client
                .cancel_payment_intent(&report.reference, &key)
                .await
                .map(|_| ())
        }
        _ => {
            let params = [
                ("payment_intent", report.reference.as_str().into()),
                ("reason", "requested_by_customer".into()),
            ];
            let key = format!("{}:{}:refund-after-void", report.payment, report.reference);
            client.create_refund(&params, &key).await.map(|_| ())
        }
    };
    let e = match done {
        Ok(()) => return Undo::Done,
        // Not a "no" about this money: Stripe down, a key rotated, revoked or without
        // the permission (401, 403: the merchant can fix it), the same key in use by a
        // racing delivery (409, which does it). Worth trying again.
        Err(e) if !e.is_refusal() || matches!(e.status, 401 | 403 | 409) => {
            return Undo::Failed;
        }
        Err(e) => e,
    };
    if e.code.as_deref() == Some("charge_already_refunded") {
        return Undo::Done;
    }
    // A cancel refused because of the intent's state: canceled already is done. Read
    // from the intent itself, not from the wording of Stripe's message.
    if report.kind == PaymentEventKind::Authorized
        && e.code.as_deref() == Some("payment_intent_unexpected_state")
    {
        match client.retrieve_payment_intent(&report.reference).await {
            Ok(intent) if intent.status == "canceled" => return Undo::Done,
            Ok(_) => {}
            Err(e) if !e.is_refusal() => return Undo::Failed,
            Err(_) => {}
        }
    }
    Undo::Refused(e.message)
}

/// What a report says, hashed: a key reused for another report is refused, as an
/// `Idempotency-Key` reused for another body is.
fn fingerprint(report: &Report) -> [u8; 32] {
    use sha2::Digest;
    // Not the intent: a refund's ID names its intent already, an intent report's
    // reference is its intent, and keys recorded before must keep their fingerprints.
    let text = format!(
        "{}|{:?}|{:?}|{}",
        report.payment, report.kind, report.amount, report.reference
    );
    sha2::Sha256::digest(text.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(kind: &str, object: Value) -> StripeEvent {
        StripeEvent::parse(
            json!({ "id": "evt_1", "type": kind, "created": 1_700_000_000, "data": { "object": object } })
                .to_string()
                .as_bytes(),
        )
        .unwrap()
    }

    fn payment() -> PaymentId {
        "pay_01a15646d13071e4b9168f3555dd4cd3".parse().unwrap()
    }

    fn intent(extra: Value) -> Value {
        let mut base =
            json!({ "id": "pi_1", "metadata": { "hypha_payment": payment().to_string() } });
        if let (Value::Object(base), Value::Object(extra)) = (&mut base, extra) {
            base.extend(extra);
        }
        base
    }

    #[test]
    fn a_succeeded_intent_is_paid_under_the_events_key() {
        let reports =
            intent_reports(&event("payment_intent.succeeded", intent(json!({})))).unwrap();
        assert_eq!(
            reports,
            [Report {
                payment: payment(),
                kind: PaymentEventKind::Paid,
                amount: None,
                reference: "pi_1".to_string(),
                intent: "pi_1".to_string(),
                occurred_at: Some(Timestamp::from_unix_micros(1_700_000_000_000_000)),
                key: "evt_1".to_string(),
            }]
        );
    }

    #[test]
    fn a_capturable_intent_is_authorized_and_a_canceled_one_voided() {
        let kinds = |kind, object| {
            intent_reports(&event(kind, object))
                .map(|r| r.into_iter().map(|r| r.kind).collect::<Vec<_>>())
        };
        assert_eq!(
            kinds(
                "payment_intent.amount_capturable_updated",
                intent(json!({ "amount_capturable": 1000 }))
            ),
            Ok(vec![PaymentEventKind::Authorized])
        );
        assert!(
            kinds(
                "payment_intent.amount_capturable_updated",
                intent(json!({ "amount_capturable": 0 }))
            )
            .is_err()
        );
        assert_eq!(
            kinds("payment_intent.canceled", intent(json!({}))),
            Ok(vec![PaymentEventKind::Voided])
        );
    }

    #[test]
    fn a_declined_card_is_not_a_failure_until_the_intent_is_over() {
        assert!(
            intent_reports(&event(
                "payment_intent.payment_failed",
                intent(json!({ "status": "requires_payment_method" }))
            ))
            .is_err()
        );
        assert_eq!(
            intent_reports(&event(
                "payment_intent.payment_failed",
                intent(json!({ "status": "canceled" }))
            ))
            .unwrap()[0]
                .kind,
            PaymentEventKind::Failed
        );
    }

    #[test]
    fn another_stores_or_a_foreign_intent_is_ignored() {
        assert!(
            intent_reports(&event(
                "payment_intent.succeeded",
                json!({ "id": "pi_1", "metadata": {} })
            ))
            .is_err()
        );
    }

    fn refund(id: &str, amount: i64, status: &str, created: i64) -> Refund {
        Refund {
            id: id.to_string(),
            amount,
            status: status.to_string(),
            created,
        }
    }

    #[test]
    fn refunds_are_reported_each_once_and_the_last_of_a_full_refund_is_refunded() {
        let charge = json!({ "amount": 1000, "currency": "eur", "refunded": true });
        let refunds = [
            refund("re_1", 300, "succeeded", 1),
            refund("re_x", 100, "failed", 2),
            refund("re_2", 700, "succeeded", 3),
        ];
        let reports =
            refund_reports(payment(), "pi_1", charge.as_object().unwrap(), &refunds).unwrap();
        let summary: Vec<_> = reports
            .iter()
            .map(|r| (r.kind, r.amount, r.reference.as_str(), r.key.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    PaymentEventKind::PartiallyRefunded,
                    Some(300),
                    "re_1",
                    "stripe-refund:re_1"
                ),
                (
                    PaymentEventKind::Refunded,
                    Some(700),
                    "re_2",
                    "stripe-refund:re_2"
                ),
            ]
        );
    }

    #[test]
    fn a_partial_refund_stays_partial() {
        let charge = json!({ "amount": 1000, "currency": "isk", "refunded": false });
        let reports = refund_reports(
            payment(),
            "pi_1",
            charge.as_object().unwrap(),
            &[refund("re_1", 30000, "succeeded", 1)],
        )
        .unwrap();
        assert_eq!(
            (reports[0].kind, reports[0].amount),
            (PaymentEventKind::PartiallyRefunded, Some(300))
        );
    }
}
