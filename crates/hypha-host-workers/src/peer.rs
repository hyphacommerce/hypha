//! The client's address on Workers (design note 0026, W8; 0009). From the internet,
//! Cloudflare sets `CF-Connecting-IP` and a client can't. The admin and storefront
//! Workers call the API through Service Bindings, which bypass the edge: there the header
//! is whatever the caller wrote, so it isn't read. A call that carries the deployment's
//! caller key (`X-Hypha-Caller`, a secret only our Workers hold) is one of ours, and then
//! the caller's `X-Forwarded-For` names the client, as a trusted proxy's does natively.
//!
//! Not the binding's props, which would need no secret: Cloudflare drops them when the
//! called Worker has a placement, and the API's is next to its database (W12).
//!
//! A co-located app (design note 0029, §9) calls through a binding too, without the key
//! (apps are untrusted): its calls carry no `CF-Connecting-IP` (Cloudflare sets it on
//! every request from the internet), and they share one address, `BOUND_APPS`, rather
//! than fail for want of one. Not the one our own Workers' calls made for no client
//! share (`OUR_WORKERS`): rate limits key by address (an IPv6 one by its /64), so an
//! app spending its limit can't spend our Workers' too.
//! Portable, so its tests run natively.

use std::net::{IpAddr, Ipv6Addr};

/// The address our own Workers' calls made for no client share (`::`).
pub const OUR_WORKERS: IpAddr = IpAddr::V6(Ipv6Addr::UNSPECIFIED);

/// The address calls from co-located apps through their bindings share: in the
/// discard-only prefix `100::/64` (RFC 6666), so it's no client's and its /64 isn't
/// `OUR_WORKERS`'.
pub const BOUND_APPS: IpAddr = IpAddr::V6(Ipv6Addr::new(0x100, 0, 0, 0, 0, 0, 0, 1));

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
                .unwrap_or(OUR_WORKERS),
        )
    } else {
        match connecting_ip {
            Some(ip) => ip.trim().parse().ok(),
            // Only a Service Binding's call lacks it: a co-located app's.
            None => Some(BOUND_APPS),
        }
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
        let ip = client_address(false, Some("203.0.113.7"), Some("198.51.100.1"));
        assert_eq!(ip, "203.0.113.7".parse().ok());
    }

    #[test]
    fn a_bound_app_without_the_key_shares_one_address_whatever_it_forwards() {
        let shared = Some(BOUND_APPS);
        assert_eq!(client_address(false, None, Some("198.51.100.1")), shared);
        assert_eq!(client_address(false, None, None), shared);
    }

    #[test]
    fn a_connecting_address_that_isnt_one_is_none() {
        assert_eq!(client_address(false, Some("garbage"), None), None);
    }

    #[test]
    fn from_our_workers_the_callers_last_hop_is_the_client() {
        let ip = client_address(true, Some("10.0.0.1"), Some("192.0.2.9, 198.51.100.1"));
        assert_eq!(ip, "198.51.100.1".parse().ok());
    }

    #[test]
    fn from_our_workers_without_a_client_every_call_shares_one_address() {
        let shared = Some(OUR_WORKERS);
        assert_eq!(client_address(true, Some("203.0.113.7"), None), shared);
        assert_eq!(client_address(true, None, Some("not an ip")), shared);
    }

    #[test]
    fn bound_apps_and_our_workers_without_a_client_count_in_different_buckets() {
        let apps = client_address(false, None, None).unwrap();
        let ours = client_address(true, None, None).unwrap();
        assert_ne!(apps, ours);
        // Limits key IPv6 by its /64: the buckets differ there too.
        use hypha_http::client_ip::limit_key;
        assert_ne!(limit_key(apps), limit_key(ours));
        assert_eq!(limit_key(apps), "100::/64");
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
