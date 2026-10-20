//! The provider against a fake Stripe: what it sends, and how it takes the answers.

use std::sync::{Arc, Mutex};

use hypha_spore::Spore;
use hypha_spore::client::{HttpMethod, HttpRequest, HttpResponse};
use hypha_spore::core::{
    ChannelId, CountryCode, Currency, Email, MarketId, Money, OrderId, PaymentId, PaymentMethodId,
};
use hypha_spore::payments::{
    ActionAnswer, ActionRequest, NextAction, PaymentAction, PaymentError, PaymentProvider,
    PaymentRequest,
};
use hypha_spore::testing::{SporeHttp, env_with_http};
use hypha_spore_stripe::Stripe;
use hypha_spore_stripe::provider::{StripeProvider, key};
use pollster::block_on;
use serde_json::{Map, Value, json};

/// A fake Stripe: records each request and answers `answer(request)`.
struct Fake {
    seen: Arc<Mutex<Vec<HttpRequest>>>,
}

impl Fake {
    fn new(
        answer: impl Fn(&HttpRequest) -> (u16, Value) + Send + Sync + 'static,
    ) -> (Self, SporeHttp) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let answer = Arc::new(answer);
        let http = SporeHttp::new(move |request| {
            let (status, body) = answer(&request);
            log.lock().unwrap().push(request);
            Box::pin(async move {
                Ok(HttpResponse {
                    status,
                    content_type: Some("application/json".to_string()),
                    body: body.to_string().into_bytes(),
                })
            })
        });
        (Self { seen }, http)
    }

    fn requests(&self) -> Vec<HttpRequest> {
        self.seen.lock().unwrap().clone()
    }
}

fn provider(http: SporeHttp) -> StripeProvider {
    let stripe = Stripe::new(env_with_http("stripe", http)).unwrap();
    let (provider,) = stripe.payment_providers();
    provider
}

