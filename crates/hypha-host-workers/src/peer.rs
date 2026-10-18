//! The client's address on Workers (design note 0026, W8; 0009). From the internet,
//! Cloudflare sets `CF-Connecting-IP` and a client can't. The admin and storefront
//! Workers call the API through Service Bindings, which bypass the edge: there the header
//! is whatever the caller wrote, so it isn't read. A call that carries the deployment's
//! caller key (`X-Hypha-Caller`, a secret only our Workers hold) is one of ours, and then
//! the caller's `X-Forwarded-For` names the client, as a trusted proxy's does natively.
//!
//! Not the binding's props, which would need no secret: Cloudflare drops them when the
//! called Worker has a placement, and the API's is next to its database (W12).
//! Portable, so its tests run natively.

use std::net::{IpAddr, Ipv6Addr};

/// The header our Workers send the caller key in.
pub const CALLER_HEADER: &str = "x-hypha-caller";

/// Whether `sent` is the deployment's caller `key`. A deployment without one (no apps
/// bound) trusts no caller. Compared in constant time, so the key can't be guessed a byte
/// at a time from response times.
pub fn is_ours(key: Option<&str>, sent: Option<&str>) -> bool {
    let (Some(key), Some(sent)) = (key.filter(|k| !k.is_empty()), sent) else {
        return false;
    };
    let (key, sent) = (key.as_bytes(), sent.as_bytes());
    key.len() == sent.len() && key.iter().zip(sent).fold(0, |d, (a, b)| d | (a ^ b)) == 0
}

/// Who called: `ours` when the call carried the caller key.
pub fn client_address(
    ours: bool,
    connecting_ip: Option<&str>,
    forwarded_for: Option<&str>,
) -> Option<IpAddr> {
    if ours {
        // The last hop the caller appended: the address of whoever called it. Without
        // one (a call made for no client), every such call shares one address: limits
        // count them together rather than refusing them.
        Some(
            forwarded_for
                .and_then(|f| f.rsplit(',').next())
                .and_then(|ip| ip.trim().parse().ok())
                .unwrap_or(IpAddr::V6(Ipv6Addr::UNSPECIFIED)),
        )
    } else {
        connecting_ip?.trim().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_the_internet_cloudflares_header_is_the_client() {
        let ip = client_address(false, Some("203.0.113.7"), Some("198.51.100.1"));
        assert_eq!(ip, "203.0.113.7".parse().ok());
    }

    #[test]
    fn from_the_internet_a_forwarded_header_is_ignored() {
        assert_eq!(client_address(false, None, Some("198.51.100.1")), None);
    }

    #[test]
    fn from_our_workers_the_callers_last_hop_is_the_client() {
        let ip = client_address(true, Some("10.0.0.1"), Some("192.0.2.9, 198.51.100.1"));
        assert_eq!(ip, "198.51.100.1".parse().ok());
    }

    #[test]
    fn from_our_workers_without_a_client_every_call_shares_one_address() {
        let shared = Some(IpAddr::V6(Ipv6Addr::UNSPECIFIED));
        assert_eq!(client_address(true, Some("203.0.113.7"), None), shared);
        assert_eq!(client_address(true, None, Some("not an ip")), shared);
    }

    #[test]
    fn only_the_key_itself_is_ours() {
        assert!(is_ours(Some("k3y"), Some("k3y")));
        assert!(!is_ours(Some("k3y"), Some("k3z")));
        assert!(!is_ours(Some("k3y"), Some("k3")));
        assert!(!is_ours(Some("k3y"), None));
    }

    #[test]
    fn without_a_key_no_caller_is_ours() {
        assert!(!is_ours(None, Some("")));
        assert!(!is_ours(Some(""), Some("")));
    }
}
