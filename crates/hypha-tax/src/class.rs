use hypha_core::{TaxClassId, Version, Versioned};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxClass {
    id: TaxClassId,
    version: Version,
    name: String,
}

impl Versioned for TaxClass {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl TaxClass {
    pub fn new(id: TaxClassId, name: String) -> Self {
        Self {
            id,
            version: Version::NEW,
            name,
        }
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
    pub name: String,
}

impl TaxClass {
    pub fn snapshot(&self) -> TaxClassSnapshot {
        TaxClassSnapshot {
            id: self.id,
            version: self.version,
            name: self.name.clone(),
        }
    }

    pub fn restore(s: TaxClassSnapshot) -> Self {
        Self {
            id: s.id,
            version: s.version,
            name: s.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn restore_round_trips() {
        let mut class = TaxClass::new(
            TaxClassId::from_uuid(Uuid::from_u128(1)),
            "Books".to_string(),
        );
        class.set_version(Version::new(2));
        assert_eq!(TaxClass::restore(class.snapshot()), class);
    }
}
