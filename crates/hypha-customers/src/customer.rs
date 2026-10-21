use std::collections::HashSet;

use hypha_core::{
    Address, AddressId, CustomerId, Email, PendingEvent, Recorded, Stamped, Stamps, Timestamp,
    Version, Versioned, optional_text,
};

use crate::{
    CustomerError, RestoreError,
    consent::{Channel, ConsentChange, ConsentSource, ConsentState, MarketingConsent},
    events::Changes,
};

/// An address in a customer's address book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAddress {
    pub id: AddressId,
    pub address: Address,
}

/// Someone who buys, identified by email: one customer per email (design note 0005,
/// C2). A guest becomes a customer on their first order; logging in comes later.
///
/// The customer owns its address book, the way a product owns its variants. The default
/// shipping and billing addresses always point into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Customer {
    id: CustomerId,
    version: Version,
    /// When it was stored and last changed, set by the adapter (design note 0016).
    stamps: Option<Stamps>,
    email: Email,
    first_name: Option<String>,
    last_name: Option<String>,
    phone: Option<String>,
    addresses: Vec<SavedAddress>,
    default_shipping: Option<AddressId>,
    default_billing: Option<AddressId>,
    /// Marketing consent per channel (design note 0032, §5).
    email_marketing: MarketingConsent,
    sms_marketing: MarketingConsent,
    /// When a requested erasure runs (design note 0032, §7.3): the cancel window.
    erasure_due_at: Option<Timestamp>,
    /// When the customer's personal data was erased (design note 0032, §7.3): read-only
    /// from then on.
    erased_at: Option<Timestamp>,
    /// What changed since it was loaded, for its events (design note 0016).
    changes: Changes,
}

impl Recorded for Customer {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.changes.take(self.id)
    }
}

impl Versioned for Customer {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Customer {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Customer {
    pub const MAX_FIELD_LEN: usize = hypha_core::MAX_TEXT_LEN;

    pub fn new(id: CustomerId, email: Email) -> Self {
        Self {
            id,
            version: Version::NEW,
            stamps: None,
            email,
            first_name: None,
            last_name: None,
            phone: None,
            addresses: Vec::new(),
            default_shipping: None,
            default_billing: None,
            email_marketing: MarketingConsent::default(),
            sms_marketing: MarketingConsent::default(),
            erasure_due_at: None,
            erased_at: None,
            changes: Changes::created(),
        }
    }

    /// The channel's consent.
    pub fn marketing_consent(&self, channel: Channel) -> MarketingConsent {
        match channel {
            Channel::Email => self.email_marketing,
            Channel::Sms => self.sms_marketing,
        }
    }

    /// Records the customer's consent for `channel` from `source` (design note 0032, §5):
    /// the newest `consented_at` (default `now`) wins, so an older write changes nothing
    /// and answers `false`. `not_subscribed` and `redacted` can't be asked for; SMS
    /// needs a phone.
    pub fn set_marketing_consent(
        &mut self,
        channel: Channel,
        change: ConsentChange,
        source: ConsentSource,
        now: Timestamp,
    ) -> Result<bool, CustomerError> {
        self.ensure_not_erased()?;
        if matches!(
            change.state,
            ConsentState::NotSubscribed | ConsentState::Redacted
        ) {
            return Err(CustomerError::ConsentStateNotSettable(change.state));
        }
        if channel == Channel::Sms && self.phone.is_none() {
            return Err(CustomerError::ConsentNeedsPhone);
        }
        let consent = match channel {
            Channel::Email => &mut self.email_marketing,
            Channel::Sms => &mut self.sms_marketing,
        };
        let applied = consent.apply(change, source, now);
        if applied {
            let (state, level) = (consent.state, consent.opt_in_level);
            self.changes.consent(channel, state, level);
        }
        Ok(applied)
    }

    /// When the customer was erased, if it was.
    pub fn erased_at(&self) -> Option<Timestamp> {
        self.erased_at
    }

    /// When a requested erasure runs, if one is.
    pub fn erasure_due_at(&self) -> Option<Timestamp> {
        self.erasure_due_at
    }

    /// Asks for the customer's personal data to go at `due_at` (design note 0032, §7.3),
    /// recording `customer.erasure_requested`. Refused while one is requested or once
    /// erased.
    pub fn request_erasure(&mut self, due_at: Timestamp) -> Result<(), CustomerError> {
        self.ensure_not_erased()?;
        if self.erasure_due_at.is_some() {
            return Err(CustomerError::ErasureAlreadyRequested);
        }
        self.erasure_due_at = Some(due_at);
        self.changes.erasure_requested(self.id, due_at);
        Ok(())
    }

