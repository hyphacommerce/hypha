//! Stripe's REST API over the host's HTTP client (`SporeEnv`), so it runs on every host:
//! form-encoded requests, JSON answers, `Idempotency-Key` on writes. `base` is Stripe
//! (`https://api.stripe.com`) or, in tests and the end-to-end script, a fake
//! (`[spores.stripe] api_base`).

use hypha_spore::client::{HttpClient, HttpError, HttpMethod, HttpRequest};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

/// Stripe's API.
pub const API_BASE: &str = "https://api.stripe.com";

/// An error answer from Stripe (`card_error`, `invalid_request_error`, …) or no answer.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct StripeError {
    /// Stripe's HTTP status; 0 when it couldn't be reached.
    pub status: u16,
    pub kind: String,
    pub message: String,
    pub code: Option<String>,
}

impl StripeError {
    /// Stripe said no to this request: retrying the same won't help.
    pub fn is_refusal(&self) -> bool {
        (400..500).contains(&self.status) && self.status != 429
    }

    fn unreachable(e: HttpError) -> Self {
        Self {
            status: 0,
            kind: "api_connection_error".to_string(),
            message: format!("Stripe couldn't be reached: {e}"),
            code: None,
        }
    }
}

/// The PaymentIntent fields used here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PaymentIntent {
    pub id: String,
    pub amount: i64,
    #[serde(default)]
    pub amount_received: i64,
    pub currency: String,
    /// `requires_payment_method`, `requires_confirmation`, `requires_action`,
    /// `processing`, `requires_capture`, `canceled` or `succeeded`.
    pub status: String,
    pub client_secret: Option<String>,
    #[serde(default)]
    pub capture_method: Option<String>,
    #[serde(default)]
    pub livemode: bool,
    #[serde(default)]
    pub metadata: std::collections::BTreeMap<String, String>,
}

/// A refund's fields used here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Refund {
    pub id: String,
    pub amount: i64,
    /// `pending`, `requires_action`, `succeeded`, `failed` or `canceled`.
    pub status: String,
    #[serde(default)]
    pub created: i64,
}

#[derive(Deserialize)]
struct List<T> {
    data: Vec<T>,
    #[serde(default)]
    has_more: bool,
}

