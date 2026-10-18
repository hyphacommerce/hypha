use hypha_core::{AppKey, CallbackUrl, Sealed, Stamped, Stamps, Version, Versioned, required_text};

use crate::AppError;

/// Which events an app wants: an exact type (`order.paid`), every type of a kind
/// (`product.*`), or everything (`*`). Types aren't checked against a catalog here
/// (apps is a leaf domain); an unknown one simply never matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventPattern(String);

impl EventPattern {
    pub fn new(pattern: &str) -> Result<Self, AppError> {
        let invalid = || AppError::InvalidSubscription(pattern.to_string());
        if pattern == "*" {
            return Ok(Self(pattern.to_string()));
        }
        let mut parts = pattern.split('.');
        let kind = parts.next().ok_or_else(invalid)?;
        let rest: Vec<&str> = parts.collect();
        let word = |w: &str| {
            !w.is_empty() && w.len() <= 64 && w.chars().all(|c| c.is_ascii_lowercase() || c == '_')
        };
        let valid = word(kind) && rest.len() == 1 && (rest[0] == "*" || word(rest[0]));
        if !valid {
            return Err(invalid());
        }
        Ok(Self(pattern.to_string()))
    }

    pub fn matches(&self, event_type: &str) -> bool {
        match self.0.strip_suffix('*') {
            Some("") => true,
            Some(prefix) => event_type.starts_with(prefix),
            None => self.0 == event_type,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where an app's webhooks go, and how long Hypha waits for each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Webhook {
    pub url: CallbackUrl,
    pub timeout_ms: u32,
}

impl Webhook {
    pub const DEFAULT_TIMEOUT_MS: u32 = 10_000;
    pub const MIN_TIMEOUT_MS: u32 = 1_000;
    pub const MAX_TIMEOUT_MS: u32 = 30_000;

    pub fn new(url: &str, timeout_ms: Option<u32>) -> Result<Self, AppError> {
        let timeout_ms = timeout_ms.unwrap_or(Self::DEFAULT_TIMEOUT_MS);
        if !(Self::MIN_TIMEOUT_MS..=Self::MAX_TIMEOUT_MS).contains(&timeout_ms) {
            return Err(AppError::InvalidTimeout {
                min: Self::MIN_TIMEOUT_MS,
                max: Self::MAX_TIMEOUT_MS,
            });
        }
        Ok(Self {
            url: CallbackUrl::new(url)?,
            timeout_ms,
        })
    }
}

/// An external app (design note 0016, EV11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    key: AppKey,
    version: Version,
    /// When it was stored and last changed, set by the adapter (design note 0016).
    stamps: Option<Stamps>,
    name: String,
    enabled: bool,
    /// Signs webhooks and rate callbacks; sealed under the host's key with the app key
    /// as context.
    secret: Sealed,
    webhook: Option<Webhook>,
    subscriptions: Vec<EventPattern>,
}

impl Versioned for App {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for App {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

/// A new app, checked by `App::new`.
pub struct NewApp {
    pub key: AppKey,
    pub name: String,
    pub secret: Sealed,
    pub webhook: Option<Webhook>,
    pub subscriptions: Vec<String>,
}

impl App {
    /// Plenty for "everything about orders and products", and keeps matching cheap.
    pub const MAX_SUBSCRIPTIONS: usize = 50;

    pub fn new(new: NewApp) -> Result<Self, AppError> {
        let mut app = Self {
            key: new.key,
            version: Version::NEW,
            stamps: None,
            name: required_text("name", &new.name)?,
            enabled: true,
            secret: new.secret,
            webhook: new.webhook,
            subscriptions: Vec::new(),
        };
        app.set_subscriptions(new.subscriptions)?;
        Ok(app)
    }