    /// Takes a request back within its window, recording `customer.erasure_cancelled`.
    pub fn cancel_erasure(&mut self) -> Result<(), CustomerError> {
        self.ensure_not_erased()?;
        if self.erasure_due_at.is_none() {
            return Err(CustomerError::ErasureNotRequested);
        }
        self.erasure_due_at = None;
        self.changes.erasure_cancelled(self.id);
        Ok(())
    }

    /// Removes the person's data (design note 0032, §7.3): names, phone and the address
    /// book go, the email becomes the placeholder, consents become `redacted`, and the
    /// customer is read-only from `now`. Groups stay. Records nothing itself: the
    /// workflow records `customer.redacted` with the orders it redacted alongside. Doing
    /// it again changes nothing.
    pub fn erase(&mut self, now: Timestamp) -> bool {
        if self.erased_at.is_some() {
            return false;
        }
        self.email = Email::placeholder(self.id.as_uuid());
        self.first_name = None;
        self.last_name = None;
        self.phone = None;
        self.addresses.clear();
        self.default_shipping = None;
        self.default_billing = None;
        self.email_marketing.redact(now);
        self.sms_marketing.redact(now);
        self.erasure_due_at = None;
        self.erased_at = Some(now);
        true
    }

    pub fn is_erased(&self) -> bool {
        self.erased_at.is_some()
    }

    fn ensure_not_erased(&self) -> Result<(), CustomerError> {
        if self.is_erased() {
            return Err(CustomerError::Erased);
        }
        Ok(())
    }

    /// Marks the customer for removal: `StageCustomers::delete_customer` writes it with
    /// `customer.deleted`. Only for a customer without orders (design note 0032, CD7),
    /// which the workflow above checks.
    pub fn delete(&mut self) {
        self.changes.deleted();
    }

    /// A new email resets email consent to never asked (design note 0032, Q7): consent
    /// was given for an address, not a person.
    pub fn set_email(&mut self, email: Email) -> Result<(), CustomerError> {
        self.ensure_not_erased()?;
        if email != self.email {
            self.email = email;
            self.changes.field("email");
            if self.email_marketing.reset() {
                self.changes.consent(
                    Channel::Email,
                    self.email_marketing.state,
                    self.email_marketing.opt_in_level,
                );
            }
        }
        Ok(())
    }

    /// Trimmed; empty clears it.
    pub fn set_first_name(&mut self, name: Option<String>) -> Result<(), CustomerError> {
        self.ensure_not_erased()?;
        let name = optional_text("first_name", name)?;
        if name != self.first_name {
            self.first_name = name;
            self.changes.field("first_name");
        }
        Ok(())
    }

    /// Trimmed; empty clears it.
    pub fn set_last_name(&mut self, name: Option<String>) -> Result<(), CustomerError> {
        self.ensure_not_erased()?;
        let name = optional_text("last_name", name)?;
        if name != self.last_name {
            self.last_name = name;
            self.changes.field("last_name");
        }
        Ok(())
    }

    /// Trimmed; empty clears it. Free text for now (no E.164 check).
    pub fn set_phone(&mut self, phone: Option<String>) -> Result<(), CustomerError> {
        self.ensure_not_erased()?;
        let phone = optional_text("phone", phone)?;
        if phone != self.phone {
            self.phone = phone;
            self.changes.field("phone");
        }
        Ok(())
    }

    /// Adds an address to the address book, optionally as a default.
    pub fn add_address(
        &mut self,
        id: AddressId,
        address: Address,
        default_shipping: bool,
        default_billing: bool,
    ) -> Result<(), CustomerError> {
        self.ensure_not_erased()?;
        if self.addresses.iter().any(|a| a.id == id) {
            return Err(CustomerError::DuplicateAddress(id));
        }
        self.addresses.push(SavedAddress { id, address });
        self.changes.field("addresses");
        self.set_defaults(id, default_shipping, default_billing);
        Ok(())
    }

