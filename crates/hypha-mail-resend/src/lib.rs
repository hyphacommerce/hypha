//! The Resend sender: `hypha_notifications::EmailSender` over Resend's HTTP API, `POST
//! https://api.resend.com/emails`.
//!
//! It opens no connections itself: HTTP goes through this crate's `HttpClient` port,
//! which the host implements (`reqwest` natively, `fetch` on Workers), so the crate stays
//! runtime-free and builds for wasm. The port is its own rather than
//! `hypha_store_s3::HttpClient`: that one carries no request body (the media storage
//! never sends file bytes), and an email adapter shouldn't depend on a storage one.

use hypha_notifications::{EmailSender, OutgoingEmail, SendError, Sent};
use serde::{Deserialize, Serialize};

/// One HTTP request for `HttpClient`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// What came back: the status and (at most `MAX_ANSWER_BYTES` of) the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// The most of an answer a client needs to read: Resend's answers are a few hundred
/// bytes (an ID, or an error's name and message).
pub const MAX_ANSWER_BYTES: usize = 64 << 10;

/// Sends a request and returns whatever status came back; `Err` only when no response
/// arrived (DNS, connection, timeout), with the client's words. `async fn` without
/// `Send`, like the other ports.
#[allow(async_fn_in_trait)]
pub trait HttpClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, String>;
}

impl<T: HttpClient + ?Sized> HttpClient for &T {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        (**self).send(request).await
    }
}

/// Sends through Resend with an API key (`HYPHA_RESEND_API_KEY`).
pub struct ResendSender<H> {
    api_key: String,
    http: H,
}

impl<H> ResendSender<H> {
    /// Where emails are sent.
    pub const URL: &'static str = "https://api.resend.com/emails";

    pub fn new(api_key: impl Into<String>, http: H) -> Self {
        Self {
            api_key: api_key.into(),
            http,
        }
    }
}

// Not `Debug`-derived: the key must never reach a log.
impl<H> std::fmt::Debug for ResendSender<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResendSender").finish_non_exhaustive()
    }
}

/// The body Resend takes (its API's snake_case names).
#[derive(Serialize)]
struct Body<'a> {
    from: String,
    to: [&'a str; 1],
    subject: &'a str,
    html: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to: Option<&'a str>,
}

/// What a 2xx carries.
#[derive(Deserialize)]
struct Accepted {
    id: Option<String>,
}

/// What an error carries: `{"statusCode": 422, "name": "validation_error", "message": …}`.
#[derive(Deserialize)]
struct Refusal {
    name: Option<String>,
    message: Option<String>,
}

impl<H: HttpClient> ResendSender<H> {
    fn request(&self, email: &OutgoingEmail) -> HttpRequest {
        let body = Body {
            from: email.from.to_string(),
            to: [email.to.as_str()],
            subject: &email.subject,
            html: &email.html,
            text: &email.text,
            reply_to: email.reply_to.as_ref().map(hypha_core::Email::as_str),
        };
        HttpRequest {
            method: "POST",
            url: Self::URL.to_string(),
            headers: vec![
                (
                    "Authorization".to_string(),
                    format!("Bearer {}", self.api_key),
                ),
                ("Content-Type".to_string(), "application/json".to_string()),
                // A retried send of the same message isn't delivered twice (Resend
                // keeps keys for 24 hours).
                ("Idempotency-Key".to_string(), email.id.to_string()),
            ],
            // Plain data with string keys: serializing can't fail.
            body: serde_json::to_vec(&body).unwrap_or_default(),
        }
    }
}

