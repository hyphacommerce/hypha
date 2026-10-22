//! External apps that calculate rates over HTTP: where Hypha calls one and what it quotes.
//! The app itself (its name, whether it's enabled, the secret calls are signed with) is
//! `hypha-apps`' record under the same key; storage links the two, so a provider needs its
//! app and goes with it. The calling is the host's (`RateProvider`).

use hypha_core::{CallbackUrl, Stamped, Stamps, Version, Versioned, required_text};

use crate::{ProviderKey, ProviderKind, ServiceInfo, ShippingError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppProvider {
    key: ProviderKey,
    version: Version,
    /// When it was stored and last changed, set by the adapter.
    stamps: Option<Stamps>,
    callback_url: CallbackUrl,
    timeout_ms: u32,
    /// Services the app declared up front; when empty, the admin asks the callback.
    services: Vec<ServiceInfo>,
}

impl Versioned for AppProvider {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for AppProvider {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

/// A new app provider, checked by `AppProvider::new`.
pub struct NewAppProvider {
    pub key: ProviderKey,
    pub callback_url: String,
    pub timeout_ms: u32,
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
            stamps: None,
            callback_url: CallbackUrl::new(&new.callback_url)?,
            timeout_ms: Self::DEFAULT_TIMEOUT_MS,
            services: Vec::new(),
        };
        provider.set_timeout_ms(new.timeout_ms)?;
        provider.set_services(new.services)?;
        Ok(provider)
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

    pub fn key(&self) -> &ProviderKey {
        &self.key
    }
    pub fn callback_url(&self) -> &CallbackUrl {
        &self.callback_url
    }
    pub fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
    pub fn services(&self) -> &[ServiceInfo] {
        &self.services
    }
}

/// Plain-data form for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppProviderSnapshot {
    pub key: String,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub callback_url: String,
    pub timeout_ms: u32,
    pub services: Vec<ServiceInfo>,
}

impl AppProvider {
    pub fn snapshot(&self) -> AppProviderSnapshot {
        AppProviderSnapshot {
            key: self.key.as_str().to_string(),
            version: self.version,
            stamps: self.stamps,
            callback_url: self.callback_url.as_str().to_string(),
            timeout_ms: self.timeout_ms,
            services: self.services.clone(),
        }
    }

    pub fn restore(s: AppProviderSnapshot) -> Result<Self, ShippingError> {
        let mut provider = Self::new(NewAppProvider {
            key: ProviderKey::new(&s.key)?,
            callback_url: s.callback_url,
            timeout_ms: s.timeout_ms,
            services: s.services,
        })?;
        provider.version = s.version;
        provider.stamps = s.stamps;
        Ok(provider)
    }
}
