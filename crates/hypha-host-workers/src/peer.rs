//! The client's address on Workers (design note 0026, W8; 0009). From the internet,
//! Cloudflare sets `CF-Connecting-IP` and a client can't. The admin and storefront
//! Workers call the API through Service Bindings, which bypass the edge: there the header
//! is whatever the caller wrote, so it isn't read. A binding's props, which only the
//! deployment's configuration sets (`{ caller = "storefront" }`), mark the call as one of
//! ours, and then the caller's `X-Forwarded-For` names the client, as a trusted proxy's
//! does natively. Portable, so its tests run natively.

use std::net::IpAddr;

/// Who called: `binding` is the caller a Service Binding's props name, if any.
pub fn client_address(
    binding: Option<&str>,
    connecting_ip: Option<&str>,
    forwarded_for: Option<&str>,
) -> Option<IpAddr> {
    match binding {
        // The last hop the caller appended: the address of whoever called it.
        Some(_) => forwarded_for?.rsplit(',').next()?.trim().parse().ok(),
        None => connecting_ip?.trim().parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_the_internet_cloudflares_header_is_the_client() {
        let ip = client_address(None, Some("203.0.113.7"), Some("198.51.100.1"));
        assert_eq!(ip, "203.0.113.7".parse().ok());
    }

    #[test]
    fn from_the_internet_a_forwarded_header_is_ignored() {
        assert_eq!(client_address(None, None, Some("198.51.100.1")), None);
    }

    #[test]
    fn through_a_binding_the_callers_last_hop_is_the_client() {
        let ip = client_address(
            Some("storefront"),
            Some("10.0.0.1"),
            Some("192.0.2.9, 198.51.100.1"),
        );
        assert_eq!(ip, "198.51.100.1".parse().ok());
    }

    #[test]
    fn through_a_binding_a_connecting_ip_alone_counts_for_nothing() {
        assert_eq!(
            client_address(Some("admin"), Some("203.0.113.7"), None),
            None
        );
        assert_eq!(client_address(Some("admin"), None, Some("not an ip")), None);
    }
}
