use std::{fmt, str::FromStr};

use hypha_core::{
    ChannelId, PendingEvent, Recorded, Recorder, Stamped, Stamps, Subject, Version, Versioned,
    required_text,
};

use crate::{ChannelChanged, ChannelError, events::CHANNEL};

/// What a channel is for (design note 0020, CH2). The kinds only label channels: every
/// rule is the same for all of them. Responses may grow new kinds; clients should show
/// the ones they don't know as `custom`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelKind {
    /// A storefront selling to shoppers: the web store, an app.
    OnlineStore,
    /// Shopping agents, through the UCP adapter (later).
    Agentic,
    /// Anything else a merchant or an app sells through.
    Custom,
}

impl ChannelKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChannelKind::OnlineStore => "online_store",
            ChannelKind::Agentic => "agentic",
            ChannelKind::Custom => "custom",
        }
    }
}

impl fmt::Display for ChannelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ChannelKind {
    /// The text that isn't a kind.
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        [
            ChannelKind::OnlineStore,
            ChannelKind::Agentic,
            ChannelKind::Custom,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
        .ok_or_else(|| s.to_string())
    }
}

/// Where products are sold (design note 0020): a storefront, an agentic surface, an
/// app. Which products it sells are its publications (`ChannelRepo::publish`), kept
/// apart since they grow with the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    id: ChannelId,
    version: Version,
    stamps: Option<Stamps>,
    name: String,
    kind: ChannelKind,
    events: Recorder,
}

impl Channel {
    pub fn new(id: ChannelId, name: &str, kind: ChannelKind) -> Result<Self, ChannelError> {
        let mut channel = Self {
            id,
            version: Version::NEW,
            stamps: None,
            name: required_text("name", name)?,
            kind,
            events: Recorder::default(),
        };
        channel.changed();
        Ok(channel)
    }

    /// Renames it; the same name records nothing.
    pub fn rename(&mut self, name: &str) -> Result<(), ChannelError> {
        let name = required_text("name", name)?;
        if name != self.name {
            self.name = name;
            self.changed();
        }
        Ok(())
    }

    fn changed(&mut self) {
        self.events.record_once(
            Subject::new(CHANNEL, self.id.as_uuid()),
            &ChannelChanged { channel: self.id },
        );
    }

    pub fn id(&self) -> ChannelId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> ChannelKind {
        self.kind
    }

    pub fn snapshot(&self) -> ChannelSnapshot {
        ChannelSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            name: self.name.clone(),
            kind: self.kind,
        }
    }

    /// Rebuilds a channel from storage, with the same checks as `new`.
    pub fn restore(s: ChannelSnapshot) -> Result<Channel, ChannelError> {
        let mut channel = Channel::new(s.id, &s.name, s.kind)?;
        channel.version = s.version;
        channel.stamps = s.stamps;
        channel.events = Recorder::default();
        Ok(channel)
    }
}

/// Plain-data form of a `Channel` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelSnapshot {
    pub id: ChannelId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
    pub kind: ChannelKind,
}

impl Recorded for Channel {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for Channel {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Channel {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    fn id() -> ChannelId {
        ChannelId::from_uuid(Uuid::from_u128(1))
    }

    #[test]
    fn kinds_parse_their_own_strings_only() {
        for kind in [
            ChannelKind::OnlineStore,
            ChannelKind::Agentic,
            ChannelKind::Custom,
        ] {
            assert_eq!(kind.as_str().parse(), Ok(kind));
        }
        assert_eq!("pos".parse::<ChannelKind>(), Err("pos".to_string()));
    }

    #[test]
    fn a_name_is_required_and_trimmed() {
        assert_eq!(
            Channel::new(id(), "  ", ChannelKind::Custom),
            Err(ChannelError::Text(TextError::Missing("name")))
        );
        let c = Channel::new(id(), " Outlet ", ChannelKind::Custom).unwrap();
        assert_eq!(c.name(), "Outlet");
    }

    #[test]
    fn creating_and_renaming_record_one_event_per_write() {
        let mut c = Channel::new(id(), "Online Store", ChannelKind::OnlineStore).unwrap();
        c.rename("Web").unwrap();
        assert_eq!(c.take_events().len(), 1);
        c.rename("Web").unwrap();
        assert!(c.take_events().is_empty(), "the same name changes nothing");
    }

    #[test]
    fn restore_round_trips() {
        let mut c = Channel::new(id(), "Agents", ChannelKind::Agentic).unwrap();
        c.set_version(Version::new(4));
        assert_eq!(Channel::restore(c.snapshot()), Ok(c));
    }
}