/// A form value: Stripe's nested encoding writes `{metadata: {a: "1"}}` as
/// `metadata[a]=1`.
pub enum Form {
    Text(String),
    Map(Vec<(&'static str, Form)>),
}

impl From<&str> for Form {
    fn from(text: &str) -> Self {
        Form::Text(text.to_string())
    }
}

impl From<String> for Form {
    fn from(text: String) -> Self {
        Form::Text(text)
    }
}

impl From<i64> for Form {
    fn from(n: i64) -> Self {
        Form::Text(n.to_string())
    }
}

/// `params` as `application/x-www-form-urlencoded`.
pub fn encode_form(params: &[(&str, Form)]) -> String {
    fn add(out: &mut Vec<String>, prefix: &str, value: &Form) {
        match value {
            Form::Text(text) => out.push(format!("{}={}", escape(prefix), escape(text))),
            Form::Map(entries) => {
                for (key, inner) in entries {
                    add(out, &format!("{prefix}[{key}]"), inner);
                }
            }
        }
    }
    let mut out = Vec::new();
    for (key, value) in params {
        add(&mut out, key, value);
    }
    out.join("&")
}

/// Percent-encodes everything but unreserved characters (RFC 3986).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Stripe, with one secret key.
pub struct StripeClient<'a, H> {
    pub http: &'a H,
    pub base: &'a str,
    pub key: &'a str,
}

impl<H: HttpClient> StripeClient<'_, H> {
    async fn request<T: DeserializeOwned>(
        &self,
        method: HttpMethod,
        path: &str,
        params: &[(&str, Form)],
        idempotency_key: Option<&str>,
    ) -> Result<T, StripeError> {
        let form = encode_form(params);
        let base = self.base.trim_end_matches('/');
        let url = match method {
            HttpMethod::Get if !form.is_empty() => format!("{base}{path}?{form}"),
            _ => format!("{base}{path}"),
        };
        let mut request = HttpRequest::new(method, url)
            .header("authorization", format!("Bearer {}", self.key))
            .header("accept", "application/json");
        if method == HttpMethod::Post {
            request = request
                .header("content-type", "application/x-www-form-urlencoded")
                .body(form.into_bytes());
            if let Some(key) = idempotency_key {
                request = request.header("idempotency-key", key);
            }
        }
        let answer = self
            .http
            .send(request)
            .await
            .map_err(StripeError::unreachable)?;
        let body: Value = serde_json::from_slice(&answer.body).unwrap_or(Value::Null);
        if !(200..300).contains(&answer.status) {
            let error = body.get("error");
            let text = |key: &str| {
                error
                    .and_then(|e| e.get(key))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            return Err(StripeError {
                status: answer.status,
                kind: text("type").unwrap_or_else(|| "api_error".to_string()),
                message: text("message")
                    .unwrap_or_else(|| format!("Stripe answered HTTP {}", answer.status)),
                code: text("code"),
            });
        }
        serde_json::from_value(body).map_err(|e| StripeError {
            status: 502,
            kind: "api_error".to_string(),
            message: format!("Stripe's answer isn't what was expected: {e}"),
            code: None,
        })
    }

    pub async fn create_payment_intent(
        &self,
        params: &[(&str, Form)],
        idempotency_key: &str,
    ) -> Result<PaymentIntent, StripeError> {
        self.request(
            HttpMethod::Post,
            "/v1/payment_intents",
            params,
            Some(idempotency_key),
        )
        .await
    }

    pub async fn retrieve_payment_intent(&self, id: &str) -> Result<PaymentIntent, StripeError> {
        self.request(
            HttpMethod::Get,
            &format!("/v1/payment_intents/{}", escape(id)),
            &[],
            None,
        )
        .await
    }

    pub async fn capture_payment_intent(
        &self,
        id: &str,
        params: &[(&str, Form)],
        idempotency_key: &str,
    ) -> Result<PaymentIntent, StripeError> {
        self.request(
            HttpMethod::Post,
            &format!("/v1/payment_intents/{}/capture", escape(id)),
            params,
            Some(idempotency_key),
        )
        .await
    }

    pub async fn cancel_payment_intent(
        &self,
        id: &str,
        idempotency_key: &str,
    ) -> Result<PaymentIntent, StripeError> {
        self.request(
            HttpMethod::Post,
            &format!("/v1/payment_intents/{}/cancel", escape(id)),
            &[("cancellation_reason", "requested_by_customer".into())],
            Some(idempotency_key),
        )
        .await
    }

    pub async fn create_refund(
        &self,
        params: &[(&str, Form)],
        idempotency_key: &str,
    ) -> Result<Refund, StripeError> {
        self.request(
            HttpMethod::Post,
            "/v1/refunds",
            params,
            Some(idempotency_key),
        )
        .await
    }

    /// Every refund of a PaymentIntent, oldest first (Stripe lists newest first, in
    /// pages of at most 100).
    pub async fn list_refunds(&self, payment_intent: &str) -> Result<Vec<Refund>, StripeError> {
        let mut all: Vec<Refund> = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut params: Vec<(&str, Form)> = vec![
                ("payment_intent", payment_intent.into()),
                ("limit", 100.into()),
            ];
            if let Some(after) = &after {
                params.push(("starting_after", after.clone().into()));
            }
            let page: List<Refund> = self
                .request(HttpMethod::Get, "/v1/refunds", &params, None)
                .await?;
            let last = page.data.last().map(|r| r.id.clone());
            all.extend(page.data);
            match (page.has_more, last) {
                (true, Some(last)) => after = Some(last),
                _ => break,
            }
        }
        all.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)));
        Ok(all)
    }
}

/// Stripe's dashboard page for a PaymentIntent (the test-mode one for test keys).
pub fn dashboard_url(intent: &str, livemode: bool) -> String {
    let mode = if livemode { "" } else { "test/" };
    format!(
        "https://dashboard.stripe.com/{mode}payments/{}",
        escape(intent)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_nest_maps_and_escape_values() {
        let form = encode_form(&[
            ("amount", 1250.into()),
            ("description", "Order #1001 & co".into()),
            (
                "metadata",
                Form::Map(vec![("hypha_payment", "pay_1".into())]),
            ),
        ]);
        assert_eq!(
            form,
            "amount=1250&description=Order%20%231001%20%26%20co&metadata%5Bhypha_payment%5D=pay_1"
        );
    }

    #[test]
    fn a_refusal_is_a_4xx_but_not_a_rate_limit() {
        let error = |status| StripeError {
            status,
            kind: "x".to_string(),
            message: "x".to_string(),
            code: None,
        };
        assert!(error(402).is_refusal());
        assert!(!error(429).is_refusal());
        assert!(!error(500).is_refusal());
        assert!(!error(0).is_refusal());
    }
}
