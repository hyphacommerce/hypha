//! Calling an app's extension point (design note 0029, §3 and §15): serialize the
//! request, have the host sign and send it, read at most `MAX_ANSWER_BYTES`, parse the
//! answer as the point's response, and record the call.

use core::time::Duration;

use hypha_core::{AppKey, Clock, ExtensionCallId, IdGenerator, Timestamp};

use crate::{CallOutcome, ExtensionCall, ExtensionCallRepo, ExtensionError, ExtensionPoint};

/// The point's name (`payments.start`), to route on without parsing the body.
pub const EXTENSION_HEADER: &str = "Hypha-Extension";
/// The point's version (`1`).
pub const EXTENSION_VERSION_HEADER: &str = "Hypha-Extension-Version";
/// The same for every retry of one piece of work (a payment's ID): an app that did it
/// already answers as it did.
pub const IDEMPOTENCY_KEY_HEADER: &str = "Idempotency-Key";
/// The store the call is for (its `store_…` ID). Set by the host, which knows the
/// store; domain code never sees one (design note 0006).
pub const STORE_HEADER: &str = "Hypha-Store";
/// The most of an answer read: a bigger one is an `invalid` call rather than filled
/// memory. The same cap as webhooks' and rate callbacks' (0015 §5).
pub const MAX_ANSWER_BYTES: usize = 256 * 1024;

/// A call for the host to send: `POST url` with `body` (JSON) and `headers`, signed
/// with `secret` (`Hypha-Signature: t=<unix>,v1=<hex HMAC-SHA256 of "<t>.<body>">`,
/// 0015 §5), the content type and the store added by the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingCall<'a> {
    pub url: &'a str,
    /// The app's secret, in plain text, for the signature.
    pub secret: &'a str,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
    pub timeout: Duration,
}

/// What came back: any status, and its body read up to `MAX_ANSWER_BYTES`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Why no answer came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Nothing within the timeout.
    Timeout,
    /// Refused, unreachable, broken off, or not a public address: in words the merchant
    /// may see (the details stay in the host's log, so an answer can't map a network).
    Unreachable(String),
    /// More than `MAX_ANSWER_BYTES`.
    TooLarge,
}

/// How the host sends calls to apps: natively over its outbound client (public
/// addresses only), on Workers through `fetch` or a Service Binding (design note 0029,
/// §9). No `Send` bound (CLAUDE.md): Workers' futures aren't.
#[allow(async_fn_in_trait)]
pub trait ExtensionCaller {
    /// Signs and sends `call`; follows no redirects; reads at most
    /// `MAX_ANSWER_BYTES`.
    async fn send(&self, call: OutgoingCall<'_>) -> Result<Answer, TransportError>;
}

/// Where an app implements a point: its key, the URL it registered, its secret, and
/// what its registration overrides.
#[derive(Debug, Clone, Copy)]
pub struct Endpoint<'a> {
    pub app: &'a AppKey,
    pub url: &'a str,
    /// The app's secret, in plain text.
    pub secret: &'a str,
    /// The app's own timeout (a shipping provider's `timeout_ms`); `None` takes the
    /// point's `TIMEOUT`.
    pub timeout: Option<Duration>,
    /// Further headers a point's older protocol promised (shipping's
    /// `Hypha-Request-Id`).
    pub headers: &'a [(&'static str, String)],
}

/// Calls `endpoint` with `request` under the point `P` and records the call in
/// `recorder` (with `correlation`, the cart, order or payment it's for). The answer
/// parsed as `P::Response` and checked by `P::check`, or why there's none.
///
/// The failure policy (`P::ON_FAILURE`) isn't applied here: callers decide, usually with
/// `apply_policy`. A call that can't be recorded is still answered (the failure is
/// traced): the log is for looking back, not part of the flow.
pub async fn call<P: ExtensionPoint>(
    caller: &impl ExtensionCaller,
    recorder: &impl ExtensionCallRepo,
    env: &(impl Clock + IdGenerator),
    endpoint: &Endpoint<'_>,
    request: &P::Request,
    idempotency_key: &str,
    correlation: Option<&str>,
) -> Result<P::Response, ExtensionError> {
    // The points' requests are plain structs of strings, numbers and lists, which
    // serialize without fail.
    let body = serde_json::to_vec(request)
        .unwrap_or_else(|e| unreachable!("{} requests serialize: {e}", P::NAME));
    let mut headers = vec![
        (EXTENSION_HEADER, P::NAME.to_string()),
        (EXTENSION_VERSION_HEADER, P::VERSION.to_string()),
        (IDEMPOTENCY_KEY_HEADER, idempotency_key.to_string()),
    ];
    headers.extend(endpoint.headers.iter().cloned());
    let timeout = endpoint.timeout.unwrap_or(P::TIMEOUT);
    let started_at = env.now();
    let sent = caller
        .send(OutgoingCall {
            url: endpoint.url,
            secret: endpoint.secret,
            headers,
            body,
            timeout,
        })
        .await;
    let ended_at = env.now();
    let (status, result) = judge::<P>(endpoint.app, request, timeout, sent);
    let (outcome, error) = match &result {
        Ok(_) => (CallOutcome::Ok, None),
        Err(Judged::Failed(reason)) => (CallOutcome::Failed, Some(reason.clone())),
        Err(Judged::Timeout(reason)) => (CallOutcome::Timeout, Some(reason.clone())),
        Err(Judged::Invalid(reason)) => (CallOutcome::Invalid, Some(reason.clone())),
    };
    let record = ExtensionCall {
        id: ExtensionCallId::from_uuid(env.new_uuid()),
        app: endpoint.app.clone(),
        point: P::NAME.to_string(),
        version: P::VERSION,
        started_at,
        duration_ms: duration_ms(started_at, ended_at),
        outcome,
        status,
        error,
        correlation: correlation.map(str::to_string),
    };
    if let Err(e) = recorder.insert(&record).await {
        tracing::warn!(app = %endpoint.app, point = P::NAME, error = %e,
            "extensions: the call couldn't be recorded");
    }
    result.map_err(|judged| judged.into_error(endpoint.app, P::NAME))
}

