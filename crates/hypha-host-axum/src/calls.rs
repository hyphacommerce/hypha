//! Sending calls to merchant-configured URLs (`hypha_http::outbound`, which signs and
//! checks them): `reqwest` with no redirects, a short connect timeout and, unless
//! `outbound.allow_private`, a resolver that keeps only public addresses, so a name that
//! resolves to a private address (or is re-pointed at one later) is refused at
//! connection time.

use std::net::SocketAddr;
use std::sync::Arc;

use hypha_http::host::HostCalls;
use hypha_http::outbound::{
    CallError, Method, OutboundAnswer, OutboundCall, OutboundPolicy, is_public,
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

/// Sends `call` and returns its answer: the status, and the body read up to
/// `call.max_answer` (empty when `discard_answer`; a longer 2xx body is `TooLarge`, a
/// longer one of another status is left unread). Doesn't follow redirects.
pub async fn send(
    client: &reqwest::Client,
    call: OutboundCall,
) -> Result<OutboundAnswer, CallError> {
    let method = match call.method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
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
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let answer = |body| OutboundAnswer {
        status,
        body,
        content_type: content_type.clone(),
    };
    let max = call.max_answer;
    if call.discard_answer {
        return Ok(answer(Vec::new()));
    }
    let success = (200..300).contains(&status);
    // Too long: a failure for a 2xx (its body is the answer), nothing to read for any
    // other (its body only names an error).
    let too_big = || match success {
        true => Err(CallError::TooLarge),
        false => Ok(answer(Vec::new())),
    };
    if response
        .content_length()
        .is_some_and(|len| len > max as u64)
    {
        return too_big();
    }
    // Read chunk by chunk so an endless answer stops at the cap, not at the timeout.
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| {
        if e.is_timeout() {
            CallError::Timeout
        } else {
            CallError::Failed("the answer broke off".to_string())
        }
    })? {
        if body.len() + chunk.len() > max {
            return too_big();
        }
        body.extend_from_slice(&chunk);
    }
    Ok(answer(body))
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A server on loopback answering every connection with an empty 200.
    async fn local_server() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    .await;
            }
        });
        port
    }

    fn get(port: u16) -> OutboundCall {
        OutboundCall {
            method: Method::Get,
            url: format!("http://localhost:{port}/"),
            timeout: std::time::Duration::from_secs(5),
            headers: Vec::new(),
            body: Vec::new(),
            discard_answer: false,
            max_answer: 1024,
            purpose: hypha_http::outbound::CallPurpose::Other,
        }
    }

    // A name, unlike an IP literal, is only checked when it resolves: this is the guard
    // that keeps a merchant's URL from reaching the host's own network.
    #[tokio::test]
    async fn a_name_resolving_to_loopback_is_refused() {
        let port = local_server().await;
        let client = client(OutboundPolicy {
            allow_private: false,
        })
        .unwrap();
        assert_eq!(
            send(&client, get(port)).await,
            Err(CallError::Failed("unreachable".to_string()))
        );
    }

    #[tokio::test]
    async fn allow_private_reaches_loopback() {
        let port = local_server().await;
        let client = client(OutboundPolicy {
            allow_private: true,
        })
        .unwrap();
        let answer = send(&client, get(port)).await.map(|a| a.status);
        assert_eq!(answer, Ok(200));
    }
}