    /// Replaces an address. `default_*: false` stops it being that default, if it was.
    pub fn update_address(
        &mut self,
        id: AddressId,
        address: Address,
        default_shipping: bool,
        default_billing: bool,
    ) -> Result<(), CustomerError> {
        self.ensure_not_erased()?;
        let saved = self
            .addresses
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(CustomerError::AddressNotFound(id))?;
        saved.address = address;
        self.changes.field("addresses");
        self.set_defaults(id, default_shipping, default_billing);
        Ok(())
    }

    /// Removes an address, and any default that pointed at it.
    pub fn remove_address(&mut self, id: AddressId) -> Result<(), CustomerError> {
        self.ensure_not_erased()?;
        let before = self.addresses.len();
        self.addresses.retain(|a| a.id != id);
        if self.addresses.len() == before {
            return Err(CustomerError::AddressNotFound(id));
        }
        self.changes.field("addresses");
        self.set_defaults(id, false, false);
        Ok(())
    }

    fn set_defaults(&mut self, id: AddressId, shipping: bool, billing: bool) {
        for (default, wanted) in [
            (&mut self.default_shipping, shipping),
            (&mut self.default_billing, billing),
        ] {
            let before = *default;
            if wanted {
                *default = Some(id);
            } else if *default == Some(id) {
                *default = None;
            }
            if *default != before {
                self.changes.field("defaults");
            }
        }
    }

    pub fn id(&self) -> CustomerId {
        self.id
    }

    pub fn email(&self) -> &Email {
        &self.email
    }

    pub fn first_name(&self) -> Option<&str> {
        self.first_name.as_deref()
    }

    pub fn last_name(&self) -> Option<&str> {
        self.last_name.as_deref()
    }

    pub fn phone(&self) -> Option<&str> {
        self.phone.as_deref()
    }

    pub fn addresses(&self) -> &[SavedAddress] {
        &self.addresses
    }

    pub fn address(&self, id: AddressId) -> Option<&SavedAddress> {
        self.addresses.iter().find(|a| a.id == id)
    }

    pub fn default_shipping(&self) -> Option<AddressId> {
        self.default_shipping
    }

    pub fn default_billing(&self) -> Option<AddressId> {
        self.default_billing
    }
}

/// Plain-data form of a `Customer` for storage adapters (design note 0001, 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerSnapshot {
    pub id: CustomerId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub email: Email,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub addresses: Vec<SavedAddress>,
    pub default_shipping: Option<AddressId>,
    pub default_billing: Option<AddressId>,
    pub email_marketing: MarketingConsent,
    pub sms_marketing: MarketingConsent,
    pub erasure_due_at: Option<Timestamp>,
    pub erased_at: Option<Timestamp>,
}

impl Customer {
    pub fn snapshot(&self) -> CustomerSnapshot {
        CustomerSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            email: self.email.clone(),
            first_name: self.first_name.clone(),
            last_name: self.last_name.clone(),
            phone: self.phone.clone(),
            addresses: self.addresses.clone(),
            default_shipping: self.default_shipping,
            default_billing: self.default_billing,
            email_marketing: self.email_marketing,
            sms_marketing: self.sms_marketing,
            erasure_due_at: self.erasure_due_at,
            erased_at: self.erased_at,
        }
    }