    pub fn rename(&mut self, name: &str) -> Result<(), AppError> {
        self.name = required_text("name", name)?;
        Ok(())
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn set_secret(&mut self, secret: Sealed) {
        self.secret = secret;
    }

    pub fn set_webhook(&mut self, webhook: Option<Webhook>) {
        self.webhook = webhook;
    }

    /// Each pattern once, in the order given.
    pub fn set_subscriptions(&mut self, patterns: Vec<String>) -> Result<(), AppError> {
        if patterns.len() > Self::MAX_SUBSCRIPTIONS {
            return Err(AppError::TooManySubscriptions {
                max: Self::MAX_SUBSCRIPTIONS,
            });
        }
        let mut checked: Vec<EventPattern> = Vec::with_capacity(patterns.len());
        for pattern in &patterns {
            let pattern = EventPattern::new(pattern.trim())?;
            if !checked.contains(&pattern) {
                checked.push(pattern);
            }
        }
        self.subscriptions = checked;
        Ok(())
    }

    /// Whether this app's webhook gets events of `event_type`: enabled, with a webhook,
    /// and subscribed.
    pub fn wants(&self, event_type: &str) -> bool {
        self.enabled
            && self.webhook.is_some()
            && self.subscriptions.iter().any(|p| p.matches(event_type))
    }

    pub fn key(&self) -> &AppKey {
        &self.key
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn secret(&self) -> &Sealed {
        &self.secret
    }
    pub fn webhook(&self) -> Option<&Webhook> {
        self.webhook.as_ref()
    }
    pub fn subscriptions(&self) -> &[EventPattern] {
        &self.subscriptions
    }
}

/// Plain-data form for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSnapshot {
    pub key: String,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
    pub enabled: bool,
    pub secret: Sealed,
    pub webhook_url: Option<String>,
    pub webhook_timeout_ms: Option<u32>,
    pub subscriptions: Vec<String>,
}

impl App {
    pub fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            key: self.key.as_str().to_string(),
            version: self.version,
            stamps: self.stamps,
            name: self.name.clone(),
            enabled: self.enabled,
            secret: self.secret.clone(),
            webhook_url: self.webhook.as_ref().map(|w| w.url.as_str().to_string()),
            webhook_timeout_ms: self.webhook.as_ref().map(|w| w.timeout_ms),
            subscriptions: self
                .subscriptions
                .iter()
                .map(|p| p.as_str().to_string())
                .collect(),
        }
    }

    /// Rebuilds an app from storage with every rule `new` checks.
    pub fn restore(s: AppSnapshot) -> Result<Self, AppError> {
        let webhook = s
            .webhook_url
            .as_deref()
            .map(|url| Webhook::new(url, s.webhook_timeout_ms))
            .transpose()?;
        let mut app = App::new(NewApp {
            key: AppKey::new(&s.key)?,
            name: s.name,
            secret: s.secret,
            webhook,
            subscriptions: s.subscriptions,
        })?;
        app.version = s.version;
        app.stamps = s.stamps;
        app.enabled = s.enabled;
        Ok(app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_match_a_type_a_kind_or_everything() {
        let p = |s: &str| EventPattern::new(s).unwrap();
        assert!(p("order.paid").matches("order.paid"));
        assert!(!p("order.paid").matches("order.placed"));
        assert!(p("product.*").matches("product.variant_removed"));
        assert!(!p("product.*").matches("price_list.changed"));
        assert!(p("*").matches("anything.at_all"));
    }

    #[test]
    fn malformed_patterns_are_refused() {
        for bad in [
            "",
            "order",
            "order.",
            "order.paid.x",
            "Order.paid",
            "*.paid",
            "order.p-aid",
        ] {
            assert_eq!(
                EventPattern::new(bad),
                Err(AppError::InvalidSubscription(bad.to_string())),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn webhook_timeouts_have_bounds() {
        let url = "https://erp.example/hooks";
        assert_eq!(Webhook::new(url, None).unwrap().timeout_ms, 10_000);
        assert!(Webhook::new(url, Some(1_000)).is_ok());
        assert_eq!(
            Webhook::new(url, Some(999)),
            Err(AppError::InvalidTimeout {
                min: 1_000,
                max: 30_000
            })
        );
    }
}
