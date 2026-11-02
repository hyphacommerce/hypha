//! The store's checkout settings: which apps checkout and address forms ask about an
//! address. One per store; until the merchant saves, nothing is asked.

use hypha_core::{
    AppKey, Event, PendingEvent, Recorded, Recorder, Stamped, Stamps, Subject, Version, Versioned,
};
use serde::Serialize;
use uuid::Uuid;

/// The extension point a verification app answers; the same name as
/// `hypha_extensions::AddressVerify`'s (a host test keeps them equal: this leaf doesn't
/// depend on the catalog).
pub const VERIFY_POINT: &str = "address.verify";

/// The points an autocomplete app answers, both of them: suggestions while typing, then
/// the picked one's parts (`hypha_extensions::AddressSuggest`, `AddressResolve`).
pub const AUTOCOMPLETE_POINTS: [&str; 2] = ["address.suggest", "address.resolve"];

/// The subject kind of its events; there's one per store, so the subject ID is nil.
pub const CHECKOUT_SETTINGS: &str = "checkout_settings";

/// `checkout_settings.updated`: `fields` names what changed (`verification_app`,
/// `autocomplete_app`).
#[derive(Debug, Serialize)]
pub struct CheckoutSettingsUpdated {
    pub fields: Vec<&'static str>,
}

impl Event for CheckoutSettingsUpdated {
    const TYPE: &'static str = "checkout_settings.updated";
    const VERSION: u16 = 1;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutSettings {
    version: Version,
    stamps: Option<Stamps>,
    /// The app verifying shipping addresses (`address.verify`); `None`: none are.
    verification_app: Option<AppKey>,
    /// The app suggesting addresses while someone types one (`address.suggest` and
    /// `address.resolve`); `None`: forms don't suggest.
    autocomplete_app: Option<AppKey>,
    events: Recorder,
}

impl Default for CheckoutSettings {
    fn default() -> Self {
        Self {
            version: Version::NEW,
            stamps: None,
            verification_app: None,
            autocomplete_app: None,
            events: Recorder::default(),
        }
    }
}

impl CheckoutSettings {
    /// Names the app that verifies addresses (`None`: none). The caller checked that it
    /// answers `address.verify`.
    pub fn set_verification_app(&mut self, app: Option<AppKey>) {
        if app != self.verification_app {
            self.verification_app = app;
            self.changed("verification_app");
        }
    }

    pub fn verification_app(&self) -> Option<&AppKey> {
        self.verification_app.as_ref()
    }

    /// Names the app that suggests addresses (`None`: none). The caller checked that it
    /// answers both `AUTOCOMPLETE_POINTS`.
    pub fn set_autocomplete_app(&mut self, app: Option<AppKey>) {
        if app != self.autocomplete_app {
            self.autocomplete_app = app;
            self.changed("autocomplete_app");
        }
    }

    pub fn autocomplete_app(&self) -> Option<&AppKey> {
        self.autocomplete_app.as_ref()
    }

    fn changed(&mut self, field: &'static str) {
        // One event per write, naming every field: coalesce with a pending one.
        let mut fields: Vec<&'static str> = self
            .events
            .take()
            .iter()
            .filter_map(|e| e.data["fields"].as_array().cloned())
            .flatten()
            .filter_map(|f| Self::FIELDS.iter().copied().find(|k| f.as_str() == Some(k)))
            .collect();
        if !fields.contains(&field) {
            fields.push(field);
        }
        fields.sort_unstable();
        self.events.record(
            Subject::new(CHECKOUT_SETTINGS, Uuid::nil()),
            &CheckoutSettingsUpdated { fields },
        );
    }

    /// Every field an event may name.
    const FIELDS: [&'static str; 2] = ["autocomplete_app", "verification_app"];

    pub fn snapshot(&self) -> CheckoutSettingsSnapshot {
        CheckoutSettingsSnapshot {
            version: self.version,
            stamps: self.stamps,
            verification_app: self.verification_app.clone(),
            autocomplete_app: self.autocomplete_app.clone(),
        }
    }

    pub fn restore(s: CheckoutSettingsSnapshot) -> Self {
        Self {
            version: s.version,
            stamps: s.stamps,
            verification_app: s.verification_app,
            autocomplete_app: s.autocomplete_app,
            events: Recorder::default(),
        }
    }
}

/// Plain-data form for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutSettingsSnapshot {
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub verification_app: Option<AppKey>,
    pub autocomplete_app: Option<AppKey>,
}

impl Recorded for CheckoutSettings {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for CheckoutSettings {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for CheckoutSettings {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> AppKey {
        AppKey::new("app_verify").unwrap()
    }

    #[test]
    fn naming_an_app_records_the_field() {
        let mut s = CheckoutSettings::default();
        s.set_verification_app(Some(app()));
        let events = s.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "checkout_settings.updated");
        assert_eq!(
            events[0].data["fields"],
            serde_json::json!(["verification_app"])
        );
    }

    #[test]
    fn naming_the_same_app_records_nothing() {
        let mut s = CheckoutSettings::default();
        s.set_verification_app(Some(app()));
        s.take_events();
        s.set_verification_app(Some(app()));
        assert!(s.take_events().is_empty());
    }

    #[test]
    fn one_event_names_every_field_changed() {
        let mut s = CheckoutSettings::default();
        s.set_verification_app(Some(app()));
        s.set_autocomplete_app(Some(app()));
        let events = s.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].data["fields"],
            serde_json::json!(["autocomplete_app", "verification_app"])
        );
    }

    #[test]
    fn two_changes_make_one_event() {
        let mut s = CheckoutSettings::default();
        s.set_verification_app(Some(app()));
        s.set_verification_app(None);
        assert_eq!(s.take_events().len(), 1);
    }
}
