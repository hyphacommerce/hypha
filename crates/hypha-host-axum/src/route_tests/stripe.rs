//! A host built with the Stripe spore (design note 0030, SP9), against a fake Stripe on
//! localhost: settings, the cart's payment started before its order and checkout
//! completing once Stripe holds the money (design note 0031), from the shopper or from
//! Stripe's webhook, the storefront element, staff's capture and refund, the admin block,
//! and money a replaced or second intent took given back.

use std::collections::BTreeMap;

use axum::body::Bytes;
use axum::http::{HeaderMap, Method as HttpMethod, Uri};
use hypha_spore_stripe::{Stripe, signature, stripe_routes};

use super::*;

crate::deployment! { enum WithStripe {
    spores: [Stripe],
    routes: [stripe_routes],
} }

const WHSEC: &str = "whsec_test_secret";

/// What the fake Stripe holds: each PaymentIntent by ID, the refunds, and every request.
#[derive(Default)]
struct FakeStripe {
    intents: BTreeMap<String, Value>,
    refunds: Vec<Value>,
    /// `(method, path, idempotency key, form body)`.
    seen: Vec<(String, String, String, String)>,
}

type Shared = Arc<Mutex<FakeStripe>>;

/// A fake of the parts of Stripe's API the spore calls, listening on localhost.
async fn fake_stripe() -> (String, Shared) {
    let state: Shared = Arc::default();
    let shared = Arc::clone(&state);
    let app = Router::new().fallback(
        move |method: HttpMethod, uri: Uri, headers: HeaderMap, body: Bytes| {
            let shared = Arc::clone(&shared);
            async move {
                let form = String::from_utf8_lossy(&body).to_string();
                let key = headers
                    .get("idempotency-key")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                let path = uri.path().to_string();
                let mut fake = shared.lock().unwrap();
                fake.seen
                    .push((method.to_string(), path.clone(), key.clone(), form.clone()));
                let field = |name: &str| {
                    form.split('&')
                        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
                        .unwrap_or_default()
                        .to_string()
                };
                let parts: Vec<&str> = path.trim_start_matches("/v1/").split('/').collect();
                let (status, answer) = match (method.as_str(), parts.as_slice()) {
                    ("POST", ["payment_intents"]) => {
                        // One intent per idempotency key, as Stripe does.
                        let existing = fake
                            .intents
                            .values()
                            .find(|i| i["idempotency_key"] == json!(key))
                            .cloned();
                        let intent = existing.unwrap_or_else(|| {
                            let id = format!("pi_{}", fake.intents.len() + 1);
                            let payment = form
                                .split('&')
                                .find_map(|p| p.strip_prefix("metadata%5Bhypha_payment%5D="))
                                .unwrap_or_default()
                                .to_string();
                            let intent = json!({
                                "id": id, "amount": field("amount").parse::<i64>().unwrap_or(0),
                                "currency": field("currency"), "status": "requires_payment_method",
                                "client_secret": format!("{id}_secret_x"),
                                "capture_method": field("capture_method"),
                                "metadata": { "hypha_payment": payment },
                                "idempotency_key": key,
                            });
                            fake.intents.insert(id, intent.clone());
                            intent
                        });
                        (200, intent)
                    }
                    ("GET", ["payment_intents", id]) => match fake.intents.get(*id) {
                        Some(intent) => (200, intent.clone()),
                        None => (404, json!({"error": {"type": "invalid_request_error", "code": "resource_missing", "message": "No such payment_intent"}})),
                    },
                    ("POST", ["payment_intents", id, action]) => {
                        let next = match *action {
                            "capture" => "succeeded",
                            _ => "canceled",
                        };
                        match fake.intents.get_mut(*id) {
                            Some(intent) => {
                                let ok = match *action {
                                    "capture" => intent["status"] == "requires_capture",
                                    _ => intent["status"] != "succeeded"
                                        && intent["status"] != "canceled",
                                };
                                if ok {
                                    intent["status"] = json!(next);
                                    (200, intent.clone())
                                } else {
                                    let state = intent["status"].as_str().unwrap_or_default();
                                    (400, json!({"error": {"type": "invalid_request_error",
                                        "code": "payment_intent_unexpected_state",
                                        "message": format!("You cannot {action} this PaymentIntent because it has a status of {state}.")}}))
                                }
                            }
                            None => (404, json!({"error": {"type": "invalid_request_error", "code": "resource_missing", "message": "No such payment_intent"}})),
                        }
                    }
                    ("POST", ["refunds"])
                        if fake.intents.get(&field("payment_intent"))
                            .is_some_and(|i| i["key_revoked"] == json!(true)) =>
                    {
                        (401, json!({"error": {"type": "invalid_request_error",
                            "message": "Expired API Key provided."}}))
                    }
                    ("POST", ["refunds"])
                        if fake.intents.get(&field("payment_intent"))
                            .is_some_and(|i| i["disputed"] == json!(true)) =>
                    {
                        (400, json!({"error": {"type": "invalid_request_error",
                            "code": "charge_disputed",
                            "message": "This charge is disputed and can't be refunded."}}))
                    }
                    ("POST", ["refunds"]) => {
                        let id = format!("re_{}", fake.refunds.len() + 1);
                        let refund = json!({
                            "id": id, "amount": field("amount").parse::<i64>().unwrap_or(0),
                            "status": "succeeded", "created": 1_700_000_000 + fake.refunds.len() as i64,
                            "payment_intent": field("payment_intent"),
                        });
                        fake.refunds.push(refund.clone());
                        (200, refund)
                    }
                    ("GET", ["refunds"]) => {
                        let intent = uri
                            .query()
                            .unwrap_or_default()
                            .split('&')
                            .find_map(|p| p.strip_prefix("payment_intent="))
                            .unwrap_or_default()
                            .to_string();
                        let data: Vec<Value> = fake
                            .refunds
                            .iter()
                            .filter(|r| r["payment_intent"] == json!(intent))
                            .cloned()
                            .collect();
                        (200, json!({ "object": "list", "data": data, "has_more": false }))
                    }
                    _ => (404, json!({"error": {"type": "invalid_request_error", "message": "unknown"}})),
                };
                (
                    StatusCode::from_u16(status).unwrap(),
                    [(CONTENT_TYPE, "application/json")],
                    answer.to_string(),
                )
            }
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });
    (url, state)
}

