//! The Workers runtime's capabilities (`hypha_http::host`): `Date.now()`, UUIDv7 from it
//! and `crypto.getRandomValues`, Argon2 inline (each request has its isolate's thread
//! to itself), `waitUntil` for work after the response, and `fetch` for calls to
//! merchant URLs and the media storage.
//!
//! JavaScript values aren't `Send`, but an isolate has one thread, so nothing they hold
//! ever moves between threads: `SendFuture` asserts that to the compiler for the futures
//! that hold them, as `#[worker::send]` does for handlers.

use std::pin::pin;

use futures_util::StreamExt;
use futures_util::future::{Either, select};
use hypha_core::{Clock, IdGenerator, Timestamp};
use hypha_http::host::{HostCalls, HostClock, HostHttp, HostIds, HostRandom, Offload};
use hypha_http::outbound::{CallError, MAX_ANSWER_BYTES, Method, OutboundCall};
use hypha_http::state::Runtime;
use hypha_store_s3::{HttpRequest, HttpResponse};
use uuid::Uuid;
use worker::send::SendFuture;
use worker::{AbortController, Date, Delay, Fetch, Headers, Request, RequestInit, RequestRedirect};

use crate::ids::V7State;

/// Time from JavaScript's `Date.now()` (`SystemTime::now()` panics on this target). Its
/// resolution is a millisecond, and it stands still while a request computes (Workers
/// advance it only on I/O, against timing attacks).
pub struct JsClock;

impl Clock for JsClock {
    fn now(&self) -> Timestamp {
        let millis = i64::try_from(Date::now().as_millis()).unwrap_or(i64::MAX);
        Timestamp::from_unix_micros(millis.saturating_mul(1000))
    }
}

/// UUIDv7 from `Date.now()` and `crypto.getRandomValues`, in order within an isolate.
#[derive(Default)]
pub struct JsIds(V7State);

impl IdGenerator for JsIds {
    fn new_uuid(&self) -> Uuid {
        let mut random = [0u8; 10];
        fill_random(&mut random);
        self.0.next(Date::now().as_millis(), random)
    }
}

fn fill_random(bytes: &mut [u8]) {
    // Without a random source no token or key can be made safely: stop, as the native
    // host does.
    if let Err(e) = getrandom::fill(bytes) {
        panic!("crypto.getRandomValues failed: {e}");
    }
}

/// The isolate's runtime.
pub fn runtime() -> Runtime {
    Runtime {
        clock: HostClock::new(JsClock),
        ids: HostIds::new(JsIds::default()),
        random: HostRandom::new(fill_random),
        offload: Offload::inline(),
    }
}

/// Calls to merchant URLs through `fetch`. Literal private addresses are refused before
/// (`outbound::prepare`); a Worker's `fetch` can't reach private networks by name
/// either, except in `wrangler dev`.
pub fn host_calls() -> HostCalls {
    HostCalls::new(|call| Box::pin(SendFuture::new(send_call(call))))
}

async fn send_call(call: OutboundCall) -> Result<Vec<u8>, CallError> {
    let failed = |what: &str| CallError::Failed(what.to_string());
    let method = match call.method {
        Method::Get => worker::Method::Get,
        Method::Post => worker::Method::Post,
    };
    let headers = Headers::new();
    for (name, value) in &call.headers {
        headers
            .set(name, value)
            .map_err(|_| failed("invalid header"))?;
    }
    let mut init = RequestInit::new();
    init.with_method(method)
        .with_headers(headers)
        .with_redirect(RequestRedirect::Manual);
    if !call.body.is_empty() {
        init.with_body(Some(
            worker::js_sys::Uint8Array::from(&call.body[..]).into(),
        ));
    }
    let request = Request::new_with_init(&call.url, &init).map_err(|_| failed("invalid URL"))?;
    let controller = AbortController::default();
    let signal = controller.signal();
    let fetch = Fetch::Request(request);
    let url = call.url.clone();
    let answer = async {
        let mut response = fetch.send_with_signal(&signal).await.map_err(|e| {
            tracing::warn!(url, error = %e, "outbound call: unreachable");
            failed("unreachable")
        })?;
        let status = response.status_code();
        if !(200..300).contains(&status) {
            return Err(CallError::Failed(format!("answered HTTP {status}")));
        }
        if call.discard_answer {
            return Ok(Vec::new());
        }
        read_body(&mut response, MAX_ANSWER_BYTES, Overflow::Fail)
            .await
            .map_err(CallError::Failed)
    };
    // The whole call, answer included, gets `timeout`; then it's aborted.
    match select(pin!(answer), pin!(Delay::from(call.timeout))).await {
        Either::Left((result, _)) => result,
        Either::Right(((), _)) => {
            controller.abort();
            Err(CallError::Timeout)
        }
    }
}

/// What happens when a body is longer than its limit.
enum Overflow {
    /// It's an error (an app's answer).
    Fail,
    /// The rest is left unread (the media storage's reads, `HttpRequest::body_limit`).
    Cut,
}

/// Reads `response`'s body chunk by chunk, so an endless one stops at `limit`.
async fn read_body(
    response: &mut worker::Response,
    limit: usize,
    overflow: Overflow,
) -> Result<Vec<u8>, String> {
    let too_big = || format!("answered more than {limit} bytes");
    if let Overflow::Fail = overflow
        && let Ok(Some(length)) = response.headers().get("content-length")
        && length.parse::<usize>().is_ok_and(|length| length > limit)
    {
        return Err(too_big());
    }
    // A response without a body (a HEAD, a 204) has nothing to stream.
    let Ok(mut stream) = response.stream() else {
        return Ok(Vec::new());
    };
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "the answer broke off".to_string())?;
        if body.len() + chunk.len() > limit {
            match overflow {
                Overflow::Fail => return Err(too_big()),
                Overflow::Cut => {
                    body.extend_from_slice(&chunk[..limit - body.len()]);
                    break;
                }
            }
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// How long one request to the media storage may take, as natively
/// (`hypha_store_s3::ReqwestClient`).
const MEDIA_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// The media storage's HTTP (R2 or another S3 service) through `fetch`, each request
/// aborted after `MEDIA_TIMEOUT`.
pub fn host_http() -> HostHttp {
    HostHttp::new(|request| {
        Box::pin(SendFuture::new(async move {
            let controller = AbortController::default();
            let signal = controller.signal();
            match select(
                pin!(send_http(request, &signal)),
                pin!(Delay::from(MEDIA_TIMEOUT)),
            )
            .await
            {
                Either::Left((result, _)) => result,
                Either::Right(((), _)) => {
                    controller.abort();
                    Err(format!("no answer within {} s", MEDIA_TIMEOUT.as_secs()))
                }
            }
        }))
    })
}

async fn send_http(
    request: HttpRequest,
    signal: &worker::AbortSignal,
) -> Result<HttpResponse, String> {
    let headers = Headers::new();
    for (name, value) in &request.headers {
        headers
            .set(name, value)
            .map_err(|e| format!("invalid header {name:?}: {e}"))?;
    }
    let mut init = RequestInit::new();
    init.with_method(worker::Method::from(request.method.to_string()))
        .with_headers(headers);
    let mut response =
        Fetch::Request(Request::new_with_init(&request.url, &init).map_err(|e| e.to_string())?)
            .send_with_signal(signal)
            .await
            .map_err(|e| e.to_string())?;
    let status = response.status_code();
    let headers = response.headers().entries().collect();
    let limit = request
        .body_limit
        .map_or(usize::MAX, |l| usize::try_from(l).unwrap_or(usize::MAX));
    let body = read_body(&mut response, limit, Overflow::Cut).await?;
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}
