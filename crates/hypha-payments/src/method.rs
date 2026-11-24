use std::collections::BTreeSet;

use hypha_core::{
    ChannelId, MarketId, PaymentMethodId, PendingEvent, Recorded, Recorder, Stamped, Stamps,
    Subject, Version, Versioned, required_text,
};

use crate::{
    PaymentError, ProviderKey,
    events::{PAYMENT_METHOD, PaymentMethodCreated, PaymentMethodDeleted, PaymentMethodUpdated},
};

/// How shoppers can pay: a provider (`manual` for cash on delivery or a bank transfer), the
/// name shoppers see, the instructions shown after checkout, and where it's offered.
///
/// Invariants, checked by `new` and `restore`: a name (trimmed, at most 255
/// characters), instructions of at most `MAX_INSTRUCTIONS_LEN` characters, markets and
/// channels without repeats (kept sorted). Whether the provider exists in this build and
/// accepts the settings is the commands' to check, with the host's providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentMethod {
    id: PaymentMethodId,
    version: Version,
    stamps: Option<Stamps>,
    provider: ProviderKey,
    name: String,
    instructions: Option<String>,
    /// Empty: every market.
    markets: Vec<MarketId>,
    /// Empty: every sales channel.
    channels: Vec<ChannelId>,
    enabled: bool,
    /// Shoppers see methods by position, then by ID.
    position: u32,
    /// The provider's own settings (none for `manual`).
    settings: serde_json::Value,
    events: Recorder,
    /// The fields changed since it was loaded: one `payment_method.updated` names them
    /// all when its events are taken.
    changed: BTreeSet<&'static str>,
}

impl Recorded for PaymentMethod {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let changed = std::mem::take(&mut self.changed);
        // A new method's `created` says it all.
        if !changed.is_empty() && self.version != Version::NEW {
            let event = PaymentMethodUpdated {
                payment_method: self.id,
                fields: changed.into_iter().collect(),
            };
            self.events.record(self.subject(), &event);
        }
        self.events.take()
    }
}

impl Versioned for PaymentMethod {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for PaymentMethod {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

/// What a new method is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPaymentMethod {
    pub id: PaymentMethodId,
    pub provider: ProviderKey,
    pub name: String,
    pub instructions: Option<String>,
    pub markets: Vec<MarketId>,
    pub channels: Vec<ChannelId>,
    pub enabled: bool,
    pub position: u32,
    pub settings: serde_json::Value,
}

impl PaymentMethod {
    /// Bank details fit; a novel doesn't.
    pub const MAX_INSTRUCTIONS_LEN: usize = 2000;

    pub fn new(new: NewPaymentMethod) -> Result<Self, PaymentError> {
        let mut method = Self::restore(PaymentMethodSnapshot {
            id: new.id,
            version: Version::NEW,
            stamps: None,
            provider: new.provider,
            name: new.name,
            instructions: new.instructions,
            markets: new.markets,
            channels: new.channels,
            enabled: new.enabled,
            position: new.position,
            settings: new.settings,
        })?;
        let event = PaymentMethodCreated {
            payment_method: method.id,
        };
        method.events.record(method.subject(), &event);
        Ok(method)
    }

    fn subject(&self) -> Subject {
        Subject::new(PAYMENT_METHOD, self.id.as_uuid())
    }

    /// Offered to a cart in `market`, sold through `channel`: enabled, and both lists
    /// either empty or naming them.
    pub fn offered_in(&self, market: MarketId, channel: ChannelId) -> bool {
        self.enabled
            && (self.markets.is_empty() || self.markets.contains(&market))
            && (self.channels.is_empty() || self.channels.contains(&channel))
    }

    pub fn rename(&mut self, name: &str) -> Result<(), PaymentError> {
        self.name = required_text("name", name)?;
        self.changed.insert("name");
        Ok(())
    }

    /// Blank clears them.
    pub fn set_instructions(&mut self, instructions: Option<String>) -> Result<(), PaymentError> {
        self.instructions = instructions_text(instructions)?;
        self.changed.insert("instructions");
        Ok(())
    }

