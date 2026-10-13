use hypha_core::{TaxClassId, TextError, Version, Versioned, required_text};

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
    pub fn new(id: TaxClassId, name: String) -> Result<Self, TextError> {
        Ok(Self {
            id,
            version: Version::NEW,
            name: required_text("name", &name)?,
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

    /// Rebuilds a class from storage, re-checking its name.
    pub fn restore(s: TaxClassSnapshot) -> Result<Self, TextError> {
        Ok(Self {
            id: s.id,
            version: s.version,
            name: required_text("name", &s.name)?,
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
