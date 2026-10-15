//! URLs Hypha calls on a merchant's behalf (design note 0015, section 5; 0016, section 8):
//! rate apps' callbacks and webhook endpoints. Hosts also check, when calling, that the
//! address is public (`outbound.rs` in the axum host); this checks the URL itself.

use thiserror::Error;

/// `https://`, or `http://` to a local host for development. No credentials in it (the
/// request is signed instead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackUrl(String);

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{0:?} is not an https URL (or http to localhost), without credentials")]
pub struct CallbackUrlError(pub String);

impl crate::ErrorCode for CallbackUrlError {
    fn code(&self) -> &'static str {
        "invalid_callback_url"
    }
}

impl CallbackUrl {
    pub const MAX_LEN: usize = 2048;

    /// Parsed with the WHATWG rules HTTP clients follow, so the host checked here is the
    /// host called: splitting the text by hand let `http://localhost:80@evil.example`
    /// through as "localhost", while a client sends it to `evil.example`.
    pub fn new(url: &str) -> Result<Self, CallbackUrlError> {
        let url = url.trim();
        let invalid = || CallbackUrlError(url.to_string());
        if url.len() > Self::MAX_LEN || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(invalid());
        }
        let parsed = url::Url::parse(url).map_err(|_| invalid())?;
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(invalid());
        }
        let is_local = match parsed.host() {
            Some(url::Host::Domain(name)) => name == "localhost" || name.ends_with(".localhost"),
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => return Err(invalid()),
        };
        match parsed.scheme() {
            "https" => {}
            "http" if is_local => {}
            _ => return Err(invalid()),
        }
        Ok(Self(url.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(url: &str) -> Result<CallbackUrl, CallbackUrlError> {
        Err(CallbackUrlError(url.to_string()))
    }

    #[test]
    fn credentials_cannot_disguise_another_host_as_local() {
        let url = "http://localhost:80@evil.example/rates";
        assert_eq!(CallbackUrl::new(url), invalid(url));
    }

    #[test]
    fn credentials_are_refused_over_https_too() {
        let url = "https://user:pass@rates.example/hypha";
        assert_eq!(CallbackUrl::new(url), invalid(url));
    }

    #[test]
    fn plain_http_to_loopback_is_allowed() {
        for url in [
            "http://localhost:4000/rates",
            "http://127.0.0.1:4000/rates",
            "http://[::1]:4000/rates",
            "http://rates.localhost/hypha",
        ] {
            assert!(CallbackUrl::new(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn plain_http_elsewhere_is_refused() {
        for url in [
            "http://rates.example/hypha",
            "http://localhost.example/rates",
        ] {
            assert_eq!(CallbackUrl::new(url), invalid(url), "{url}");
        }
    }

    #[test]
    fn other_schemes_are_refused() {
        let url = "ftp://localhost/rates";
        assert_eq!(CallbackUrl::new(url), invalid(url));
    }
}