fn settings(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn configured() -> Map<String, Value> {
    settings(json!({
        "secret_key": "sk_test_key",
        "publishable_key": "pk_test_key",
        "capture_mode": "manual"
    }))
}

fn payment_id() -> PaymentId {
    "pay_01a15646d13071e4b9168f3555dd4cd3".parse().unwrap()
}

fn request(store_settings: Map<String, Value>) -> PaymentRequest {
    PaymentRequest {
        payment: payment_id(),
        order: "ord_01a15646d13071e4b9168f3555dd4cd4"
            .parse::<OrderId>()
            .unwrap(),
        order_number: 1001,
        method: "pm_01a15646d13071e4b9168f3555dd4cd5"
            .parse::<PaymentMethodId>()
            .unwrap(),
        amount: Money::new(1250, Currency::EUR).unwrap(),
        channel: "chan_01a15646d13071e4b9168f3555dd4cd6"
            .parse::<ChannelId>()
            .unwrap(),
        market: "mkt_01a15646d13071e4b9168f3555dd4cd7"
            .parse::<MarketId>()
            .unwrap(),
        country: CountryCode::new("HR").unwrap(),
        customer_email: Email::new("buyer@example.com").unwrap(),
        return_url: Some("https://shop.example/order/1".to_string()),
        reference: None,
        instructions: None,
        settings: json!({}),
        store_settings,
    }
}

fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

#[test]
fn a_start_creates_one_intent_keyed_by_the_payment_and_answers_the_element() {
    let (fake, http) = Fake::new(|_| {
        (
            200,
            json!({
                "id": "pi_123", "amount": 1250, "currency": "eur",
                "status": "requires_payment_method", "client_secret": "pi_123_secret"
            }),
        )
    });
    let started = block_on(provider(http).start(&request(configured()))).unwrap();
    assert_eq!(started.reference.as_deref(), Some("pi_123"));
    let NextAction::ClientWidget { element, props } = started.next_action else {
        panic!("expected the element");
    };
    assert_eq!(element, "stripe-payment");
    assert_eq!(props["client_secret"], json!("pi_123_secret"));
    assert_eq!(props["publishable_key"], json!("pk_test_key"));
    assert_eq!(props["payment_id"], json!(payment_id().to_string()));

    let sent = fake.requests();
    assert_eq!(sent.len(), 1);
    let create = &sent[0];
    assert_eq!(create.method, HttpMethod::Post);
    assert!(
        create.url.ends_with("/v1/payment_intents"),
        "{}",
        create.url
    );
    assert_eq!(header(create, "authorization"), Some("Bearer sk_test_key"));
    assert_eq!(
        header(create, "idempotency-key"),
        Some(payment_id().to_string().as_str())
    );
    let body = String::from_utf8(create.body.clone()).unwrap();
    for part in [
        "amount=1250",
        "currency=eur",
        "capture_method=manual",
        "receipt_email=buyer%40example.com",
        "metadata%5Bhypha_payment%5D=pay_",
    ] {
        assert!(body.contains(part), "{part} in {body}");
    }
}

#[test]
fn a_start_without_keys_says_how_to_set_it_up() {
    let (fake, http) = Fake::new(|_| (500, json!({})));
    let result = block_on(provider(http).start(&request(Map::new())));
    assert!(matches!(
        result,
        Err(PaymentError::ProviderFailed { ref reason, .. }) if reason.contains("Apps → Stripe")
    ));
    assert!(fake.requests().is_empty());
}

#[test]
fn a_canceled_intent_is_not_started_again() {
    let (_, http) = Fake::new(|_| {
        (
            200,
            json!({ "id": "pi_1", "amount": 1250, "currency": "eur", "status": "canceled", "client_secret": null }),
        )
    });
    assert!(matches!(
        block_on(provider(http).start(&request(configured()))),
        Err(PaymentError::ProviderFailed { .. })
    ));
}

fn action(kind: PaymentAction, amount: Option<i64>, reference: Option<&str>) -> ActionRequest {
    ActionRequest {
        action: kind,
        payment: payment_id(),
        order: "ord_01a15646d13071e4b9168f3555dd4cd4".parse().unwrap(),
        amount: amount.map(|a| Money::new(a, Currency::EUR).unwrap()),
        reference: reference.map(str::to_string),
        idempotency_key: "attempt-1".to_string(),
        store_settings: configured(),
    }
}

#[test]
fn capture_void_and_refund_call_stripe_under_a_key_per_attempt() {
    let (fake, http) = Fake::new(|request| match request.url.as_str() {
        url if url.contains("/v1/refunds") => (
            200,
            json!({ "id": "re_9", "amount": 500, "status": "succeeded" }),
        ),
        _ => (
            200,
            json!({ "id": "pi_1", "amount": 1250, "currency": "eur", "status": "succeeded", "client_secret": null }),
        ),
    });
    let provider = provider(http);
    // Stripe's answers say the capture and the refund are done: Hypha records them at
    // once. The fake's cancel answers `succeeded`, not `canceled`: only accepted.
    assert_eq!(
        block_on(provider.act(&action(PaymentAction::Capture, None, Some("pi_1")))),
        Ok(ActionAnswer::Completed {
            reference: Some("pi_1".to_string())
        })
    );
    assert_eq!(
        block_on(provider.act(&action(PaymentAction::Refund, Some(500), Some("pi_1")))),
        Ok(ActionAnswer::Completed {
            reference: Some("re_9".to_string())
        })
    );
    assert_eq!(
        block_on(provider.act(&action(PaymentAction::Void, None, Some("pi_1")))),
        Ok(ActionAnswer::Accepted {
            reference: Some("pi_1".to_string())
        })
    );
    let sent = fake.requests();
    let urls: Vec<&str> = sent
        .iter()
        .map(|r| r.url.rsplit("/v1/").next().unwrap())
        .collect();
    assert_eq!(
        urls,
        [
            "payment_intents/pi_1/capture",
            "refunds",
            "payment_intents/pi_1/cancel"
        ]
    );
    assert_eq!(
        header(&sent[1], "idempotency-key"),
        Some(format!("{}:refund:attempt-1", payment_id()).as_str())
    );
    let refund = String::from_utf8(sent[1].body.clone()).unwrap();
    assert!(
        refund.contains("amount=500") && refund.contains("payment_intent=pi_1"),
        "{refund}"
    );
}

#[test]
fn stripes_no_is_a_refusal_staff_see_and_its_outage_a_failure() {
    let (_, http) = Fake::new(|_| {
        (
            400,
            json!({ "error": { "type": "invalid_request_error", "message": "This PaymentIntent could not be captured." } }),
        )
    });
    assert_eq!(
        block_on(provider(http).act(&action(PaymentAction::Capture, None, Some("pi_1")))),
        Ok(ActionAnswer::Refused {
            reason: "This PaymentIntent could not be captured.".to_string()
        })
    );
    let (_, http) = Fake::new(|_| (503, json!({})));
    assert!(matches!(
        block_on(provider(http).act(&action(PaymentAction::Capture, None, Some("pi_1")))),
        Err(PaymentError::ProviderFailed { .. })
    ));
}

#[test]
fn a_payment_without_an_intent_is_refused_without_calling_stripe() {
    let (fake, http) = Fake::new(|_| (200, json!({})));
    assert!(matches!(
        block_on(provider(http).act(&action(PaymentAction::Refund, None, None))),
        Ok(ActionAnswer::Refused { .. })
    ));
    assert!(fake.requests().is_empty());
    assert_eq!(key().to_string(), "spore_stripe");
}

#[test]
fn a_start_after_the_capture_mode_changed_replays_the_first_intent() {
    // Stripe refuses the payment's key with other parameters; the same request in the
    // other mode is the first one, and Stripe answers it with the intent made then.
    let (fake, http) = Fake::new(|request| {
        if String::from_utf8_lossy(&request.body).contains("capture_method=manual") {
            (
                400,
                json!({ "error": { "type": "idempotency_error", "message": "Keys for idempotent requests can only be used with the same parameters" } }),
            )
        } else {
            (
                200,
                json!({ "id": "pi_first", "amount": 1250, "currency": "eur", "status": "requires_payment_method", "client_secret": "pi_first_secret" }),
            )
        }
    });
    let started = block_on(provider(http).start(&request(configured()))).unwrap();
    assert_eq!(started.reference.as_deref(), Some("pi_first"));
    let sent = fake.requests();
    assert_eq!(sent.len(), 2);
    assert_eq!(
        header(&sent[1], "idempotency-key"),
        Some(payment_id().to_string().as_str())
    );
}

#[test]
fn a_payment_started_before_resumes_its_intent() {
    let (fake, http) = Fake::new(|_| {
        (
            200,
            json!({ "id": "pi_kept", "amount": 1250, "currency": "eur", "status": "requires_payment_method", "client_secret": "pi_kept_secret" }),
        )
    });
    let mut again = request(configured());
    again.reference = Some("pi_kept".to_string());
    let started = block_on(provider(http).start(&again)).unwrap();
    assert_eq!(started.reference.as_deref(), Some("pi_kept"));
    let sent = fake.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, HttpMethod::Get);
    assert!(
        sent[0].url.ends_with("/v1/payment_intents/pi_kept"),
        "{}",
        sent[0].url
    );
}