/// Reads Resend's answer: 2xx is sent; rate limits, server errors and a concurrent
/// request with the same key are worth retrying; any other refusal (an invalid address,
/// an unverified domain, a bad key) never works.
fn outcome(response: &HttpResponse) -> Result<Sent, SendError> {
    let status = response.status;
    if (200..300).contains(&status) {
        // The message was taken even if its ID can't be read.
        let id = serde_json::from_slice::<Accepted>(&response.body)
            .ok()
            .and_then(|a| a.id);
        return Ok(Sent {
            provider_message_id: id,
        });
    }
    let refusal = serde_json::from_slice::<Refusal>(&response.body).ok();
    let name = refusal.as_ref().and_then(|r| r.name.clone());
    let message = refusal
        .and_then(|r| r.message)
        .unwrap_or_else(|| snippet(&response.body));
    let said = match (&name, message.is_empty()) {
        (Some(name), false) => format!("HTTP {status} {name}: {message}"),
        (Some(name), true) => format!("HTTP {status} {name}"),
        (None, false) => format!("HTTP {status}: {message}"),
        (None, true) => format!("HTTP {status}"),
    };
    let retry = match status {
        408 | 429 => true,
        // Another request with this key is in flight: it may still fail.
        409 => name.as_deref() == Some("concurrent_idempotent_requests"),
        400..=499 => false,
        _ => true,
    };
    if retry {
        Err(SendError::Transient(said))
    } else {
        Err(SendError::Permanent(said))
    }
}

/// A body that isn't Resend's JSON, short enough for the record.
fn snippet(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    text.trim().chars().take(200).collect()
}

impl<H: HttpClient> EmailSender for ResendSender<H> {
    fn name(&self) -> &'static str {
        "resend"
    }

    async fn send(&self, email: &OutgoingEmail) -> Result<Sent, SendError> {
        let response = self
            .http
            .send(self.request(email))
            .await
            .map_err(|e| SendError::Transient(format!("no answer from Resend: {e}")))?;
        outcome(&response)
    }
}

/// `HttpClient` on `reqwest`, for the native host.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct ReqwestClient(reqwest::Client);

#[cfg(not(target_arch = "wasm32"))]
impl ReqwestClient {
    /// How long opening a connection may take.
    pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    /// How long a whole request may take: one email, so the send job's lease is never at
    /// risk.
    pub const REQUEST_TIMEOUT: std::time::Duration = hypha_notifications::SEND_TIMEOUT;

    pub fn new() -> Result<Self, String> {
        reqwest::Client::builder()
            .connect_timeout(Self::CONNECT_TIMEOUT)
            .timeout(Self::REQUEST_TIMEOUT)
            .build()
            .map(Self)
            .map_err(|e| format!("cannot build the HTTP client: {e}"))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl HttpClient for ReqwestClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        let method =
            reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|e| e.to_string())?;
        let mut builder = self.0.request(method, &request.url).body(request.body);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let mut response = builder.send().await.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let mut body = Vec::new();
        while body.len() < MAX_ANSWER_BYTES {
            let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? else {
                break;
            };
            body.extend_from_slice(&chunk);
        }
        body.truncate(MAX_ANSWER_BYTES);
        Ok(HttpResponse { status, body })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use hypha_core::{Email, NotificationId};
    use hypha_notifications::Mailbox;
    use uuid::Uuid;

    use super::*;

    /// Answers with `answer` and keeps the requests.
    struct FakeHttp {
        answer: Result<HttpResponse, String>,
        requests: RefCell<Vec<HttpRequest>>,
    }

    impl HttpClient for FakeHttp {
        async fn send(&self, request: HttpRequest) -> Result<HttpResponse, String> {
            self.requests.borrow_mut().push(request);
            self.answer.clone()
        }
    }

    fn answering(status: u16, body: &str) -> FakeHttp {
        FakeHttp {
            answer: Ok(HttpResponse {
                status,
                body: body.as_bytes().to_vec(),
            }),
            requests: RefCell::new(Vec::new()),
        }
    }

    fn email(reply_to: Option<&str>) -> OutgoingEmail {
        OutgoingEmail {
            id: NotificationId::from_uuid(Uuid::from_u128(7)),
            template: hypha_notifications::TemplateKey::StaffInvite,
            from: Mailbox {
                name: Some("Ana's \"Shop\"".to_string()),
                address: Email::new("shop@mail.example.com").unwrap(),
            },
            reply_to: reply_to.map(|r| Email::new(r).unwrap()),
            to: Email::new("buyer@example.com").unwrap(),
            subject: "Order #1001 confirmed".to_string(),
            html: "<p>Thanks</p>".to_string(),
            text: "Thanks".to_string(),
        }
    }

    fn send(http: &FakeHttp, email: &OutgoingEmail) -> Result<Sent, SendError> {
        pollster::block_on(ResendSender::new("re_test", http).send(email))
    }

