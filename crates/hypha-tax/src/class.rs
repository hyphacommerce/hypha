use hypha_core::{
    PendingEvent, Recorded, Recorder, Stamped, Stamps, Subject, TaxClassId, TextError, Version,
    Versioned, required_text,
};

use crate::events::{TAX_CLASS, TaxClassChanged};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxClass {
    id: TaxClassId,
    version: Version,
    /// When it was stored and last changed, set by the adapter (design note 0016).
    stamps: Option<Stamps>,
    name: String,
    events: Recorder,
}

impl Recorded for TaxClass {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for TaxClass {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for TaxClass {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl TaxClass {
    pub fn new(id: TaxClassId, name: String) -> Result<Self, TextError> {
        let mut events = Recorder::default();
        events.record(
            Subject::new(TAX_CLASS, id.as_uuid()),
            &TaxClassChanged { tax_class: id },
        );
        Ok(Self {
            id,
            version: Version::NEW,
            stamps: None,
            name: required_text("name", &name)?,
            events,
        })
    }

    pub fn id(&self) -> TaxClassId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxClassSnapshot {
    pub id: TaxClassId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
}

impl TaxClass {
    pub fn snapshot(&self) -> TaxClassSnapshot {
        TaxClassSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            name: self.name.clone(),
        }
    }

    /// Rebuilds a class from storage, re-checking its name.
    pub fn restore(s: TaxClassSnapshot) -> Result<Self, TextError> {
        Ok(Self {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            name: required_text("name", &s.name)?,
            events: Recorder::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn a_class_needs_a_name() {
        assert_eq!(
            TaxClass::new(TaxClassId::from_uuid(Uuid::from_u128(1)), " ".to_string()),
            Err(TextError::Missing("name"))
        );
    }

    #[test]
    fn restore_round_trips() {
        let mut class = TaxClass::new(
            TaxClassId::from_uuid(Uuid::from_u128(1)),
            "Books".to_string(),
        )
        .unwrap();
        class.set_version(Version::new(2));
        assert_eq!(TaxClass::restore(class.snapshot()), Ok(class));
    }
}
