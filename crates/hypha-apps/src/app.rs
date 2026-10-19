use hypha_core::{
    Actor, AppId, AppKey, CallbackUrl, Event, EventRoute, PendingEvent, Recorded, Recorder, Scope,
    Sealed, Stamped, Stamps, Subject, Timestamp, Version, Versioned, required_text,
};

use crate::endpoints::ExtensionEndpoint;
use crate::events::{APP, AppGrantChanged, AppInstalled, AppUninstalled};
use crate::manifest::{AppManifest, ManifestSource};

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

/// The scopes staff approved for an app installed through OAuth (design note 0029,
/// AP5): every access token the app holds acts with these at most, read fresh on each
/// request, so narrowing a grant takes effect at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// In `Scope::ALL` order, each once.
    scopes: Vec<Scope>,
    /// The staff member who approved (or last narrowed) it.
    approved_by: Actor,
    approved_at: Timestamp,
}

impl Grant {
    pub fn new(scopes: &[Scope], approved_by: Actor, approved_at: Timestamp) -> Self {
        Self {
            scopes: normalize(scopes),
            approved_by,
            approved_at,
        }
    }

    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }
    pub fn approved_by(&self) -> Actor {
        self.approved_by
    }
    pub fn approved_at(&self) -> Timestamp {
        self.approved_at
    }
    pub fn has(&self, scope: Scope) -> bool {
        self.scopes.contains(&scope)
            || scope
                .implied_by()
                .is_some_and(|wider| self.scopes.contains(&wider))
    }
}

/// `scopes` in `Scope::ALL` order, each once.
pub fn normalize(scopes: &[Scope]) -> Vec<Scope> {
    Scope::ALL
        .into_iter()
        .filter(|s| scopes.contains(s))
        .collect()
}

/// The manifest an app was installed (or last refreshed) from, and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledManifest {
    /// The manifest's URL: the app's OAuth client ID.
    pub url: String,
    pub manifest: AppManifest,
    pub fetched_at: Timestamp,
}

/// The read scope an event's subject kind needs, for apps with a grant: an app is only
/// sent events about what it may read (design note 0029, §4.1 step 5). `None` for a
/// kind no scope covers, which granted apps aren't sent.
pub fn event_scope(subject_kind: &str) -> Option<Scope> {
    Some(match subject_kind {
        "product" | "category" | "search_settings" => Scope::ProductsRead,
        "channel" => Scope::ChannelsRead,
        "catalog" | "market" => Scope::MarketsRead,
        "price_list" => Scope::PricingRead,
        "tax_class" | "tax_region" => Scope::TaxesRead,
        "inventory_item" | "location" => Scope::InventoryRead,
        "customer" => Scope::CustomersRead,
        "order" | "payment" => Scope::OrdersRead,
        "fulfillment" => Scope::FulfillmentsRead,
        "shipping_zone" => Scope::ShippingRead,
        _ => return None,
    })
}

/// An external app (design note 0016, EV11). Made by hand (`create_app`, webhooks
/// only) or installed through OAuth from its manifest (design note 0029), which gives
/// it a grant.
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    key: AppKey,
    /// What it acts as (`Actor::App`): `Copy`, unlike the key.
    id: AppId,
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
    /// The manifest it was installed from; `None` for an app made by hand.
    manifest: Option<InstalledManifest>,
    /// What staff approved; `None` until the first approval, and after an uninstall.
    grant: Option<Grant>,
    /// When it was uninstalled: it then only gets `app.uninstalled`, its last webhook,
    /// until it's deleted or installed again.
    uninstalled_at: Option<Timestamp>,
    events: Recorder,
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

