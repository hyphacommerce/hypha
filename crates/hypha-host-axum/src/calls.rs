//! Sending calls to merchant-configured URLs (`hypha_http::outbound`, which signs and
//! checks them): `reqwest` with no redirects, a short connect timeout and, unless
//! `outbound.allow_private`, a resolver that keeps only public addresses, so a name that
//! resolves to a private address (or is re-pointed at one later) is refused at
//! connection time.

use std::net::SocketAddr;
use std::sync::Arc;

use hypha_http::host::HostCalls;
use hypha_http::outbound::{
    CallError, MAX_ANSWER_BYTES, Method, OutboundCall, OutboundPolicy, is_public,
};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// The client for merchant-configured URLs under `policy`.
pub fn client(policy: OutboundPolicy) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none());
    if !policy.allow_private {
        builder = builder.dns_resolver(Arc::new(PublicOnly));
    }
    builder
        .build()
        .map_err(|e| format!("cannot build the HTTP client: {e}"))
}

/// The host's `HostCalls`, sending through `client`.
pub fn host_calls(client: reqwest::Client) -> HostCalls {
    HostCalls::new(move |call| {
        let client = client.clone();
        Box::pin(async move { send(&client, call).await })
    })
}

/// Sends `call` and returns the body of a 2xx answer, read up to `MAX_ANSWER_BYTES`
/// (empty when `discard_answer`). Doesn't follow redirects.
pub async fn send(client: &reqwest::Client, call: OutboundCall) -> Result<Vec<u8>, CallError> {
    let method = match call.method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
    };
    let mut request = client.request(method, &call.url).timeout(call.timeout);
    for (name, value) in &call.headers {
        request = request.header(name, value);
    }
    let url = call.url.clone();
    let response = request.body(call.body).send().await.map_err(|e| {
        if e.is_timeout() {
            CallError::Timeout
        } else {
            tracing::warn!(url, error = %e, "outbound call: unreachable");
            CallError::Failed("unreachable".to_string())
        }
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(CallError::Failed(format!(
            "answered HTTP {}",
            status.as_u16()
        )));
    }
    if call.discard_answer {
        return Ok(Vec::new());
    }
    // Read chunk by chunk so an endless answer stops at the cap, not at the timeout.
    let too_big = || CallError::Failed(format!("answered more than {MAX_ANSWER_BYTES} bytes"));
    if response
        .content_length()
        .is_some_and(|len| len > MAX_ANSWER_BYTES as u64)
    {
        return Err(too_big());
    }
    let mut response = response;
    let mut answer = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| {
        if e.is_timeout() {
            CallError::Timeout
        } else {
            CallError::Failed("the answer broke off".to_string())
        }
    })? {
        if answer.len() + chunk.len() > MAX_ANSWER_BYTES {
            return Err(too_big());
        }
        answer.extend_from_slice(&chunk);
    }
    Ok(answer)
}

/// Resolves with the system resolver and keeps only public addresses; a name with none
/// fails to resolve.
struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let found: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| is_public(a.ip()))
                .collect();
            if found.is_empty() {
                return Err(format!("{host} has no public address").into());
            }
            let addrs: Addrs = Box::new(found.into_iter());
            Ok(addrs)
        })
    }
}