    pub fn set_markets(&mut self, markets: Vec<MarketId>) {
        self.markets = sorted(markets);
        self.changed.insert("markets");
    }

    pub fn set_channels(&mut self, channels: Vec<ChannelId>) {
        self.channels = sorted(channels);
        self.changed.insert("channels");
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.changed.insert("enabled");
    }

    pub fn set_position(&mut self, position: u32) {
        self.position = position;
        self.changed.insert("position");
    }

    /// The caller checked them with the provider.
    pub fn set_settings(&mut self, settings: serde_json::Value) {
        self.settings = settings;
        self.changed.insert("settings");
    }

    pub fn id(&self) -> PaymentMethodId {
        self.id
    }

    pub fn provider(&self) -> &ProviderKey {
        &self.provider
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn instructions(&self) -> Option<&str> {
        self.instructions.as_deref()
    }

    pub fn markets(&self) -> &[MarketId] {
        &self.markets
    }

    pub fn channels(&self) -> &[ChannelId] {
        &self.channels
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn position(&self) -> u32 {
        self.position
    }

    pub fn settings(&self) -> &serde_json::Value {
        &self.settings
    }
}

/// Trimmed, blank is none, at most `MAX_INSTRUCTIONS_LEN` characters.
pub(crate) fn instructions_text(text: Option<String>) -> Result<Option<String>, PaymentError> {
    let Some(text) = text else { return Ok(None) };
    let text = text.trim();
    if text.chars().count() > PaymentMethod::MAX_INSTRUCTIONS_LEN {
        return Err(PaymentError::InstructionsTooLong);
    }
    Ok((!text.is_empty()).then(|| text.to_string()))
}

/// A set of IDs as stored: sorted, each once.
fn sorted<T: Ord>(mut ids: Vec<T>) -> Vec<T> {
    ids.sort();
    ids.dedup();
    ids
}

/// Plain-data form of a `PaymentMethod` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentMethodSnapshot {
    pub id: PaymentMethodId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub provider: ProviderKey,
    pub name: String,
    pub instructions: Option<String>,
    pub markets: Vec<MarketId>,
    pub channels: Vec<ChannelId>,
    pub enabled: bool,
    pub position: u32,
    pub settings: serde_json::Value,
}

impl PaymentMethod {
    pub fn snapshot(&self) -> PaymentMethodSnapshot {
        PaymentMethodSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            provider: self.provider.clone(),
            name: self.name.clone(),
            instructions: self.instructions.clone(),
            markets: self.markets.clone(),
            channels: self.channels.clone(),
            enabled: self.enabled,
            position: self.position,
            settings: self.settings.clone(),
        }
    }

    /// Rebuilds a method from storage (and builds new ones, through `new`), checking
    /// every invariant.
    pub fn restore(s: PaymentMethodSnapshot) -> Result<Self, PaymentError> {
        Ok(Self {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            provider: s.provider,
            name: required_text("name", &s.name)?,
            instructions: instructions_text(s.instructions)?,
            markets: sorted(s.markets),
            channels: sorted(s.channels),
            enabled: s.enabled,
            position: s.position,
            settings: s.settings,
            events: Recorder::default(),
            changed: BTreeSet::new(),
        })
    }