impl Recorded for App {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

/// A new app, checked by `App::new`.
pub struct NewApp {
    pub key: AppKey,
    pub id: AppId,
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
            id: new.id,
            version: Version::NEW,
            stamps: None,
            name: required_text("name", &new.name)?,
            enabled: true,
            secret: new.secret,
            webhook: new.webhook,
            subscriptions: Vec::new(),
            manifest: None,
            grant: None,
            uninstalled_at: None,
            events: Recorder::default(),
        };
        app.set_subscriptions(new.subscriptions)?;
        Ok(app)
    }

    /// An app installed from its manifest with the scopes staff approved: its name,
    /// webhook and subscriptions come from the manifest. Records `app.installed`.
    pub fn install(
        id: AppId,
        secret: Sealed,
        manifest: InstalledManifest,
        grant: Grant,
    ) -> Result<Self, AppError> {
        let mut app = App::new(NewApp {
            key: manifest.manifest.key().clone(),
            id,
            name: manifest.manifest.name().to_string(),
            secret,
            webhook: None,
            subscriptions: Vec::new(),
        })?;
        app.apply_manifest(manifest)?;
        app.events.record(
            app.subject(),
            &AppInstalled {
                app: app.key.to_string(),
                scopes: grant.scopes().iter().map(|s| s.as_str()).collect(),
            },
        );
        app.grant = Some(grant);
        Ok(app)
    }

    /// Installs an app again after it was uninstalled (the same key): a new secret,
    /// the manifest and the grant, enabled. Records `app.installed`.
    pub fn reinstall(
        &mut self,
        secret: Sealed,
        manifest: InstalledManifest,
        grant: Grant,
    ) -> Result<(), AppError> {
        self.apply_manifest(manifest)?;
        self.secret = secret;
        self.enabled = true;
        self.uninstalled_at = None;
        self.events.record(
            self.subject(),
            &AppInstalled {
                app: self.key.to_string(),
                scopes: grant.scopes().iter().map(|s| s.as_str()).collect(),
            },
        );
        self.grant = Some(grant);
        Ok(())
    }

    /// Takes the manifest's name, webhook and events (the registrations), keeping the
    /// webhook's timeout. The manifest must be this app's.
    pub fn apply_manifest(&mut self, manifest: InstalledManifest) -> Result<(), AppError> {
        if manifest.manifest.key() != &self.key {
            return Err(AppError::ManifestForOtherApp(
                manifest.manifest.key().clone(),
            ));
        }
        self.name = required_text("name", manifest.manifest.name())?;
        let timeout = self.webhook.as_ref().map(|w| w.timeout_ms);
        match manifest.manifest.webhook() {
            Some((url, events)) => {
                self.webhook = Some(Webhook::new(url.as_str(), timeout)?);
                self.subscriptions = events.to_vec();
            }
            None => {
                self.webhook = None;
                self.subscriptions = Vec::new();
            }
        }
        self.manifest = Some(manifest);
        Ok(())
    }

    /// Replaces the grant with `grant`, recording `app.grant_changed` when its scopes
    /// differ from the current grant's.
    pub fn set_grant(&mut self, grant: Grant) {
        let before = self
            .grant
            .as_ref()
            .map(|g| g.scopes().to_vec())
            .unwrap_or_default();
        if before != grant.scopes() {
            self.record_grant_change(&before, grant.scopes());
        }
        self.grant = Some(grant);
    }

    /// Narrows the grant to `scopes` (each one it has), at once: tokens act with the
    /// grant as it is on each request. Staff do it from the admin (§4.1).
    pub fn narrow_grant(
        &mut self,
        scopes: &[Scope],
        by: Actor,
        at: Timestamp,
    ) -> Result<(), AppError> {
        let current = self.grant.as_ref().ok_or(AppError::NotInstalled)?;
        if let Some(extra) = scopes.iter().find(|s| !current.scopes().contains(s)) {
            return Err(AppError::GrantNotNarrower(*extra));
        }
        self.set_grant(Grant::new(scopes, by, at));
        Ok(())
    }

    fn record_grant_change(&mut self, before: &[Scope], after: &[Scope]) {
        let names = |list: &[Scope], not_in: &[Scope]| -> Vec<&'static str> {
            list.iter()
                .filter(|s| !not_in.contains(s))
                .map(|s| s.as_str())
                .collect()
        };
        self.events.record(
            self.subject(),
            &AppGrantChanged {
                app: self.key.to_string(),
                scopes: after.iter().map(|s| s.as_str()).collect(),
                added: names(after, before),
                removed: names(before, after),
            },
        );
    }

    /// Uninstalls it (design note 0029, §4.1): the grant goes, it takes no more events
    /// but `app.uninstalled`, which it's sent last. Its webhook and secret stay for that
    /// delivery; deleting it afterwards removes it for good.
    pub fn uninstall(&mut self, at: Timestamp) -> Result<(), AppError> {
        if self.grant.is_none() || self.uninstalled_at.is_some() {
            return Err(AppError::NotInstalled);
        }
        self.grant = None;
        self.uninstalled_at = Some(at);
        self.subscriptions = Vec::new();
        self.events.record(
            self.subject(),
            &AppUninstalled {
                app: self.key.to_string(),
            },
        );
        Ok(())
    }

    fn subject(&self) -> Subject {
        Subject::new(APP, self.id.as_uuid())
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
    /// subscribed, and not uninstalled. Says nothing about the subject: `wants_event`
    /// is what fan-out asks.
    pub fn wants(&self, event_type: &str) -> bool {
        self.enabled
            && self.uninstalled_at.is_none()
            && self.webhook.is_some()
            && self.subscriptions.iter().any(|p| p.matches(event_type))
    }

    /// Whether fan-out sends this app `route`'s event. An app's own lifecycle events
    /// (`app.*`) go to that app only, unsubscribed; others as `wants` says, and for an
    /// app with a grant only about what the grant reads (`event_scope`).
    pub fn wants_event(&self, route: &EventRoute<'_>) -> bool {
        if route.subject_kind == APP {
            return route.subject_id == self.id.as_uuid()
                && self.webhook.is_some()
                && (route.event_type == AppUninstalled::TYPE
                    || (self.enabled && self.uninstalled_at.is_none()));
        }
        if !self.wants(route.event_type) {
            return false;
        }
        match (&self.grant, &self.manifest) {
            (Some(grant), _) => event_scope(route.subject_kind).is_some_and(|s| grant.has(s)),
            // Installed from a manifest but not granted: nothing.
            (None, Some(_)) => false,
            // Made by hand (webhooks only, design note 0016): as subscribed.
            (None, None) => true,
        }
    }

    pub fn key(&self) -> &AppKey {
        &self.key
    }
    pub fn id(&self) -> AppId {
        self.id
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
    pub fn manifest(&self) -> Option<&InstalledManifest> {
        self.manifest.as_ref()
    }
    pub fn grant(&self) -> Option<&Grant> {
        self.grant.as_ref()
    }
    pub fn uninstalled_at(&self) -> Option<Timestamp> {
        self.uninstalled_at
    }
    /// Installed through OAuth and in force: a grant, not uninstalled.
    pub fn is_installed(&self) -> bool {
        self.grant.is_some() && self.uninstalled_at.is_none()
    }

    /// Whether Hypha calls it at an extension point that needs `scope` (design note
    /// 0029, §4.1 step 5): enabled, and either made by hand (staff registered its
    /// endpoints, as they did its webhook) or installed with a grant that has `scope`.
    /// Uninstalled, or installed from a manifest but not granted: never.
    pub fn may_answer(&self, scope: Scope) -> bool {
        if !self.enabled {
            return false;
        }
        match (&self.grant, &self.manifest) {
            (None, None) => true,
            (Some(grant), _) => self.uninstalled_at.is_none() && grant.has(scope),
            (None, Some(_)) => false,
        }
    }

    /// The extension points its manifest registers (URLs resolved against its
    /// `app_url`); `None` for an app without a manifest, whose endpoints staff set.
    pub fn manifest_endpoints(&self) -> Option<Vec<ExtensionEndpoint>> {
        let manifest = self.manifest.as_ref()?;
        Some(
            manifest
                .manifest
                .document()
                .extension_points
                .iter()
                .filter_map(|p| {
                    Some(ExtensionEndpoint {
                        point: p.point.clone(),
                        version: p.version,
                        url: CallbackUrl::new(&p.url).ok()?,
                    })
                })
                .collect(),
        )
    }
}

/// A grant as plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantSnapshot {
    pub scopes: Vec<Scope>,
    pub approved_by: Actor,
    pub approved_at: Timestamp,
}

