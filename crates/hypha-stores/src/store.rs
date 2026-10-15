use hypha_core::{Stamped, Stamps, StoreId, Version, Versioned};

use crate::error::StoreError;

/// A merchant's store. Store-wide settings (policies, custom field definitions; design
/// note 0002) will hang off it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    id: StoreId,
    version: Version,
    /// When it was stored and last changed, set by the adapter (design note 0016).
    stamps: Option<Stamps>,
    name: String,
}

impl Store {
    pub const MAX_NAME_CHARS: usize = 100;

    /// The name is trimmed; it must not be empty or longer than `MAX_NAME_CHARS`.
    pub fn new(id: StoreId, name: &str) -> Result<Self, StoreError> {
        Ok(Self {
            id,
            version: Version::NEW,
            stamps: None,
            name: valid_name(name)?,
        })
    }

    pub fn rename(&mut self, name: &str) -> Result<(), StoreError> {
        self.name = valid_name(name)?;
        Ok(())
    }

    pub fn id(&self) -> StoreId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn snapshot(&self) -> StoreSnapshot {
        StoreSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            name: self.name.clone(),
        }
    }

    /// Rebuilds a store from storage, re-checking the name rules.
    pub fn restore(s: StoreSnapshot) -> Result<Store, StoreError> {
        let mut store = Store::new(s.id, &s.name)?;
        store.version = s.version;
        store.stamps = s.stamps;
        Ok(store)
    }
}

impl Versioned for Store {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Store {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

/// Plain-data form of a `Store` for storage adapters (design note 0001, 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreSnapshot {
    pub id: StoreId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
}

fn valid_name(name: &str) -> Result<String, StoreError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StoreError::EmptyName);
    }
    if name.chars().count() > Store::MAX_NAME_CHARS {
        return Err(StoreError::NameTooLong {
            max: Store::MAX_NAME_CHARS,
        });
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn sid() -> StoreId {
        StoreId::from_uuid(Uuid::from_u128(1))
    }

    #[test]
    fn new_trims_the_name() {
        assert_eq!(
            Store::new(sid(), "  Acme  ").map(|s| s.name().to_string()),
            Ok("Acme".to_string())
        );
    }

    #[test]
    fn rejects_empty_or_blank_name() {
        assert_eq!(Store::new(sid(), ""), Err(StoreError::EmptyName));
        assert_eq!(Store::new(sid(), "   "), Err(StoreError::EmptyName));
    }

    #[test]
    fn name_limit_counts_characters_not_bytes() {
        let at_limit = "ž".repeat(Store::MAX_NAME_CHARS);
        assert!(Store::new(sid(), &at_limit).is_ok());
        assert_eq!(
            Store::new(sid(), &format!("{at_limit}ž")),
            Err(StoreError::NameTooLong { max: 100 })
        );
    }

    #[test]
    fn rename_validates_and_keeps_old_name_on_error() {
        let mut store = Store::new(sid(), "Acme").unwrap();
        assert_eq!(store.rename(" "), Err(StoreError::EmptyName));
        assert_eq!(store.name(), "Acme");
        assert_eq!(store.rename("Acme EU"), Ok(()));
        assert_eq!(store.name(), "Acme EU");
    }

    #[test]
    fn restore_round_trips_and_rechecks_rules() {
        let mut store = Store::new(sid(), "Acme").unwrap();
        store.set_version(Version::new(4));
        assert_eq!(Store::restore(store.snapshot()), Ok(store.clone()));

        let mut bad = store.snapshot();
        bad.name = String::new();
        assert_eq!(Store::restore(bad), Err(StoreError::EmptyName));
    }
}