    /// Records that the method is being deleted; the repository's `delete` writes it.
    pub fn delete(&mut self) {
        self.changed.clear();
        let event = PaymentMethodDeleted {
            payment_method: self.id,
        };
        self.events.record(self.subject(), &event);
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    fn market(n: u128) -> MarketId {
        MarketId::from_uuid(Uuid::from_u128(n))
    }

    fn channel(n: u128) -> ChannelId {
        ChannelId::from_uuid(Uuid::from_u128(n))
    }

    fn new() -> NewPaymentMethod {
        NewPaymentMethod {
            id: PaymentMethodId::from_uuid(Uuid::from_u128(1)),
            provider: ProviderKey::manual(),
            name: " Cash on delivery ".to_string(),
            instructions: Some("  Pay the courier. ".to_string()),
            markets: vec![],
            channels: vec![],
            enabled: true,
            position: 0,
            settings: serde_json::json!({}),
        }
    }

    #[test]
    fn text_is_trimmed() {
        let m = PaymentMethod::new(new()).unwrap();
        assert_eq!(m.name(), "Cash on delivery");
        assert_eq!(m.instructions(), Some("Pay the courier."));
    }

    #[test]
    fn a_name_is_required() {
        let mut blank = new();
        blank.name = "  ".to_string();
        assert_eq!(
            PaymentMethod::new(blank),
            Err(PaymentError::Text(TextError::Missing("name")))
        );
    }

    #[test]
    fn instructions_are_capped_and_blank_is_none() {
        let mut m = PaymentMethod::new(new()).unwrap();
        let max = "x".repeat(PaymentMethod::MAX_INSTRUCTIONS_LEN);
        assert_eq!(m.set_instructions(Some(max.clone())), Ok(()));
        assert_eq!(
            m.set_instructions(Some(format!("{max}x"))),
            Err(PaymentError::InstructionsTooLong)
        );
        assert_eq!(m.set_instructions(Some(" ".to_string())), Ok(()));
        assert_eq!(m.instructions(), None);
    }

    #[test]
    fn markets_and_channels_are_a_set() {
        let mut m = PaymentMethod::new(new()).unwrap();
        m.set_markets(vec![market(3), market(1), market(3)]);
        m.set_channels(vec![channel(9), channel(9)]);
        assert_eq!(m.markets(), [market(1), market(3)]);
        assert_eq!(m.channels(), [channel(9)]);
    }

    #[test]
    fn offered_where_listed_or_everywhere_when_empty() {
        let mut m = PaymentMethod::new(new()).unwrap();
        assert!(m.offered_in(market(1), channel(1)));
        m.set_markets(vec![market(1)]);
        assert!(m.offered_in(market(1), channel(1)));
        assert!(!m.offered_in(market(2), channel(1)));
        m.set_channels(vec![channel(2)]);
        assert!(!m.offered_in(market(1), channel(1)));
        assert!(m.offered_in(market(1), channel(2)));
        m.set_enabled(false);
        assert!(!m.offered_in(market(1), channel(2)));
    }

    fn types(m: &mut PaymentMethod) -> Vec<&'static str> {
        m.take_events().iter().map(|e| e.event_type).collect()
    }

    #[test]
    fn a_new_method_records_its_creation_alone() {
        let mut m = PaymentMethod::new(new()).unwrap();
        m.set_enabled(false);
        assert_eq!(types(&mut m), ["payment_method.created"]);
    }

    #[test]
    fn a_stored_methods_changes_are_one_event_naming_the_fields() {
        let mut m = PaymentMethod::new(new()).unwrap();
        m.take_events();
        m.set_version(Version::new(1));
        assert!(m.take_events().is_empty(), "nothing changed");
        m.rename("Cash").unwrap();
        m.set_enabled(false);
        m.rename("Cash on delivery").unwrap();
        let events = m.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "payment_method.updated");
        assert_eq!(
            events[0].data["fields"],
            serde_json::json!(["enabled", "name"])
        );
        assert!(m.take_events().is_empty(), "taken once");
    }

    #[test]
    fn a_deleted_method_records_its_deletion_alone() {
        let mut m = PaymentMethod::new(new()).unwrap();
        m.take_events();
        m.set_version(Version::new(1));
        m.set_enabled(false);
        m.delete();
        assert_eq!(types(&mut m), ["payment_method.deleted"]);
    }

    #[test]
    fn restore_round_trips() {
        let mut m = PaymentMethod::new(new()).unwrap();
        m.set_markets(vec![market(2), market(1)]);
        m.set_position(7);
        m.take_events();
        assert_eq!(PaymentMethod::restore(m.snapshot()), Ok(m));
    }
}