    /// Rebuilds a customer from storage, with the same rules as the setters: each
    /// address once, defaults pointing into the address book.
    pub fn restore(s: CustomerSnapshot) -> Result<Customer, RestoreError> {
        let mut customer = Customer::new(s.id, s.email);
        customer.set_first_name(s.first_name)?;
        customer.set_last_name(s.last_name)?;
        customer.set_phone(s.phone)?;
        let mut ids = HashSet::new();
        for saved in &s.addresses {
            if !ids.insert(saved.id) {
                return Err(CustomerError::DuplicateAddress(saved.id).into());
            }
        }
        for default in [s.default_shipping, s.default_billing]
            .into_iter()
            .flatten()
        {
            if !ids.contains(&default) {
                return Err(RestoreError::UnknownDefaultAddress(default));
            }
        }
        customer.addresses = s.addresses;
        customer.default_shipping = s.default_shipping;
        customer.default_billing = s.default_billing;
        customer.email_marketing = s.email_marketing;
        customer.sms_marketing = s.sms_marketing;
        customer.erasure_due_at = s.erasure_due_at;
        customer.erased_at = s.erased_at;
        customer.version = s.version;
        customer.stamps = s.stamps;
        // Rebuilt through `new` and the setters; nothing happened, though.
        customer.changes = Changes::default();
        Ok(customer)
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{AddressParts, TextError};
    use uuid::Uuid;

    use super::*;

    fn addr_id(n: u128) -> AddressId {
        AddressId::from_uuid(Uuid::from_u128(n))
    }

    fn address(city: &str) -> Address {
        Address::new(AddressParts {
            first_name: None,
            last_name: "Horvat".to_string(),
            company: None,
            line1: "Ilica 1".to_string(),
            line2: None,
            city: city.to_string(),
            province: None,
            postal_code: None,
            country: "HR".to_string(),
            phone: None,
        })
        .unwrap()
    }

    fn customer() -> Customer {
        Customer::new(
            CustomerId::from_uuid(Uuid::from_u128(1)),
            Email::new("ana@example.hr").unwrap(),
        )
    }

    #[test]
    fn names_are_trimmed_and_empty_clears_them() {
        let mut c = customer();
        c.set_first_name(Some(" Ana ".to_string())).unwrap();
        assert_eq!(c.first_name(), Some("Ana"));
        c.set_first_name(Some("  ".to_string())).unwrap();
        assert_eq!(c.first_name(), None);
    }

    #[test]
    fn fields_are_capped_at_max_length() {
        let mut c = customer();
        let max = "x".repeat(Customer::MAX_FIELD_LEN);
        assert_eq!(c.set_phone(Some(max.clone())), Ok(()));
        assert_eq!(
            c.set_phone(Some(format!("{max}x"))),
            Err(CustomerError::Text(TextError::TooLong("phone")))
        );
    }

    #[test]
    fn adding_an_address_can_make_it_a_default() {
        let mut c = customer();
        c.add_address(addr_id(1), address("Zagreb"), true, false)
            .unwrap();
        c.add_address(addr_id(2), address("Split"), false, true)
            .unwrap();
        assert_eq!(c.default_shipping(), Some(addr_id(1)));
        assert_eq!(c.default_billing(), Some(addr_id(2)));
    }

    #[test]
    fn a_new_default_replaces_the_old_one() {
        let mut c = customer();
        c.add_address(addr_id(1), address("Zagreb"), true, true)
            .unwrap();
        c.add_address(addr_id(2), address("Split"), true, false)
            .unwrap();
        assert_eq!(c.default_shipping(), Some(addr_id(2)));
        assert_eq!(c.default_billing(), Some(addr_id(1)));
    }

    #[test]
    fn the_same_address_id_cannot_be_added_twice() {
        let mut c = customer();
        c.add_address(addr_id(1), address("Zagreb"), false, false)
            .unwrap();
        assert_eq!(
            c.add_address(addr_id(1), address("Split"), false, false),
            Err(CustomerError::DuplicateAddress(addr_id(1)))
        );
    }

    #[test]
    fn updating_replaces_the_address_and_can_drop_a_default() {
        let mut c = customer();
        c.add_address(addr_id(1), address("Zagreb"), true, true)
            .unwrap();
        c.update_address(addr_id(1), address("Rijeka"), true, false)
            .unwrap();
        assert_eq!(c.address(addr_id(1)).unwrap().address, address("Rijeka"));
        assert_eq!(c.default_shipping(), Some(addr_id(1)));
        assert_eq!(c.default_billing(), None);
    }

    #[test]
    fn updating_an_unknown_address_fails() {
        assert_eq!(
            customer().update_address(addr_id(9), address("Zagreb"), false, false),
            Err(CustomerError::AddressNotFound(addr_id(9)))
        );
    }

    #[test]
    fn removing_an_address_clears_its_defaults() {
        let mut c = customer();
        c.add_address(addr_id(1), address("Zagreb"), true, true)
            .unwrap();
        c.add_address(addr_id(2), address("Split"), false, false)
            .unwrap();
        c.remove_address(addr_id(1)).unwrap();
        assert_eq!(c.addresses().len(), 1);
        assert_eq!((c.default_shipping(), c.default_billing()), (None, None));
        assert_eq!(
            c.remove_address(addr_id(1)),
            Err(CustomerError::AddressNotFound(addr_id(1)))
        );
    }

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_unix_micros(secs * 1_000_000)
    }

    fn subscribe() -> ConsentChange {
        ConsentChange {
            state: ConsentState::Subscribed,
            opt_in_level: crate::OptInLevel::Single,
            consented_at: None,
        }
    }

