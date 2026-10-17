//! Calls to URLs a merchant configured (rate apps now, webhooks with 0016): only to
//! public addresses, so a callback URL can't reach the host's own network (cloud
//! metadata at 169.254.169.254, the database, internal services). The check runs on
//! the addresses a name resolves to, at connection time, so a name that resolves to a
//! private address (or is re-pointed at one later) is refused too.
//!
//! `outbound.allow_private` (`HYPHA_ALLOW_PRIVATE_CALLBACKS=true`) lifts it, for
//! development and tests that run an app on localhost.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// Whether calls may reach private, loopback and other non-public addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OutboundPolicy {
    pub allow_private: bool,
}

impl OutboundPolicy {
    /// Whether a URL whose host is the IP literal `ip` may be called. Names are checked
    /// by the client's resolver instead; literals never reach it.
    pub fn allows(self, ip: IpAddr) -> bool {
        self.allow_private || is_public(ip)
    }
}

/// The HTTP client for merchant-configured URLs: no redirects (a public URL could
/// redirect to a private one), a short connect timeout, and, unless the policy allows
/// private addresses, a resolver that drops every non-public address.
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

/// The header carrying a signed call's signature: `t=<unix seconds>,v1=<hex
/// HMAC-SHA256 of "<t>.<body>" under the app's secret>` (Stripe's scheme), for rate
/// callbacks and webhooks alike.
pub const SIGNATURE_HEADER: &str = "Hypha-Signature";
/// The most of an answer read: rates for a cart are a few kilobytes, and a bigger
/// answer counts as a failure rather than filling memory.
pub const MAX_ANSWER_BYTES: usize = 256 * 1024;

/// A signed call to a merchant-configured URL.
pub struct SignedCall<'a> {
    pub method: reqwest::Method,
    pub url: &'a str,
    pub timeout: std::time::Duration,
    /// The app's secret, in plain text.
    pub secret: &'a str,
    /// Unix seconds, for the signature.
    pub at: i64,
    /// Further headers (request or event IDs).
    pub headers: &'a [(&'static str, String)],
    pub body: Vec<u8>,
    /// Whether the answer's body is ignored (webhooks): then a 2xx is success whatever
    /// follows, and nothing is read.
    pub discard_answer: bool,
}

/// Why a signed call failed. `Failed` carries what a merchant may see: transport
/// details (refused, TLS, no public address) are only logged, since in an answer they
/// would map the network behind the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    Timeout,
    Failed(String),
}

/// Sends `call` and returns the body of a 2xx answer, read up to `MAX_ANSWER_BYTES`
/// (empty when `discard_answer`).
/// Refuses an IP literal that isn't public under `policy` (names are checked by the
/// client's resolver, `client`), and doesn't follow redirects.
pub async fn send_signed(
    client: &reqwest::Client,
    policy: OutboundPolicy,
    call: SignedCall<'_>,
) -> Result<Vec<u8>, CallError> {
    let literal = reqwest::Url::parse(call.url).ok().and_then(|u| {
        let host = u.host_str()?.trim_start_matches('[').trim_end_matches(']');
        host.parse::<IpAddr>().ok()
    });
    if literal.is_some_and(|ip| !policy.allows(ip)) {
        return Err(CallError::Failed(
            "the URL is not a public address".to_string(),
        ));
    }
    let mut request = client
        .request(call.method, call.url)
        .timeout(call.timeout)
        .header("content-type", "application/json")
        .header(SIGNATURE_HEADER, sign(call.secret, call.at, &call.body));
    for (name, value) in call.headers {
        request = request.header(*name, value);
    }
    let response = request.body(call.body).send().await.map_err(|e| {
        if e.is_timeout() {
            CallError::Timeout
        } else {
            tracing::warn!(url = call.url, error = %e, "outbound call: unreachable");
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

/// `t=<at>,v1=<hex HMAC-SHA256 of "<at>.<body>">`.
pub fn sign(secret: &str, at: i64, body: &[u8]) -> String {
    use hmac::{Hmac, KeyInit, Mac};
    // HMAC takes keys of any length.
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC accepts every key length"));
    mac.update(at.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("t={at},v1={hex}")
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

/// Whether `ip` is a globally routable unicast address: not loopback, private,
/// link-local, shared (carrier NAT), documentation, benchmarking, multicast, reserved
/// or unspecified. IPv4-mapped IPv6 addresses are judged as their IPv4 address.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        // 100.64.0.0/10, shared address space (carrier-grade NAT).
        || (a == 100 && (b & 0xc0) == 64)
        // 192.0.0.0/24, IETF protocol assignments.
        || (a == 192 && b == 0 && c == 0)
        // 198.18.0.0/15, benchmarking.
        || (a == 198 && (b & 0xfe) == 18)
        // 240.0.0.0/4, reserved.
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        // 2001:db8::/32, documentation.
        || (first == 0x2001 && ip.segments()[1] == 0x0db8)
        // 64:ff9b:1::/48, local-use NAT64; and ::/96, IPv4-compatible (deprecated).
        || (first == 0x0064 && ip.segments()[1] == 0xff9b && ip.segments()[2] == 1)
        || ip.segments()[..6] == [0; 6])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(s: &str) -> bool {
        is_public(s.parse().unwrap())
    }

    #[test]
    fn internal_addresses_are_not_public() {
        for ip in [
            "127.0.0.1",
            "10.0.0.5",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "198.18.0.1",
            "240.0.0.1",
            "::1",
            "::",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "2001:db8::1",
        ] {
            assert!(!public(ip), "{ip} counted as public");
        }
    }

    #[test]
    fn internet_addresses_are_public() {
        for ip in [
            "1.1.1.1",
            "8.8.8.8",
            "100.128.0.1",
            "2606:4700::1111",
            "::ffff:1.1.1.1",
        ] {
            assert!(public(ip), "{ip} counted as private");
        }
    }

    #[test]
    fn the_policy_allows_private_literals_only_when_told_to() {
        let ip: IpAddr = "10.0.0.5".parse().unwrap();
        assert!(!OutboundPolicy::default().allows(ip));
        assert!(
            OutboundPolicy {
                allow_private: true
            }
            .allows(ip)
        );
    }
}
