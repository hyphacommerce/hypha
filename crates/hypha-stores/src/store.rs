use hypha_core::{
    Address, AddressParts, Email, Language, Stamped, Stamps, StoreId, Timezone, Version, Versioned,
    optional_text,
};

use crate::error::StoreError;

/// A merchant's store and its details (design note 0021, SU4). Store-wide settings
/// (policies, custom field definitions; design note 0002) will hang off it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    id: StoreId,
    version: Version,
    /// When it was stored and last changed, set by the adapter (design note 0016).
    stamps: Option<Stamps>,
    name: String,
    contact_email: Option<Email>,
    description: Option<String>,
    legal_name: Option<String>,
    legal_address: Option<Address>,
    timezone: Option<Timezone>,
    default_language: Language,
}

impl Store {
    pub const MAX_NAME_CHARS: usize = 100;
    /// A short plain-text description, for a storefront's header or meta tags.
    pub const MAX_DESCRIPTION_CHARS: usize = 500;

    /// The name is trimmed; it must not be empty or longer than `MAX_NAME_CHARS`. Every
    /// other detail starts empty, and the content language is English (`en`) until the
    /// merchant says otherwise (design note 0021, SU5).
    pub fn new(id: StoreId, name: &str) -> Result<Self, StoreError> {
        Ok(Self {
            id,
            version: Version::NEW,
            stamps: None,
            name: valid_name(name)?,
            contact_email: None,
            description: None,
            legal_name: None,
            legal_address: None,
            timezone: None,
            default_language: english(),
        })
    }

    pub fn rename(&mut self, name: &str) -> Result<(), StoreError> {
        self.name = valid_name(name)?;
        Ok(())
    }

    /// Where shoppers and agents can write to the store; `None` clears it.
    pub fn set_contact_email(&mut self, email: Option<Email>) {
        self.contact_email = email;
    }

    /// Plain text, trimmed, at most `MAX_DESCRIPTION_CHARS`; blank or `None` clears it.
    pub fn set_description(&mut self, description: Option<&str>) -> Result<(), StoreError> {
        self.description = valid_description(description)?;
        Ok(())
    }

    /// The registered business name, for invoices and legal pages; blank clears it.
    pub fn set_legal_name(&mut self, legal_name: Option<&str>) -> Result<(), StoreError> {
        self.legal_name = optional_text("legal_name", legal_name.map(str::to_string))?;
        Ok(())
    }

    pub fn set_legal_address(&mut self, address: Option<Address>) {
        self.legal_address = address;
    }

    pub fn set_timezone(&mut self, timezone: Option<Timezone>) {
        self.timezone = timezone;
    }

    /// The language every content field (titles, descriptions, option labels) is
    /// written in.
    pub fn set_default_language(&mut self, language: Language) {
        self.default_language = language;
    }

    pub fn id(&self) -> StoreId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn contact_email(&self) -> Option<&Email> {
        self.contact_email.as_ref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn legal_name(&self) -> Option<&str> {
        self.legal_name.as_deref()
    }

    pub fn legal_address(&self) -> Option<&Address> {
        self.legal_address.as_ref()
    }

    pub fn timezone(&self) -> Option<Timezone> {
        self.timezone
    }

    pub fn default_language(&self) -> &Language {
        &self.default_language
    }

    pub fn snapshot(&self) -> StoreSnapshot {
        StoreSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            name: self.name.clone(),
            contact_email: self.contact_email.clone(),
            description: self.description.clone(),
            legal_name: self.legal_name.clone(),
            legal_address: self.legal_address.as_ref().map(|a| a.parts().clone()),
            timezone: self.timezone,
            default_language: self.default_language.clone(),
        }
    }

    /// Rebuilds a store from storage, re-checking the rules of every field.
    pub fn restore(s: StoreSnapshot) -> Result<Store, StoreError> {
        let mut store = Store::new(s.id, &s.name)?;
        store.version = s.version;
        store.stamps = s.stamps;
        store.contact_email = s.contact_email;
        store.set_description(s.description.as_deref())?;
        store.set_legal_name(s.legal_name.as_deref())?;
        store.legal_address = s.legal_address.map(Address::new).transpose()?;
        store.timezone = s.timezone;
        store.default_language = s.default_language;
        Ok(store)
    }
}