    #[test]
    fn it_posts_the_message_with_the_key_and_the_idempotency_key() {
        let http = answering(200, r#"{"id":"49a3999c-0ce1-4ea6-ab68-afcd6dc2e794"}"#);
        let email = email(Some("help@example.com"));
        assert_eq!(
            send(&http, &email),
            Ok(Sent {
                provider_message_id: Some("49a3999c-0ce1-4ea6-ab68-afcd6dc2e794".to_string()),
            })
        );
        let requests = http.requests.borrow();
        let [request] = requests.as_slice() else {
            panic!("one request expected, got {requests:?}");
        };
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "https://api.resend.com/emails");
        assert_eq!(
            request.headers,
            [
                ("Authorization".to_string(), "Bearer re_test".to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Idempotency-Key".to_string(), email.id.to_string()),
            ]
        );
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "from": r#""Ana's \"Shop\"" <shop@mail.example.com>"#,
                "to": ["buyer@example.com"],
                "subject": "Order #1001 confirmed",
                "html": "<p>Thanks</p>",
                "text": "Thanks",
                "reply_to": "help@example.com",
            })
        );
    }

    #[test]
    fn without_a_reply_to_the_field_is_left_out() {
        let http = answering(200, r#"{"id":"x"}"#);
        send(&http, &email(None)).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&http.requests.borrow()[0].body).unwrap();
        assert_eq!(body.get("reply_to"), None);
    }

    #[test]
    fn an_accepted_message_without_a_readable_id_is_still_sent() {
        let http = answering(200, "not json");
        assert_eq!(send(&http, &email(None)), Ok(Sent::default()));
    }

    #[test]
    fn a_refused_address_or_domain_or_key_is_permanent_with_resends_words() {
        for (status, name, message) in [
            (422, "validation_error", "Invalid `to` field."),
            (
                403,
                "validation_error",
                "The mail.example.com domain is not verified.",
            ),
            (401, "validation_error", "API key is invalid"),
            (
                400,
                "invalid_idempotency_key",
                "The key must be 1-256 characters.",
            ),
            (409, "invalid_idempotent_request", "Same key, another body."),
        ] {
            let body =
                format!(r#"{{"statusCode":{status},"name":"{name}","message":"{message}"}}"#);
            let http = answering(status, &body);
            assert_eq!(
                send(&http, &email(None)),
                Err(SendError::Permanent(format!(
                    "HTTP {status} {name}: {message}"
                ))),
                "{status}"
            );
        }
    }

    #[test]
    fn timeouts_rate_limits_server_errors_and_a_concurrent_retry_are_transient() {
        for (status, name) in [
            (408, "request_timeout"),
            (429, "rate_limit_exceeded"),
            (429, "daily_quota_exceeded"),
            (500, "application_error"),
            (503, "service_unavailable"),
            (409, "concurrent_idempotent_requests"),
        ] {
            let body = format!(r#"{{"name":"{name}","message":"later"}}"#);
            let http = answering(status, &body);
            assert_eq!(
                send(&http, &email(None)),
                Err(SendError::Transient(format!("HTTP {status} {name}: later"))),
                "{status}"
            );
        }
    }

    #[test]
    fn an_answer_that_isnt_resends_json_is_quoted_short() {
        let http = answering(502, &format!("<html>{}</html>", "x".repeat(300)));
        let Err(SendError::Transient(said)) = send(&http, &email(None)) else {
            panic!("a 502 is transient");
        };
        assert_eq!(said.len(), "HTTP 502: ".len() + 200);
        let http = answering(404, "");
        assert_eq!(
            send(&http, &email(None)),
            Err(SendError::Permanent("HTTP 404".to_string()))
        );
    }

    #[test]
    fn no_answer_is_transient() {
        let http = FakeHttp {
            answer: Err("connection refused".to_string()),
            requests: RefCell::new(Vec::new()),
        };
        assert_eq!(
            send(&http, &email(None)),
            Err(SendError::Transient(
                "no answer from Resend: connection refused".to_string()
            ))
        );
    }

    #[test]
    fn its_name_is_resend_and_its_debug_hides_the_key() {
        let sender = ResendSender::new("re_secret", ());
        assert_eq!(format!("{sender:?}"), "ResendSender { .. }");
        let http = answering(200, "{}");
        assert_eq!(ResendSender::new("k", &http).name(), "resend");
    }
}
