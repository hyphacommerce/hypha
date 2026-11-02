//! The store's checkout settings: which apps checkout and address forms ask about an
//! address. One per store; until the merchant saves, nothing is asked.

use hypha_core::{
    AppKey, Event, PendingEvent, Recorded, Recorder, Stamped, Stamps, Subject, Version, Versioned,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
use uuid::Uuid;

/// The extension point a verification app answers; the same name as
/// `hypha_extensions::AddressVerify`'s (a host test keeps them equal: this leaf doesn't
/// depend on the catalog).
pub const VERIFY_POINT: &str = "address.verify";

/// The points an autocomplete app answers, both of them: suggestions while typing, then
/// the picked one's parts (`hypha_extensions::AddressSuggest`, `AddressResolve`).
pub const AUTOCOMPLETE_POINTS: [&str; 2] = ["address.suggest", "address.resolve"];

/// The point a validation app answers (`hypha_extensions::CheckoutValidate`'s name).
pub const VALIDATE_POINT: &str = "checkout.validate";

/// At most this many apps validate a store's checkouts: every one is asked at once on each
/// step, and checkout waits for the slowest.
pub const MAX_VALIDATION_APPS: usize = 5;

/// How checkout treats one validation app. An app not listed in the settings is asked, and
/// its failure doesn't block (`ValidationAppSetting::default_for`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationAppSetting {
    #[serde(serialize_with = "key_str", deserialize_with = "key_parse")]
    pub app: AppKey,
    /// Whether it's asked at all.
    pub enabled: bool,
    /// Whether checkout stops (`validation_unavailable`) when it can't be asked, rather
    /// than going on without it.
    pub block_on_failure: bool,
}

impl ValidationAppSetting {
    /// What an app installed for the point gets until the merchant says otherwise.
    pub fn default_for(app: AppKey) -> Self {
        Self {
            app,
            enabled: true,
            block_on_failure: false,
        }
    }
}

fn key_str<S: Serializer>(key: &AppKey, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(key.as_str())
}

fn key_parse<'de, D: Deserializer<'de>>(d: D) -> Result<AppKey, D::Error> {
    let raw = String::deserialize(d)?;
    AppKey::new(&raw).map_err(D::Error::custom)
}

/// The subject kind of its events; there's one per store, so the subject ID is nil.
pub const CHECKOUT_SETTINGS: &str = "checkout_settings";

/// `checkout_settings.updated`: `fields` names what changed (`verification_app`,
/// `autocomplete_app`, `validation_apps`).
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
    /// The validation apps the merchant set something for, one entry each.
    validation_apps: Vec<ValidationAppSetting>,
    events: Recorder,
}

impl Default for CheckoutSettings {
    fn default() -> Self {
        Self {
            version: Version::NEW,
            stamps: None,
            verification_app: None,
            autocomplete_app: None,
            validation_apps: Vec::new(),
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

    /// Sets how checkout treats each validation app named, replacing what was set. The
    /// caller checked each app answers `checkout.validate` and each is named once.
    pub fn set_validation_apps(&mut self, apps: Vec<ValidationAppSetting>) {
        if apps != self.validation_apps {
            self.validation_apps = apps;
            self.changed("validation_apps");
        }
    }

    /// What the merchant set, per app.
    pub fn validation_apps(&self) -> &[ValidationAppSetting] {
        &self.validation_apps
    }

    /// How checkout treats `app`: as set, else the default (asked, not blocking).
    pub fn validation_app(&self, app: &AppKey) -> ValidationAppSetting {
        self.validation_apps
            .iter()
            .find(|s| &s.app == app)
            .cloned()
            .unwrap_or_else(|| ValidationAppSetting::default_for(app.clone()))
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
    const FIELDS: [&'static str; 3] = ["autocomplete_app", "validation_apps", "verification_app"];

    pub fn snapshot(&self) -> CheckoutSettingsSnapshot {
        CheckoutSettingsSnapshot {
            version: self.version,
            stamps: self.stamps,
            verification_app: self.verification_app.clone(),
            autocomplete_app: self.autocomplete_app.clone(),
            validation_apps: self.validation_apps.clone(),
        }
    }

    pub fn restore(s: CheckoutSettingsSnapshot) -> Self {
        Self {
            version: s.version,
            stamps: s.stamps,
            verification_app: s.verification_app,
            autocomplete_app: s.autocomplete_app,
            validation_apps: s.validation_apps,
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
    pub validation_apps: Vec<ValidationAppSetting>,
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

    fn blocking(app: AppKey) -> ValidationAppSetting {
        ValidationAppSetting {
            block_on_failure: true,
            ..ValidationAppSetting::default_for(app)
        }
    }

    #[test]
    fn an_app_not_listed_is_asked_and_doesnt_block() {
        let s = CheckoutSettings::default();
        assert_eq!(
            s.validation_app(&app()),
            ValidationAppSetting {
                app: app(),
                enabled: true,
                block_on_failure: false
            }
        );
    }

    #[test]
    fn a_listed_app_is_treated_as_set() {
        let mut s = CheckoutSettings::default();
        s.set_validation_apps(vec![blocking(app())]);
        assert!(s.validation_app(&app()).block_on_failure);
    }

    #[test]
    fn setting_validation_apps_records_the_field() {
        let mut s = CheckoutSettings::default();
        s.set_validation_apps(vec![blocking(app())]);
        assert_eq!(
            s.take_events()[0].data["fields"],
            serde_json::json!(["validation_apps"])
        );
    }

    #[test]
    fn setting_the_same_validation_apps_records_nothing() {
        let mut s = CheckoutSettings::default();
        s.set_validation_apps(vec![blocking(app())]);
        s.take_events();
        s.set_validation_apps(vec![blocking(app())]);
        assert!(s.take_events().is_empty());
    }

    #[test]
    fn a_setting_reads_back_from_its_json() {
        let setting = blocking(app());
        let json = serde_json::to_string(&setting).unwrap();
        assert_eq!(
            json,
            r#"{"app":"app_verify","enabled":true,"block_on_failure":true}"#
        );
        assert_eq!(
            serde_json::from_str::<ValidationAppSetting>(&json).unwrap(),
            setting
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