    #[test]
    fn consent_cannot_be_never_asked_or_redacted_and_sms_needs_a_phone() {
        let mut c = customer();
        for state in [ConsentState::NotSubscribed, ConsentState::Redacted] {
            let change = ConsentChange {
                state,
                ..subscribe()
            };
            assert_eq!(
                c.set_marketing_consent(Channel::Email, change, ConsentSource::Admin, at(1)),
                Err(CustomerError::ConsentStateNotSettable(state))
            );
        }
        assert_eq!(
            c.set_marketing_consent(Channel::Sms, subscribe(), ConsentSource::Admin, at(1)),
            Err(CustomerError::ConsentNeedsPhone)
        );
        c.set_phone(Some("+385 91".to_string())).unwrap();
        assert_eq!(
            c.set_marketing_consent(Channel::Sms, subscribe(), ConsentSource::Admin, at(1)),
            Ok(true)
        );
        assert_eq!(c.marketing_consent(Channel::Sms).consented_at, Some(at(1)));
        assert_eq!(
            c.marketing_consent(Channel::Email).state,
            ConsentState::NotSubscribed
        );
    }

    #[test]
    fn a_new_email_resets_email_consent_and_records_it() {
        let mut c = customer();
        c.set_marketing_consent(Channel::Email, subscribe(), ConsentSource::Checkout, at(1))
            .unwrap();
        c.take_events();
        c.set_email(Email::new("ana2@example.hr").unwrap()).unwrap();
        assert_eq!(
            c.marketing_consent(Channel::Email),
            MarketingConsent::default()
        );
        let events = c.take_events();
        let types: Vec<&str> = events.iter().map(|e| e.event_type).collect();
        assert_eq!(
            types,
            ["customer.updated", "customer.marketing_consent_updated"]
        );
        // The same email again: nothing happens.
        c.set_email(Email::new("ana2@example.hr").unwrap()).unwrap();
        assert!(c.take_events().is_empty());
    }

    #[test]
    fn consent_round_trips_through_restore() {
        let mut c = customer();
        c.set_marketing_consent(Channel::Email, subscribe(), ConsentSource::Checkout, at(5))
            .unwrap();
        assert_eq!(Customer::restore(c.snapshot()), Ok(c));
    }

    #[test]
    fn erasure_is_requested_once_cancelled_and_then_run() {
        let mut c = customer();
        c.set_first_name(Some("Ana".to_string())).unwrap();
        c.add_address(addr_id(1), address("Zagreb"), true, true)
            .unwrap();
        c.take_events();
        assert_eq!(c.cancel_erasure(), Err(CustomerError::ErasureNotRequested));
        c.request_erasure(at(10)).unwrap();
        assert_eq!(
            c.request_erasure(at(11)),
            Err(CustomerError::ErasureAlreadyRequested)
        );
        let events = c.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "customer.erasure_requested");
        c.cancel_erasure().unwrap();
        assert_eq!(c.erasure_due_at(), None);
        assert_eq!(c.take_events()[0].event_type, "customer.erasure_cancelled");

        c.request_erasure(at(10)).unwrap();
        assert!(c.erase(at(10)));
        assert!(c.is_erased());
        assert!(c.email().is_placeholder());
        assert_eq!(c.first_name(), None);
        assert!(c.addresses().is_empty());
        assert_eq!(c.default_shipping(), None);
        assert_eq!(
            c.marketing_consent(Channel::Email).state,
            ConsentState::Redacted
        );
        assert_eq!(c.erasure_due_at(), None);
        assert!(!c.erase(at(11)), "a second run changes nothing");
        assert_eq!(
            c.set_first_name(Some("Ana".to_string())),
            Err(CustomerError::Erased)
        );
        assert_eq!(c.request_erasure(at(12)), Err(CustomerError::Erased));
        assert_eq!(Customer::restore(c.snapshot()), Ok(c));
    }

    #[test]
    fn restore_round_trips() {
        let mut c = customer();
        c.set_last_name(Some("Horvat".to_string())).unwrap();
        c.add_address(addr_id(1), address("Zagreb"), true, false)
            .unwrap();
        c.set_version(Version::new(4));
        assert_eq!(Customer::restore(c.snapshot()), Ok(c));
    }

    #[test]
    fn restore_rejects_a_default_outside_the_address_book() {
        let mut s = customer().snapshot();
        s.default_billing = Some(addr_id(7));
        assert_eq!(
            Customer::restore(s),
            Err(RestoreError::UnknownDefaultAddress(addr_id(7)))
        );
    }
}
