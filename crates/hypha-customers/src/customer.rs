use std::collections::HashSet;

use hypha_core::{Address, AddressId, CustomerId, Email, Version, Versioned, optional_text};

use crate::{CustomerError, RestoreError};

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
    email: Email,
    first_name: Option<String>,
    last_name: Option<String>,
    phone: Option<String>,
    addresses: Vec<SavedAddress>,
    default_shipping: Option<AddressId>,
    default_billing: Option<AddressId>,
}

impl Versioned for Customer {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Customer {
    pub const MAX_FIELD_LEN: usize = hypha_core::MAX_TEXT_LEN;

    pub fn new(id: CustomerId, email: Email) -> Self {
        Self {
            id,
            version: Version::NEW,
            email,
            first_name: None,
            last_name: None,
            phone: None,
            addresses: Vec::new(),
            default_shipping: None,
            default_billing: None,
        }
    }

    pub fn set_email(&mut self, email: Email) {
        self.email = email;
    }

    /// Trimmed; empty clears it.
    pub fn set_first_name(&mut self, name: Option<String>) -> Result<(), CustomerError> {
        self.first_name = optional_text("first_name", name)?;
        Ok(())
    }

    /// Trimmed; empty clears it.
    pub fn set_last_name(&mut self, name: Option<String>) -> Result<(), CustomerError> {
        self.last_name = optional_text("last_name", name)?;
        Ok(())
    }

    /// Trimmed; empty clears it. Free text for now (no E.164 check).
    pub fn set_phone(&mut self, phone: Option<String>) -> Result<(), CustomerError> {
        self.phone = optional_text("phone", phone)?;
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
        if self.addresses.iter().any(|a| a.id == id) {
            return Err(CustomerError::DuplicateAddress(id));
        }
        self.addresses.push(SavedAddress { id, address });
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
        let saved = self
            .addresses
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(CustomerError::AddressNotFound(id))?;
        saved.address = address;
        self.set_defaults(id, default_shipping, default_billing);
        Ok(())
    }

    /// Removes an address, and any default that pointed at it.
    pub fn remove_address(&mut self, id: AddressId) -> Result<(), CustomerError> {
        let before = self.addresses.len();
        self.addresses.retain(|a| a.id != id);
        if self.addresses.len() == before {
            return Err(CustomerError::AddressNotFound(id));
        }
        self.set_defaults(id, false, false);
        Ok(())
    }

    fn set_defaults(&mut self, id: AddressId, shipping: bool, billing: bool) {
        for (default, wanted) in [
            (&mut self.default_shipping, shipping),
            (&mut self.default_billing, billing),
        ] {
            if wanted {
                *default = Some(id);
            } else if *default == Some(id) {
                *default = None;
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
    pub email: Email,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub addresses: Vec<SavedAddress>,
    pub default_shipping: Option<AddressId>,
    pub default_billing: Option<AddressId>,
}

impl Customer {
    pub fn snapshot(&self) -> CustomerSnapshot {
        CustomerSnapshot {
            id: self.id,
            version: self.version,
            email: self.email.clone(),
            first_name: self.first_name.clone(),
            last_name: self.last_name.clone(),
            phone: self.phone.clone(),
            addresses: self.addresses.clone(),
            default_shipping: self.default_shipping,
            default_billing: self.default_billing,
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
        customer.version = s.version;
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
