//! Calls to URLs a merchant configured (rate apps now, webhooks with 0016): only to
//! public addresses, so a callback URL can't reach the host's own network (cloud
//! metadata at 169.254.169.254, the database, internal services). The check runs on
//! the addresses a name resolves to, at connection time, so a name that resolves to a
//! private address (or is re-pointed at one later) is refused too.
//!
//! `HYPHA_ALLOW_PRIVATE_CALLBACKS=true` lifts it, for development and tests that run an
//! app on localhost.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// Whether calls may reach private, loopback and other non-public addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OutboundPolicy {
    pub allow_private: bool,
}

impl OutboundPolicy {
    /// From `HYPHA_ALLOW_PRIVATE_CALLBACKS` (`true` allows; anything else doesn't).
    pub fn from_env() -> Self {
        Self {
            allow_private: std::env::var("HYPHA_ALLOW_PRIVATE_CALLBACKS").as_deref() == Ok("true"),
        }
    }

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