/// `en`: the language a new store's content is written in.
fn english() -> Language {
    // A literal that `Language::new` always accepts (a test checks it).
    Language::new("en").expect("\"en\" is a valid language tag")
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

/// Plain-data form of a `Store` for storage adapters (design note 0001, 3.4). Values
/// with their own types (email, timezone, language) are parsed by the adapter; the
/// rest `restore` checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreSnapshot {
    pub id: StoreId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
    pub contact_email: Option<Email>,
    pub description: Option<String>,
    pub legal_name: Option<String>,
    pub legal_address: Option<AddressParts>,
    pub timezone: Option<Timezone>,
    pub default_language: Language,
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

fn valid_description(description: Option<&str>) -> Result<Option<String>, StoreError> {
    let Some(description) = description.map(str::trim).filter(|d| !d.is_empty()) else {
        return Ok(None);
    };
    if description.chars().count() > Store::MAX_DESCRIPTION_CHARS {
        return Err(StoreError::DescriptionTooLong {
            max: Store::MAX_DESCRIPTION_CHARS,
        });
    }
    Ok(Some(description.to_string()))
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

    #[test]
    fn a_new_store_writes_in_english_and_has_no_details() {
        let store = Store::new(sid(), "Acme").unwrap();
        assert_eq!(store.default_language().as_str(), "en");
        assert_eq!(
            (store.description(), store.legal_name(), store.timezone()),
            (None, None, None)
        );
        assert!(store.contact_email().is_none() && store.legal_address().is_none());
    }

    #[test]
    fn description_is_trimmed_blank_clears_and_500_characters_is_the_limit() {
        let mut store = Store::new(sid(), "Acme").unwrap();
        assert_eq!(store.set_description(Some("  Mugs.  ")), Ok(()));
        assert_eq!(store.description(), Some("Mugs."));
        assert_eq!(store.set_description(Some("   ")), Ok(()));
        assert_eq!(store.description(), None);
        let at_limit = "ž".repeat(Store::MAX_DESCRIPTION_CHARS);
        assert_eq!(store.set_description(Some(&at_limit)), Ok(()));
        assert_eq!(
            store.set_description(Some(&format!("{at_limit}ž"))),
            Err(StoreError::DescriptionTooLong { max: 500 })
        );
        assert_eq!(store.description(), Some(at_limit.as_str()));
    }

    #[test]
    fn legal_name_follows_the_text_rule() {
        let mut store = Store::new(sid(), "Acme").unwrap();
        assert_eq!(store.set_legal_name(Some(" Acme d.o.o. ")), Ok(()));
        assert_eq!(store.legal_name(), Some("Acme d.o.o."));
        assert_eq!(
            store.set_legal_name(Some(&"x".repeat(256))),
            Err(StoreError::Text(hypha_core::TextError::TooLong(
                "legal_name"
            )))
        );
    }

    #[test]
    fn restore_round_trips_every_detail() {
        let mut store = Store::new(sid(), "Acme").unwrap();
        store.set_contact_email(Some(Email::new("hi@acme.example").unwrap()));
        store.set_description(Some("Mugs")).unwrap();
        store.set_legal_name(Some("Acme d.o.o.")).unwrap();
        let parts = AddressParts {
            first_name: None,
            last_name: "Acme".to_string(),
            company: None,
            line1: "Ilica 1".to_string(),
            line2: None,
            city: "Zagreb".to_string(),
            province: None,
            postal_code: Some("10000".to_string()),
            country: "HR".to_string(),
            phone: None,
        };
        store.set_legal_address(Some(Address::new(parts).unwrap()));
        store.set_timezone(Some(Timezone::new("Europe/Zagreb").unwrap()));
        store.set_default_language(Language::new("hr").unwrap());
        assert_eq!(Store::restore(store.snapshot()), Ok(store));
    }
}