/// A host with the Stripe spore pointed at a fake Stripe, its keys saved.
async fn stripe_host() -> (TestApp<WithStripe>, Shared) {
    let (base, fake) = fake_stripe().await;
    let toml = format!("[spores.stripe]\napi_base = \"{base}\"");
    let app = app_with_spores::<WithStripe>(Config::load(Some(&toml), |_| None).unwrap()).await;
    let body = json!({ "settings": {
        "secret_key": "sk_test_key",
        "publishable_key": "pk_test_key",
        "webhook_secret": WHSEC,
        "capture_mode": "manual"
    }})
    .to_string();
    let (status, json) = send(
        &app,
        "PUT",
        "/admin/apps/spore_stripe/settings",
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    (app, fake)
}

/// A cart paying by the Stripe method: the publishable key and the cart's path.
async fn cart_paying_by_stripe(app: &TestApp<WithStripe>) -> (String, String) {
    let body = r#"{"provider":"spore_stripe","name":"Card"}"#;
    let (status, method) = send(app, "POST", "/admin/payment-methods", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{method}");
    let (pk, cart_path, _) = cart_for_three_mugs(app).await;
    let body = json!({ "payment_method": method["id"] }).to_string();
    let (status, cart) = send_as(
        app,
        &pk,
        "PUT",
        &format!("{cart_path}/payment-method"),
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cart}");
    (pk, cart_path)
}

/// Stripe's event `kind` about `object`, signed with the spore's secret, delivered.
async fn deliver(
    app: &TestApp<WithStripe>,
    id: &str,
    kind: &str,
    object: Value,
) -> (StatusCode, Value) {
    let body =
        json!({ "id": id, "type": kind, "created": 1_700_000_000, "data": { "object": object } })
            .to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let signed = signature::sign(WHSEC, body.as_bytes(), now);
    send_with(
        app,
        &[("stripe-signature", &signed)],
        "POST",
        "/spores/stripe/webhook",
        Some(&body),
    )
    .await
}

async fn payment_status(app: &TestApp<WithStripe>, payment: &str) -> String {
    let (status, json) = send(app, "GET", &format!("/admin/payments/{payment}"), None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    json["status"].as_str().unwrap_or_default().to_string()
}

/// The shopper starts paying for the cart (design note 0031): the answer.
async fn start_paying(app: &TestApp<WithStripe>, pk: &str, cart_path: &str) -> Value {
    let (status, json) =
        send_as(app, pk, "POST", &format!("{cart_path}/payment"), Some("{}")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    json
}

/// The shopper's checkout once the card form said it's paid.
async fn check_out(app: &TestApp<WithStripe>, pk: &str, cart_path: &str) -> (StatusCode, Value) {
    send_as(
        app,
        pk,
        "POST",
        &format!("{cart_path}/checkout"),
        Some("{}"),
    )
    .await
}

/// Stripe holds `intent`'s money: its status as Stripe would answer it now.
fn hold(fake: &Shared, intent: &str) {
    fake.lock().unwrap().intents.get_mut(intent).unwrap()["status"] = json!("requires_capture");
}

#[tokio::test]
async fn a_card_payment_goes_from_the_checkout_through_stripe_to_paid_and_partly_refunded() {
    let (app, fake) = stripe_host().await;

    // Staff are offered it as a provider.
    let (status, providers) = send(&app, "GET", "/admin/payment-providers", None).await;
    assert_eq!(status, StatusCode::OK, "{providers}");
    assert!(
        providers["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["key"] == "spore_stripe" && p["kind"] == "spore" && p["name"] == "Stripe"),
        "{providers}"
    );

    // The shopper starts paying before any order: a PaymentIntent keyed by the payment,
    // the element to mount on the checkout.
    let (pk, cart_path) = cart_paying_by_stripe(&app).await;
    let (status, early) = check_out(&app, &pk, &cart_path).await;
    assert_eq!(
        (status, early["code"].as_str()),
        (StatusCode::CONFLICT, Some("payment_required")),
        "{early}"
    );
    let started = start_paying(&app, &pk, &cart_path).await;
    assert_eq!(started["next_action"]["type"], "client_widget", "{started}");
    assert_eq!(started["next_action"]["element"], "stripe-payment");
    assert_eq!(
        started["next_action"]["props"]["client_secret"],
        "pi_1_secret_x"
    );
    assert_eq!(
        started["next_action"]["props"]["publishable_key"],
        "pk_test_key"
    );
    assert_eq!(started["status"], "pending", "{started}");
    let payment = started["payment"].as_str().unwrap().to_string();
    let (_, json) = send(&app, "GET", &format!("/admin/payments/{payment}"), None).await;
    assert_eq!(json["order"], Value::Null, "no order yet: {json}");
    assert_eq!(json["attempt"], "pi_1", "{json}");
    {
        let fake = fake.lock().unwrap();
        let (_, path, key, form) = &fake.seen[0];
        assert_eq!(path, "/v1/payment_intents");
        assert_eq!(
            key, &payment,
            "Hypha's payment ID is Stripe's idempotency key"
        );
        assert!(
            form.contains("capture_method=manual")
                && form.contains("amount=")
                && form.contains("metadata%5Bhypha_cart%5D=cart_"),
            "{form}"
        );
        assert_eq!(
            fake.intents["pi_1"]["metadata"]["hypha_payment"],
            json!(payment)
        );
    }

    // Started again (the checkout page reloaded): the same intent, resumed by its ID.
    let again = start_paying(&app, &pk, &cart_path).await;
    assert_eq!(again["payment"], json!(payment), "{again}");
    assert_eq!(
        again["next_action"]["props"]["client_secret"],
        "pi_1_secret_x"
    );
    {
        let fake = fake.lock().unwrap();
        let creates = fake
            .seen
            .iter()
            .filter(|(m, p, _, _)| m == "POST" && p == "/v1/payment_intents")
            .count();
        assert_eq!(creates, 1, "{:?}", fake.seen);
    }

    // The storefront finds the element, served by the spore at the API's address.
    let (status, found) = send_as(
        &app,
        &pk,
        "GET",
        "/storefront/app-extensions?target=checkout.payment",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{found}");
    let element = &found["elements"][0];
    assert_eq!(
        (element["app"].as_str(), element["tag"].as_str()),
        (Some("spore_stripe"), Some("stripe-payment"))
    );
    assert!(
        element["module"]
            .as_str()
            .unwrap()
            .ends_with("/spores/stripe/elements/payment.js"),
        "{element}"
    );
    let request = Request::builder()
        .uri("/spores/stripe/elements/payment.js")
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["access-control-allow-origin"], "*");

    // Before the shopper pays: checkout waits.
    let (status, waiting) = check_out(&app, &pk, &cart_path).await;
    assert_eq!(
        (status, waiting["code"].as_str()),
        (StatusCode::CONFLICT, Some("payment_pending")),
        "{waiting}"
    );

    // The card is authorized; the browser checks out before Stripe's webhook: Hypha
    // asks Stripe, and the order is placed with the payment authorized.
    hold(&fake, "pi_1");
    let (status, placed) = check_out(&app, &pk, &cart_path).await;
    assert_eq!(status, StatusCode::CREATED, "{placed}");
    assert_eq!(placed["next_action"], Value::Null, "paid online already");
    assert_eq!(placed["financial_status"], "authorized", "{placed}");
    assert_eq!(placed["payment"]["status"], "authorized", "{placed}");
    let order = placed["id"].as_str().unwrap().to_string();
    let (_, json) = send(&app, "GET", &format!("/admin/payments/{payment}"), None).await;
    assert_eq!(json["order"], json!(order), "{json}");
    // Stripe's webhook then changes nothing.
    let intent = fake.lock().unwrap().intents["pi_1"].clone();
    let capturable =
        json!({ "id": "pi_1", "amount_capturable": 3000, "metadata": intent["metadata"] });
    let (status, outcome) = deliver(
        &app,
        "evt_auth",
        "payment_intent.amount_capturable_updated",
        capturable,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(payment_status(&app, &payment).await, "authorized");

    // A forged or unsigned delivery changes nothing.
    let (status, _) = send_with(
        &app,
        &[("stripe-signature", "t=1,v1=00")],
        "POST",
        "/spores/stripe/webhook",
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The block shows Stripe's view, with Capture and Void.
    let block = format!(
        "/admin/app-extensions/blocks/spore_stripe/payment?order_id={order}&payment_id={payment}"
    );
    let (status, tree) = send(&app, "GET", &block, None).await;
    assert_eq!(status, StatusCode::OK, "{tree}");
    let text = tree.to_string();
    assert!(
        text.contains("Authorized") && text.contains("\"action\":\"capture\""),
        "{text}"
    );

    // Staff capture through the payment's route: Stripe's answer says it's done, so the
    // payment is paid at once; Stripe's webhook and its redelivery then change nothing.
    let (status, json) = send(
        &app,
        "POST",
        &format!("/admin/payments/{payment}/capture"),
        Some("{}"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{json}");
    assert_eq!(json["payment"]["status"], "paid", "{json}");
    assert_eq!(json["completed"], true, "{json}");
    let succeeded = json!({ "id": "pi_1", "metadata": intent["metadata"] });
    for event in ["evt_paid", "evt_paid"] {
        let (status, _) = deliver(&app, event, "payment_intent.succeeded", succeeded.clone()).await;
        assert_eq!(status, StatusCode::OK);
    }
    assert_eq!(payment_status(&app, &payment).await, "paid");

    // A partial refund: Stripe's answer says it's done, so it counts at once, and its
    // charge.refunded (twice) counts it no more: the refund's ID was counted.
    let (status, json) = send(
        &app,
        "POST",
        &format!("/admin/payments/{payment}/refund"),
        Some(r#"{"amount":500}"#),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{json}");
    assert_eq!(json["payment"]["status"], "partially_refunded", "{json}");
    let charge = json!({ "id": "ch_1", "payment_intent": "pi_1", "amount": 3000, "currency": "eur", "refunded": false });
    for event in ["evt_refund", "evt_refund_again"] {
        let (status, outcome) = deliver(&app, event, "charge.refunded", charge.clone()).await;
        assert_eq!(status, StatusCode::OK, "{outcome}");
    }
    let (_, json) = send(&app, "GET", &format!("/admin/payments/{payment}"), None).await;
    assert_eq!(json["status"], "partially_refunded", "{json}");
    assert_eq!(json["refunded"]["amount"], 500, "{json}");
}

#[tokio::test]
async fn stripes_webhook_completes_the_checkout_when_the_shopper_never_comes_back() {
    let (app, fake) = stripe_host().await;
    let (pk, cart_path) = cart_paying_by_stripe(&app).await;
    let payment = start_paying(&app, &pk, &cart_path).await["payment"]
        .as_str()
        .unwrap()
        .to_string();
    hold(&fake, "pi_1");
    let metadata = fake.lock().unwrap().intents["pi_1"]["metadata"].clone();
    let capturable = json!({ "id": "pi_1", "amount_capturable": 3000, "metadata": metadata });
    let (status, outcome) = deliver(
        &app,
        "evt_auth",
        "payment_intent.amount_capturable_updated",
        capturable,
    )
    .await;
    assert_eq!(
        (status, outcome["status"].as_str()),
        (StatusCode::OK, Some("reported")),
        "{outcome}"
    );
    let (_, json) = send(&app, "GET", &format!("/admin/payments/{payment}"), None).await;
    assert_eq!(json["status"], "authorized", "{json}");
    let order = json["order"]
        .as_str()
        .expect("the order is placed")
        .to_string();
    let (status, admin) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    assert_eq!(status, StatusCode::OK, "{admin}");
    assert_eq!(admin["financial_status"], "authorized", "{admin}");
    // The shopper's browser comes back later: the same order.
    let (status, placed) = check_out(&app, &pk, &cart_path).await;
    assert_eq!(status, StatusCode::CREATED, "{placed}");
    assert_eq!(placed["id"], json!(order), "{placed}");
}

#[tokio::test]
async fn money_for_a_replaced_payment_goes_back() {
    let (app, fake_state) = stripe_host().await;
    let (pk, cart_path) = cart_paying_by_stripe(&app).await;
    let first = start_paying(&app, &pk, &cart_path).await["payment"]
        .as_str()
        .unwrap()
        .to_string();
    let metadata = fake_state.lock().unwrap().intents["pi_1"]["metadata"].clone();
    // The shopper takes one mug more: a new payment for the new total, the old one
    // voided and its intent canceled at Stripe.
    let (_, cart) = send_as(&app, &pk, "GET", &cart_path, None).await;
    let mug = cart["lines"][0]["variant"].as_str().unwrap().to_string();
    let body = json!({ "quantity": 4 }).to_string();
    let (status, json) = send_as(
        &app,
        &pk,
        "PUT",
        &format!("{cart_path}/lines/{mug}"),
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let second = start_paying(&app, &pk, &cart_path).await;
    assert_ne!(second["payment"], json!(first), "{second}");
    assert_eq!(
        second["next_action"]["props"]["client_secret"],
        "pi_2_secret_x"
    );
    assert_eq!(payment_status(&app, &first).await, "voided");
    assert!(
        fake_state
            .lock()
            .unwrap()
            .seen
            .iter()
            .any(|(m, p, _, _)| m == "POST" && p == "/v1/payment_intents/pi_1/cancel")
    );

    // Had the old tab's confirmation won the race, Stripe reports it paid: the payment
    // was voided, so the money goes back at Stripe, once per delivery.
    let succeeded = json!({ "id": "pi_1", "metadata": metadata });
    for event in ["evt_late", "evt_late_again"] {
        let (status, outcome) =
            deliver(&app, event, "payment_intent.succeeded", succeeded.clone()).await;
        assert_eq!(status, StatusCode::OK, "{outcome}");
        assert_eq!(outcome["status"], "ignored", "{outcome}");
    }
    {
        let fake = fake_state.lock().unwrap();
        let refunds: Vec<_> = fake
            .seen
            .iter()
            .filter(|(m, p, _, _)| m == "POST" && p == "/v1/refunds")
            .collect();
        assert_eq!(refunds.len(), 2, "{:?}", fake.seen);
        assert!(
            refunds
                .iter()
                .all(|(_, _, key, form)| key.ends_with(":refund-after-void")
                    && form.contains("payment_intent=pi_1"))
        );
    }

    // A key the merchant rotated isn't Stripe saying no about the money: 500, so
    // Stripe delivers it again once the new key is saved.
    fake_state.lock().unwrap().intents.get_mut("pi_1").unwrap()["key_revoked"] = json!(true);
    let (status, outcome) = deliver(
        &app,
        "evt_rotated",
        "payment_intent.succeeded",
        succeeded.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{outcome}");
    fake_state.lock().unwrap().intents.get_mut("pi_1").unwrap()["key_revoked"] = json!(false);

    // A refund Stripe refuses (the charge is disputed) is acknowledged, not retried
    // forever: staff give the money back in Stripe's dashboard.
    fake_state.lock().unwrap().intents.get_mut("pi_1").unwrap()["disputed"] = json!(true);
    let (status, outcome) =
        deliver(&app, "evt_disputed", "payment_intent.succeeded", succeeded).await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["status"], "ignored", "{outcome}");
    assert!(
        outcome["reason"]
            .as_str()
            .is_some_and(|r| r.contains("Stripe refused to give it back")),
        "{outcome}"
    );
}

#[tokio::test]
async fn money_paid_through_a_second_intent_of_a_paid_payment_goes_back() {
    let (app, fake_state) = stripe_host().await;
    let (pk, cart_path) = cart_paying_by_stripe(&app).await;
    let payment = start_paying(&app, &pk, &cart_path).await["payment"]
        .as_str()
        .unwrap()
        .to_string();
    let metadata = fake_state.lock().unwrap().intents["pi_1"]["metadata"].clone();
    let paid = |id: &str| json!({ "id": id, "metadata": metadata });
    let (status, outcome) =
        deliver(&app, "evt_paid", "payment_intent.succeeded", paid("pi_1")).await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(payment_status(&app, &payment).await, "paid");

    // Another intent of the same payment paid in an old tab: the payment is paid once,
    // and that money goes back.
    let (status, outcome) =
        deliver(&app, "evt_second", "payment_intent.succeeded", paid("pi_9")).await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["status"], "ignored", "{outcome}");
    assert!(
        outcome["reason"]
            .as_str()
            .is_some_and(|r| r.contains("through another PaymentIntent")),
        "{outcome}"
    );
    let (_, json) = send(&app, "GET", &format!("/admin/payments/{payment}"), None).await;
    assert_eq!(json["status"], "paid", "{json}");
    assert_eq!(json["reference"], "pi_1", "{json}");
    {
        let fake = fake_state.lock().unwrap();
        assert!(
            fake.seen.iter().any(|(m, p, _, form)| m == "POST"
                && p == "/v1/refunds"
                && form.contains("payment_intent=pi_9")),
            "{:?}",
            fake.seen
        );
        assert!(
            !fake.seen.iter().any(|(m, p, _, form)| m == "POST"
                && p == "/v1/refunds"
                && form.contains("payment_intent=pi_1")),
            "{:?}",
            fake.seen
        );
    }

    // Stripe then reports that intent refunded, and canceled: neither is the payment's.
    {
        let mut fake = fake_state.lock().unwrap();
        let mut second = fake.intents["pi_1"].clone();
        second["id"] = json!("pi_9");
        fake.intents.insert("pi_9".to_string(), second);
    }
    let charge = json!({ "id": "ch_9", "payment_intent": "pi_9", "amount": 3000, "currency": "eur", "refunded": true });
    let (status, outcome) = deliver(&app, "evt_refunded_9", "charge.refunded", charge).await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["status"], "ignored", "{outcome}");
    let (status, outcome) = deliver(
        &app,
        "evt_canceled_9",
        "payment_intent.canceled",
        paid("pi_9"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["status"], "ignored", "{outcome}");
    let (_, json) = send(&app, "GET", &format!("/admin/payments/{payment}"), None).await;
    assert_eq!(json["status"], "paid", "{json}");
    assert_eq!(json["refunded"]["amount"], 0, "{json}");
}

#[tokio::test]
async fn another_intent_paid_while_the_payment_is_authorized_goes_back() {
    let (app, fake_state) = stripe_host().await;
    let (pk, cart_path) = cart_paying_by_stripe(&app).await;
    let payment = start_paying(&app, &pk, &cart_path).await["payment"]
        .as_str()
        .unwrap()
        .to_string();
    let metadata = fake_state.lock().unwrap().intents["pi_1"]["metadata"].clone();
    let capturable = json!({ "id": "pi_1", "amount_capturable": 3000, "metadata": metadata });
    let (status, outcome) = deliver(
        &app,
        "evt_auth",
        "payment_intent.amount_capturable_updated",
        capturable,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(payment_status(&app, &payment).await, "authorized");

    // Stripe captures through the payment's own intent: another one paid is a second
    // collection, given back, and the authorization stays as it was.
    let other = json!({ "id": "pi_9", "metadata": metadata });
    let (status, outcome) = deliver(&app, "evt_other", "payment_intent.succeeded", other).await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["status"], "ignored", "{outcome}");
    assert_eq!(payment_status(&app, &payment).await, "authorized");
    let fake = fake_state.lock().unwrap();
    assert!(
        fake.seen.iter().any(|(m, p, _, form)| m == "POST"
            && p == "/v1/refunds"
            && form.contains("payment_intent=pi_9")),
        "{:?}",
        fake.seen
    );
}

#[tokio::test]
async fn a_stripe_without_keys_starts_nothing_and_a_disabled_one_is_not_offered() {
    let (base, _) = fake_stripe().await;
    let toml = format!("[spores.stripe]\napi_base = \"{base}\"");
    let app = app_with_spores::<WithStripe>(Config::load(Some(&toml), |_| None).unwrap()).await;
    let (pk, cart_path) = cart_paying_by_stripe(&app).await;
    let started = start_paying(&app, &pk, &cart_path).await;
    assert_eq!(started["next_action"], Value::Null, "{started}");
    assert_eq!(
        started["payment_error"]["code"], "payment_provider_failed",
        "{started}"
    );

    let (status, _) = send(
        &app,
        "PATCH",
        "/admin/apps/spore_stripe",
        Some(r#"{"enabled":false}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, providers) = send(&app, "GET", "/admin/payment-providers", None).await;
    assert!(
        !providers["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["key"] == "spore_stripe"),
        "{providers}"
    );
    let (status, _) = send_with(&app, &[], "GET", "/spores/stripe/elements/payment.js", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
