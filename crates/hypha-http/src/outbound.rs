//! Calls to URLs a merchant configured (rate apps now, webhooks with 0016): only to
//! public addresses, so a callback URL can't reach the host's own network (cloud
//! metadata at 169.254.169.254, the database, internal services). The check runs on
//! the addresses a name resolves to, at connection time, so a name that resolves to a
//! private address (or is re-pointed at one later) is refused too.
//!
//! `outbound.allow_private` (`HYPHA_ALLOW_PRIVATE_CALLBACKS=true`) lifts it, for
//! development and tests that run an app on localhost.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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

/// The header carrying a signed call's signature: `t=<unix seconds>,v1=<hex
/// HMAC-SHA256 of "<t>.<body>" under the app's secret>` (Stripe's scheme), for rate
/// callbacks and webhooks alike.
pub const SIGNATURE_HEADER: &str = "Hypha-Signature";
/// The most of an answer read: rates for a cart are a few kilobytes, and a bigger
/// answer counts as a failure rather than filling memory.
pub const MAX_ANSWER_BYTES: usize = 256 * 1024;

/// The methods merchant URLs are called with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// A signed call to a merchant-configured URL.
pub struct SignedCall<'a> {
    pub method: Method,
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

/// A call ready to send: signed, its URL checked, every header set. What the host's
/// `HostCalls` takes; it adds the transport (a connect timeout, no redirects, natively
/// a resolver that keeps only public addresses).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundCall {
    pub method: Method,
    pub url: String,
    pub timeout: std::time::Duration,
    /// Every header, the signature and content type included.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The answer's body isn't read: a 2xx is success.
    pub discard_answer: bool,
}

/// Signs `call` and checks its URL: an IP literal that isn't public under `policy` is
/// refused here (names are the host's to check, when it resolves them).
pub fn prepare(policy: OutboundPolicy, call: SignedCall<'_>) -> Result<OutboundCall, CallError> {
    let literal = url_host(call.url).and_then(|host| host.parse::<IpAddr>().ok());
    if literal.is_some_and(|ip| !policy.allows(ip)) {
        return Err(CallError::Failed(
            "the URL is not a public address".to_string(),
        ));
    }
    let mut headers = vec![
        ("content-type".to_string(), "application/json".to_string()),
        (
            SIGNATURE_HEADER.to_string(),
            sign(call.secret, call.at, &call.body),
        ),
    ];
    headers.extend(
        call.headers
            .iter()
            .map(|(n, v)| ((*n).to_string(), v.clone())),
    );
    Ok(OutboundCall {
        method: call.method,
        url: call.url.to_string(),
        timeout: call.timeout,
        headers,
        body: call.body,
        discard_answer: call.discard_answer,
    })
}

/// The host of an `http(s)://` URL, brackets removed from an IPv6 literal.
fn url_host(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(v6) = host_port.strip_prefix('[') {
        v6.split(']').next()?
    } else {
        host_port.split(':').next()?
    };
    (!host.is_empty()).then_some(host)
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
    fn a_urls_host_is_read_without_port_path_or_brackets() {
        assert_eq!(
            url_host("https://rates.example.com:8443/quote?x=1"),
            Some("rates.example.com")
        );
        assert_eq!(url_host("http://user:pw@10.0.0.1/hook"), Some("10.0.0.1"));
        assert_eq!(url_host("https://[::1]:3000/hook"), Some("::1"));
        assert_eq!(url_host("not a url"), None);
        assert_eq!(url_host("https:///nohost"), None);
    }

    fn call<'a>(url: &'a str, headers: &'a [(&'static str, String)]) -> SignedCall<'a> {
        SignedCall {
            method: Method::Post,
            url,
            timeout: std::time::Duration::from_secs(5),
            secret: "s3cret",
            at: 1_700_000_000,
            headers,
            body: b"{}".to_vec(),
            discard_answer: false,
        }
    }

    #[test]
    fn a_prepared_call_carries_its_signature_and_headers() {
        let headers = [("Hypha-Request-Id", String::from("rreq_1"))];
        let prepared = prepare(
            OutboundPolicy::default(),
            call("https://rates.example.com/q", &headers),
        )
        .unwrap();
        assert_eq!(
            prepared.headers,
            vec![
                ("content-type".to_string(), "application/json".to_string()),
                (
                    SIGNATURE_HEADER.to_string(),
                    sign("s3cret", 1_700_000_000, b"{}")
                ),
                ("Hypha-Request-Id".to_string(), "rreq_1".to_string()),
            ]
        );
    }

    #[test]
    fn a_private_literal_is_refused_before_any_call() {
        let refused = prepare(
            OutboundPolicy::default(),
            call("http://169.254.169.254/latest", &[]),
        );
        assert_eq!(
            refused,
            Err(CallError::Failed(
                "the URL is not a public address".to_string()
            ))
        );
        let allowed = OutboundPolicy {
            allow_private: true,
        };
        assert!(prepare(allowed, call("http://127.0.0.1:9000/hook", &[])).is_ok());
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