#[test]
fn a_payment_whose_intent_was_canceled_is_not_started_again() {
    let (_, http) = Fake::new(|_| {
        (
            200,
            json!({ "id": "pi_kept", "amount": 1250, "currency": "eur", "status": "canceled", "client_secret": "pi_kept_secret" }),
        )
    });
    let mut again = request(configured());
    again.reference = Some("pi_kept".to_string());
    assert!(matches!(
        block_on(provider(http).start(&again)),
        Err(PaymentError::ProviderFailed { reason, .. }) if reason.contains("was canceled")
    ));
}

#[test]
fn a_create_stripe_failed_is_tried_again_under_a_second_key() {
    // Stripe replays a key's 500 for a day: the second key gets past it.
    let (fake, http) = Fake::new(|request| {
        if header(request, "idempotency-key") == Some(payment_id().to_string().as_str()) {
            (
                500,
                json!({ "error": { "type": "api_error", "message": "Something went wrong" } }),
            )
        } else {
            (
                200,
                json!({ "id": "pi_second", "amount": 1250, "currency": "eur", "status": "requires_payment_method", "client_secret": "pi_second_secret" }),
            )
        }
    });
    let started = block_on(provider(http).start(&request(configured()))).unwrap();
    assert_eq!(started.reference.as_deref(), Some("pi_second"));
    let sent = fake.requests();
    assert_eq!(sent.len(), 2);
    assert_eq!(
        header(&sent[1], "idempotency-key"),
        Some(format!("{}:again", payment_id()).as_str())
    );
}