/// Plain-data form for storage adapters.
#[derive(Debug, Clone, PartialEq)]
pub struct AppSnapshot {
    pub key: String,
    pub id: AppId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
    pub enabled: bool,
    pub secret: Sealed,
    pub webhook_url: Option<String>,
    pub webhook_timeout_ms: Option<u32>,
    pub subscriptions: Vec<String>,
    pub manifest_url: Option<String>,
    /// The checked manifest as JSON (`AppManifest::to_value`).
    pub manifest: Option<serde_json::Value>,
    pub manifest_fetched_at: Option<Timestamp>,
    /// From the manifest, for adapters that keep them in their own columns.
    pub publisher: Option<String>,
    pub privacy_url: Option<String>,
    pub grant: Option<GrantSnapshot>,
    pub uninstalled_at: Option<Timestamp>,
}

impl App {
    pub fn snapshot(&self) -> AppSnapshot {
        let manifest = self.manifest.as_ref();
        AppSnapshot {
            key: self.key.as_str().to_string(),
            id: self.id,
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
            manifest_url: manifest.map(|m| m.url.clone()),
            manifest: manifest.map(|m| m.manifest.to_value()),
            manifest_fetched_at: manifest.map(|m| m.fetched_at),
            publisher: manifest.and_then(|m| m.manifest.publisher().map(str::to_string)),
            privacy_url: manifest.map(|m| m.manifest.privacy_url().to_string()),
            grant: self.grant.as_ref().map(|g| GrantSnapshot {
                scopes: g.scopes.clone(),
                approved_by: g.approved_by,
                approved_at: g.approved_at,
            }),
            uninstalled_at: self.uninstalled_at,
        }
    }

    /// Rebuilds an app from storage with every rule `new` checks, its manifest's
    /// included.
    pub fn restore(s: AppSnapshot) -> Result<Self, AppError> {
        let webhook = s
            .webhook_url
            .as_deref()
            .map(|url| Webhook::new(url, s.webhook_timeout_ms))
            .transpose()?;
        let mut app = App::new(NewApp {
            key: AppKey::new(&s.key)?,
            id: s.id,
            name: s.name,
            secret: s.secret,
            webhook,
            subscriptions: s.subscriptions,
        })?;
        app.manifest = match (s.manifest_url, s.manifest, s.manifest_fetched_at) {
            (Some(url), Some(value), Some(fetched_at)) => Some(InstalledManifest {
                manifest: AppManifest::from_value_of(value, ManifestSource::of_client(&url))
                    .map_err(AppError::Manifest)?,
                url,
                fetched_at,
            }),
            (None, None, None) => None,
            _ => return Err(AppError::IncompleteManifest),
        };
        app.grant = s
            .grant
            .map(|g| Grant::new(&g.scopes, g.approved_by, g.approved_at));
        app.uninstalled_at = s.uninstalled_at;
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