/// A failed call, with the reason recorded.
enum Judged {
    Failed(String),
    Timeout(String),
    Invalid(String),
}

impl Judged {
    fn into_error(self, app: &AppKey, point: &'static str) -> ExtensionError {
        let app = app.clone();
        match self {
            Judged::Failed(reason) => ExtensionError::Unavailable { app, point, reason },
            Judged::Timeout(_) => ExtensionError::Timeout { app, point },
            Judged::Invalid(reason) => ExtensionError::Invalid { app, point, reason },
        }
    }
}

/// What the host's answer means for `P`: the status (when there was an answer) and the
/// response, or why there's none.
fn judge<P: ExtensionPoint>(
    app: &AppKey,
    request: &P::Request,
    timeout: Duration,
    sent: Result<Answer, TransportError>,
) -> (Option<u16>, Result<P::Response, Judged>) {
    let answer = match sent {
        Ok(answer) => answer,
        Err(TransportError::Timeout) => {
            let reason = format!("no answer within {} ms", timeout.as_millis());
            return (None, Err(Judged::Timeout(reason)));
        }
        Err(TransportError::Unreachable(reason)) => return (None, Err(Judged::Failed(reason))),
        Err(TransportError::TooLarge) => return (None, Err(Judged::Invalid(too_large()))),
    };
    let status = Some(answer.status);
    if !(200..300).contains(&answer.status) {
        let reason = match app_error_code(&answer.body) {
            Some(code) => format!("answered HTTP {}: {code}", answer.status),
            None => format!("answered HTTP {}", answer.status),
        };
        return (status, Err(Judged::Failed(reason)));
    }
    // A host keeps to the cap; one that didn't still gets no further.
    if answer.body.len() > MAX_ANSWER_BYTES {
        return (status, Err(Judged::Invalid(too_large())));
    }
    let parsed = serde_json::from_slice::<P::Response>(&answer.body)
        .map_err(|e| Judged::Invalid(format!("not a {} response: {e}", P::NAME)))
        .and_then(|response| {
            P::check(request, &response)
                .map(|()| response)
                .map_err(Judged::Invalid)
        });
    if let Err(Judged::Invalid(reason)) = &parsed {
        tracing::info!(%app, point = P::NAME, "extensions: an invalid answer: {reason}");
    }
    (status, parsed)
}

fn too_large() -> String {
    format!("answered more than {MAX_ANSWER_BYTES} bytes")
}

/// An app's error `code`, when its answer is the SDK's `{"code": "…", "error": "…"}`
/// (`invalid_signature`, `unknown_extension`): what the merchant needs to fix it. Only a
/// short snake_case code is kept, so an answer can't fill the log.
fn app_error_code(body: &[u8]) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct AppError {
        code: String,
    }
    let parsed: AppError = serde_json::from_slice(body).ok()?;
    let plain = !parsed.code.is_empty()
        && parsed.code.len() <= 64
        && parsed
            .code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    plain.then_some(parsed.code)
}

/// Milliseconds from `start` to `end`: 0 if the clock went back, at most `u32::MAX`.
fn duration_ms(start: Timestamp, end: Timestamp) -> u32 {
    let micros = end.unix_micros().saturating_sub(start.unix_micros()).max(0);
    u32::try_from(micros / 1000).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_error_code_is_kept_only_when_plain() {
        assert_eq!(
            app_error_code(br#"{"code":"invalid_signature","error":"bad"}"#),
            Some("invalid_signature".to_string())
        );
        assert_eq!(app_error_code(br#"{"code":"Not Plain"}"#), None);
        assert_eq!(app_error_code(b"<html>"), None);
        let long = format!(r#"{{"code":"{}"}}"#, "a".repeat(65));
        assert_eq!(app_error_code(long.as_bytes()), None);
    }

    #[test]
    fn durations_are_whole_milliseconds_and_never_negative() {
        let at = Timestamp::from_unix_micros;
        assert_eq!(duration_ms(at(1_000), at(3_999)), 2);
        assert_eq!(duration_ms(at(5_000), at(1_000)), 0);
        assert_eq!(duration_ms(at(0), at(i64::MAX)), u32::MAX);
    }
}
