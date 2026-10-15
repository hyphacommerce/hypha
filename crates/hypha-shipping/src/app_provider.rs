//! External apps that calculate rates over HTTP (design note 0015, section 5): what
//! Hypha needs to call one, and the secret it signs those calls with. The calling itself
//! is the host's (`RateProvider`); this crate keeps the record.

use hypha_core::{Sealed, Version, Versioned, required_text};

use crate::{ProviderKey, ProviderKind, ServiceInfo, ShippingError};

/// The callback URL: `https://`, or `http://` to a local host for development. No
/// credentials in it (the request is signed instead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackUrl(String);

impl CallbackUrl {
    pub const MAX_LEN: usize = 2048;

    /// Parsed with the WHATWG rules HTTP clients follow, so the host checked here is the
    /// host called: splitting the text by hand let `http://localhost:80@evil.example`
    /// through as "localhost", while a client sends it to `evil.example`.
    pub fn new(url: &str) -> Result<Self, ShippingError> {
        let url = url.trim();
        let invalid = || ShippingError::InvalidCallbackUrl(url.to_string());
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppProvider {
    key: ProviderKey,
    version: Version,
    name: String,
    callback_url: CallbackUrl,
    timeout_ms: u32,
    /// The signing secret, sealed under the host's key with the provider key as context.
    secret: Sealed,
    /// Services the app declared up front; when empty, the admin asks the callback.
    services: Vec<ServiceInfo>,
    enabled: bool,
}

impl Versioned for AppProvider {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

/// A new app provider, checked by `AppProvider::new`.
pub struct NewAppProvider {
    pub key: ProviderKey,
    pub name: String,
    pub callback_url: String,
    pub timeout_ms: u32,
    pub secret: Sealed,
    pub services: Vec<ServiceInfo>,
}

impl AppProvider {
    pub const DEFAULT_TIMEOUT_MS: u32 = 3_000;
    pub const MIN_TIMEOUT_MS: u32 = 100;
    pub const MAX_TIMEOUT_MS: u32 = 10_000;

    pub fn new(new: NewAppProvider) -> Result<Self, ShippingError> {
        if new.key.kind() != ProviderKind::App {
            return Err(ShippingError::NotAnAppKey(new.key));
        }
        let mut provider = Self {
            key: new.key,
            version: Version::NEW,
            name: required_text("name", &new.name)?,
            callback_url: CallbackUrl::new(&new.callback_url)?,
            timeout_ms: Self::DEFAULT_TIMEOUT_MS,
            secret: new.secret,
            services: Vec::new(),
            enabled: true,
        };
        provider.set_timeout_ms(new.timeout_ms)?;
        provider.set_services(new.services)?;
        Ok(provider)
    }

    pub fn rename(&mut self, name: &str) -> Result<(), ShippingError> {
        self.name = required_text("name", name)?;
        Ok(())
    }

    pub fn set_callback_url(&mut self, url: &str) -> Result<(), ShippingError> {
        self.callback_url = CallbackUrl::new(url)?;
        Ok(())
    }

    pub fn set_timeout_ms(&mut self, timeout_ms: u32) -> Result<(), ShippingError> {
        if !(Self::MIN_TIMEOUT_MS..=Self::MAX_TIMEOUT_MS).contains(&timeout_ms) {
            return Err(ShippingError::InvalidTimeout {
                min: Self::MIN_TIMEOUT_MS,
                max: Self::MAX_TIMEOUT_MS,
            });
        }
        self.timeout_ms = timeout_ms;
        Ok(())
    }

    /// Codes and names follow the text rule; each code once.
    pub fn set_services(&mut self, services: Vec<ServiceInfo>) -> Result<(), ShippingError> {
        let mut checked = Vec::with_capacity(services.len());
        for service in services {
            let code = required_text("service code", &service.code)?;
            if checked.iter().any(|s: &ServiceInfo| s.code == code) {
                return Err(ShippingError::DuplicateServiceCode(code));
            }
            checked.push(ServiceInfo {
                code,
                name: required_text("service name", &service.name)?,
            });
        }
        self.services = checked;
        Ok(())
    }

    pub fn set_secret(&mut self, secret: Sealed) {
        self.secret = secret;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn key(&self) -> &ProviderKey {
        &self.key
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn callback_url(&self) -> &CallbackUrl {
        &self.callback_url
    }
    pub fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
    pub fn secret(&self) -> &Sealed {
        &self.secret
    }
    pub fn services(&self) -> &[ServiceInfo] {
        &self.services
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Plain-data form for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppProviderSnapshot {
    pub key: String,
    pub version: Version,
    pub name: String,
    pub callback_url: String,
    pub timeout_ms: u32,
    pub secret: Sealed,
    pub services: Vec<ServiceInfo>,
    pub enabled: bool,
}

impl AppProvider {
    pub fn snapshot(&self) -> AppProviderSnapshot {
        AppProviderSnapshot {
            key: self.key.as_str().to_string(),
            version: self.version,
            name: self.name.clone(),
            callback_url: self.callback_url.as_str().to_string(),
            timeout_ms: self.timeout_ms,
            secret: self.secret.clone(),
            services: self.services.clone(),
            enabled: self.enabled,
        }
    }

    pub fn restore(s: AppProviderSnapshot) -> Result<Self, ShippingError> {
        let mut provider = Self::new(NewAppProvider {
            key: ProviderKey::new(&s.key)?,
            name: s.name,
            callback_url: s.callback_url,
            timeout_ms: s.timeout_ms,
            secret: s.secret,
            services: s.services,
        })?;
        provider.enabled = s.enabled;
        provider.version = s.version;
        Ok(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(url: &str) -> Result<CallbackUrl, ShippingError> {
        Err(ShippingError::InvalidCallbackUrl(url.to_string()))
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
