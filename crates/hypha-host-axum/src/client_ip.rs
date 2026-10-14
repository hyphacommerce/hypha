//! Which client a request comes from, for rate limits (design note 0009, R4 and R5).
//!
//! The socket's peer address, unless that peer is a proxy listed in
//! `HYPHA_TRUSTED_PROXIES`: then `X-Forwarded-For`, read right to left up to the first
//! address that isn't a trusted proxy. Without trusted proxies the header is ignored:
//! anyone can send one, and a limit keyed by it would be a limit the attacker picks.

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{HeaderMap, request::Parts},
};
use ipnet::IpNet;

use crate::{
    error::ApiError,
    state::{SharedState, Storage},
};

/// The proxies whose `X-Forwarded-For` is believed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<IpNet>);

impl TrustedProxies {
    /// A comma-separated list of CIDR ranges (`10.0.0.0/8,fd00::/8`); a bare address
    /// means just that address.
    pub fn parse(list: &str) -> Result<Self, String> {
        list.split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(|part| {
                part.parse::<IpNet>()
                    .or_else(|_| part.parse::<IpAddr>().map(IpNet::from))
                    .map_err(|_| format!("{part:?} is not an IP address or CIDR range"))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn contains(&self, ip: IpAddr) -> bool {
        self.0.iter().any(|net| net.contains(&ip))
    }

    /// The client's address for a request from `peer` carrying `headers`.
    pub fn client(&self, peer: IpAddr, headers: &HeaderMap) -> IpAddr {
        let peer = canonical(peer);
        if !self.contains(peer) {
            return peer;
        }
        // Every `X-Forwarded-For` header, in order: each proxy appends the address it
        // got the request from.
        let hops: Vec<&str> = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .collect();
        let mut client = peer;
        for hop in hops.iter().rev() {
            // A hop that isn't an address can't be followed further; the last trusted
            // proxy is the best that's known.
            let Ok(ip) = hop.parse::<IpAddr>().map(canonical) else {
                return client;
            };
            client = ip;
            if !self.contains(ip) {
                return ip;
            }
        }
        client
    }
}

/// IPv4 addresses written as IPv6 (`::ffff:1.2.3.4`, from dual-stack sockets) as IPv4,
/// so one client has one key.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    }
}

/// The rate-limit key for `ip`: an IPv4 address whole, an IPv6 address by its /64,
/// since one customer usually gets a whole /64 and could otherwise rotate through
/// billions of keys (0009, R5).
pub fn limit_key(ip: IpAddr) -> String {
    match canonical(ip) {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let prefix = u128::from(v6) & !((1u128 << 64) - 1);
            format!("{}/64", std::net::Ipv6Addr::from(prefix))
        }
    }
}

/// The calling client's rate-limit key (`limit_key` of its address). Needs the server
/// to record peer addresses (`into_make_service_with_connect_info`), as `main` does.
pub struct ClientIp(pub String);

impl<S: Storage> FromRequestParts<SharedState<S>> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState<S>,
    ) -> Result<Self, ApiError> {
        // Through axum's extractor, which also takes the address tests set with
        // `MockConnectInfo`.
        let ConnectInfo(peer) = ConnectInfo::<SocketAddr>::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::internal("the server records no peer addresses"))?;
        let ip = state.trusted_proxies.client(peer.ip(), &parts.headers);
        Ok(ClientIp(limit_key(ip)))
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn forwarded(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for v in values {
            headers.append("x-forwarded-for", HeaderValue::from_str(v).unwrap());
        }
        headers
    }

    fn proxies(list: &str) -> TrustedProxies {
        TrustedProxies::parse(list).unwrap()
    }

    #[test]
    fn without_trusted_proxies_the_header_is_ignored() {
        let headers = forwarded(&["6.6.6.6"]);
        assert_eq!(
            TrustedProxies::default().client(ip("1.2.3.4"), &headers),
            ip("1.2.3.4")
        );
    }

    #[test]
    fn a_request_not_from_a_trusted_proxy_keeps_its_peer() {
        let headers = forwarded(&["6.6.6.6"]);
        assert_eq!(
            proxies("10.0.0.0/8").client(ip("1.2.3.4"), &headers),
            ip("1.2.3.4")
        );
    }

    #[test]
    fn behind_proxies_the_first_untrusted_hop_from_the_right_is_the_client() {
        // The client sent a forged first entry; the load balancer (10.0.0.1) appended
        // the real client, and an inner proxy (10.0.0.2) appended the balancer.
        let headers = forwarded(&["6.6.6.6, 1.2.3.4", "10.0.0.1"]);
        assert_eq!(
            proxies("10.0.0.0/8").client(ip("10.0.0.2"), &headers),
            ip("1.2.3.4")
        );
    }

    #[test]
    fn a_junk_hop_stops_the_walk_at_the_last_trusted_proxy() {
        let headers = forwarded(&["1.2.3.4, nonsense"]);
        assert_eq!(
            proxies("10.0.0.0/8").client(ip("10.0.0.1"), &headers),
            ip("10.0.0.1")
        );
    }

    #[test]
    fn parse_takes_ranges_and_bare_addresses_and_refuses_junk() {
        assert_eq!(proxies(" 10.0.0.0/8 , fd00::/8,127.0.0.1 ").0.len(), 3);
        assert!(proxies("").is_empty());
        assert!(TrustedProxies::parse("10.0.0.0/8,nope").is_err());
    }

    #[test]
    fn ipv6_is_keyed_by_its_64_and_mapped_ipv4_as_ipv4() {
        assert_eq!(limit_key(ip("2001:db8:1:2:aaaa::1")), "2001:db8:1:2::/64");
        assert_eq!(
            limit_key(ip("2001:db8:1:2:aaaa::1")),
            limit_key(ip("2001:db8:1:2:ffff::9"))
        );
        assert_ne!(
            limit_key(ip("2001:db8:1:2::1")),
            limit_key(ip("2001:db8:1:3::1"))
        );
        assert_eq!(limit_key(ip("::ffff:1.2.3.4")), "1.2.3.4");
        assert_eq!(limit_key(ip("1.2.3.4")), "1.2.3.4");
    }
}
