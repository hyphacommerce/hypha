use std::collections::BTreeSet;

use hypha_core::{
    Address, AddressParts, Currency, Email, PendingEvent, Recorded, Stamped, Stamps, Subject,
    SupplierId, Version, Versioned, optional_text, required_text,
};

use crate::{
    PurchasingError,
    events::{SUPPLIER, SupplierCreated, SupplierDeleted, SupplierUpdated},
};

/// The most contacts a supplier keeps.
pub const MAX_CONTACTS: usize = 10;
/// The longest lead time, and the longest net payment terms, in days.
pub const MAX_LEAD_TIME_DAYS: u32 = 365;
pub const MAX_NET_DAYS: u32 = 365;
/// The longest notes, in characters.
pub const MAX_NOTES_CHARS: usize = 5000;

/// Someone at a supplier, as given: checked by `Supplier::new` and `update`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContactDraft {
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    /// Purchase orders are emailed to the primary contact.
    pub primary: bool,
}

/// Someone at a supplier: a name, and how to reach them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    name: String,
    email: Option<Email>,
    phone: Option<String>,
    primary: bool,
}

impl Contact {
    fn new(draft: ContactDraft) -> Result<Self, PurchasingError> {
        let email = optional_text("contact email", draft.email)?
            .map(|e| Email::new(&e))
            .transpose()?;
        Ok(Self {
            name: required_text("contact name", &draft.name)?,
            email,
            phone: optional_text("contact phone", draft.phone)?,
            primary: draft.primary,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn email(&self) -> Option<&Email> {
        self.email.as_ref()
    }

    pub fn phone(&self) -> Option<&str> {
        self.phone.as_deref()
    }

    pub fn primary(&self) -> bool {
        self.primary
    }

    fn draft(&self) -> ContactDraft {
        ContactDraft {
            name: self.name.clone(),
            email: self.email.as_ref().map(|e| e.as_str().to_string()),
            phone: self.phone.clone(),
            primary: self.primary,
        }
    }
}

/// When a supplier is paid. Typed rather than free text, so a due date can be worked out
/// and acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentDue {
    /// Before the goods are sent.
    InAdvance,
    /// When the goods arrive.
    OnReceipt,
    /// `days` (1 to `MAX_NET_DAYS`) after the order.
    Net { days: u32 },
}

impl PaymentDue {
    pub fn as_str(self) -> &'static str {
        match self {
            PaymentDue::InAdvance => "in_advance",
            PaymentDue::OnReceipt => "on_receipt",
            PaymentDue::Net { .. } => "net",
        }
    }
}

/// Payment terms as given: checked by `PaymentTerms::new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentTermsDraft {
    pub due: PaymentDue,
    pub note: Option<String>,
}

/// When a supplier is paid, with a note ("2% off within 10 days").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentTerms {
    due: PaymentDue,
    note: Option<String>,
}

impl PaymentTerms {
    pub fn new(due: PaymentDue, note: Option<String>) -> Result<Self, PurchasingError> {
        if let PaymentDue::Net { days } = due
            && !(1..=MAX_NET_DAYS).contains(&days)
        {
            return Err(PurchasingError::InvalidNetDays {
                days,
                max: MAX_NET_DAYS,
            });
        }
        Ok(Self {
            due,
            note: optional_text("payment terms note", note)?,
        })
    }

    pub fn due(&self) -> PaymentDue {
        self.due
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    fn draft(&self) -> PaymentTermsDraft {
        PaymentTermsDraft {
            due: self.due,
            note: self.note.clone(),
        }
    }
}

/// A supplier as given: checked as a whole by `Supplier::new` and `update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierDraft {
    pub name: String,
    pub contacts: Vec<ContactDraft>,
    pub address: Option<AddressParts>,
    /// The default for its purchase orders, and its catalogue's costs.
    pub currency: Currency,
    pub payment_terms: Option<PaymentTermsDraft>,
    /// The default for a purchase order's expected date.
    pub lead_time_days: Option<u32>,
    pub tax_number: Option<String>,
    /// For staff only.
    pub notes: Option<String>,
    /// Archived suppliers take no new purchase orders.
    pub archived: bool,
}

impl SupplierDraft {
    /// A draft with only the required fields.
    pub fn new(name: &str, currency: Currency) -> Self {
        Self {
            name: name.to_string(),
            contacts: Vec::new(),
            address: None,
            currency,
            payment_terms: None,
            lead_time_days: None,
            tax_number: None,
            notes: None,
            archived: false,
        }
    }
}

/// The checked fields of a supplier.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fields {
    name: String,
    contacts: Vec<Contact>,
    address: Option<Address>,
    currency: Currency,
    payment_terms: Option<PaymentTerms>,
    lead_time_days: Option<u32>,
    tax_number: Option<String>,
    notes: Option<String>,
    archived: bool,
}

fn check(draft: SupplierDraft) -> Result<Fields, PurchasingError> {
    if draft.contacts.len() > MAX_CONTACTS {
        return Err(PurchasingError::TooManyContacts { max: MAX_CONTACTS });
    }
    if draft.contacts.iter().filter(|c| c.primary).count() > 1 {
        return Err(PurchasingError::SeveralPrimaryContacts);
    }
    if let Some(days) = draft.lead_time_days {
        check_lead_time(days)?;
    }
    Ok(Fields {
        name: required_text("name", &draft.name)?,
        contacts: draft
            .contacts
            .into_iter()
            .map(Contact::new)
            .collect::<Result<_, _>>()?,
        address: draft.address.map(Address::new).transpose()?,
        currency: draft.currency,
        payment_terms: draft
            .payment_terms
            .map(|t| PaymentTerms::new(t.due, t.note))
            .transpose()?,
        lead_time_days: draft.lead_time_days,
        tax_number: optional_text("tax number", draft.tax_number)?,
        notes: notes(draft.notes)?,
        archived: draft.archived,
    })
}

pub(crate) fn check_lead_time(days: u32) -> Result<(), PurchasingError> {
    if days > MAX_LEAD_TIME_DAYS {
        return Err(PurchasingError::LeadTimeTooLong {
            days,
            max: MAX_LEAD_TIME_DAYS,
        });
    }
    Ok(())
}

/// Trimmed; blank is none.
fn notes(notes: Option<String>) -> Result<Option<String>, PurchasingError> {
    let Some(notes) = notes else { return Ok(None) };
    let notes = notes.trim();
    if notes.chars().count() > MAX_NOTES_CHARS {
        return Err(PurchasingError::NotesTooLong {
            max: MAX_NOTES_CHARS,
        });
    }
    Ok((!notes.is_empty()).then(|| notes.to_string()))
}

/// A business the store buys stock from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Supplier {
    id: SupplierId,
    version: Version,
    stamps: Option<Stamps>,
    fields: Fields,
    changes: Changes,
}

/// What happened since it was loaded, coalesced into one event per write. Compares as
/// always equal, so it never makes two suppliers unequal.
#[derive(Debug, Clone, Default)]
struct Changes {
    created: bool,
    deleted: bool,
    fields: BTreeSet<&'static str>,
}

impl PartialEq for Changes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changes {}

impl Recorded for Supplier {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let changes = std::mem::take(&mut self.changes);
        let subject = Subject::new(SUPPLIER, self.id.as_uuid());
        let supplier = self.id;
        if changes.deleted {
            return vec![PendingEvent::new(subject, &SupplierDeleted { supplier })];
        }
        if changes.created {
            return vec![PendingEvent::new(subject, &SupplierCreated { supplier })];
        }
        if changes.fields.is_empty() {
            return Vec::new();
        }
        let event = SupplierUpdated {
            supplier,
            fields: changes.fields.into_iter().collect(),
        };
        vec![PendingEvent::new(subject, &event)]
    }
}

impl Versioned for Supplier {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Supplier {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Supplier {
    pub fn new(id: SupplierId, draft: SupplierDraft) -> Result<Self, PurchasingError> {
        Ok(Self {
            id,
            version: Version::NEW,
            stamps: None,
            fields: check(draft)?,
            changes: Changes {
                created: true,
                ..Changes::default()
            },
        })
    }

    /// Replaces everything with `draft`, checked like a new supplier; what changed goes
    /// into `supplier.updated`. Whether the currency may change (no catalogue cost in the
    /// old one) is the command's to check.
    pub fn update(&mut self, draft: SupplierDraft) -> Result<(), PurchasingError> {
        let new = check(draft)?;
        let old = &self.fields;
        let changed = [
            ("name", new.name != old.name),
            ("contacts", new.contacts != old.contacts),
            ("address", new.address != old.address),
            ("currency", new.currency != old.currency),
            ("payment_terms", new.payment_terms != old.payment_terms),
            ("lead_time_days", new.lead_time_days != old.lead_time_days),
            ("tax_number", new.tax_number != old.tax_number),
            ("notes", new.notes != old.notes),
            ("archived", new.archived != old.archived),
        ];
        self.changes.fields.extend(
            changed
                .into_iter()
                .filter(|(_, changed)| *changed)
                .map(|(field, _)| field),
        );
        self.fields = new;
        Ok(())
    }

    /// Whether anything changed since it was loaded (or it's new or deleted).
    pub fn is_changed(&self) -> bool {
        self.changes.created || self.changes.deleted || !self.changes.fields.is_empty()
    }

    /// Records `supplier.deleted`; the repository removes it.
    pub fn delete(&mut self) {
        self.changes.deleted = true;
    }

    /// The supplier as a draft, to change some of it and `update`.
    pub fn draft(&self) -> SupplierDraft {
        let f = &self.fields;
        SupplierDraft {
            name: f.name.clone(),
            contacts: f.contacts.iter().map(Contact::draft).collect(),
            address: f.address.as_ref().map(|a| a.parts().clone()),
            currency: f.currency,
            payment_terms: f.payment_terms.as_ref().map(PaymentTerms::draft),
            lead_time_days: f.lead_time_days,
            tax_number: f.tax_number.clone(),
            notes: f.notes.clone(),
            archived: f.archived,
        }
    }

    pub fn id(&self) -> SupplierId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.fields.name
    }

    pub fn contacts(&self) -> &[Contact] {
        &self.fields.contacts
    }

    /// Where purchase orders are emailed.
    pub fn primary_contact(&self) -> Option<&Contact> {
        self.fields.contacts.iter().find(|c| c.primary)
    }

    pub fn address(&self) -> Option<&Address> {
        self.fields.address.as_ref()
    }

    pub fn currency(&self) -> Currency {
        self.fields.currency
    }

    pub fn payment_terms(&self) -> Option<&PaymentTerms> {
        self.fields.payment_terms.as_ref()
    }

    pub fn lead_time_days(&self) -> Option<u32> {
        self.fields.lead_time_days
    }

    pub fn tax_number(&self) -> Option<&str> {
        self.fields.tax_number.as_deref()
    }

    pub fn notes(&self) -> Option<&str> {
        self.fields.notes.as_deref()
    }

    pub fn archived(&self) -> bool {
        self.fields.archived
    }

    pub fn snapshot(&self) -> SupplierSnapshot {
        SupplierSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            draft: self.draft(),
        }
    }

    /// Rebuilds a supplier from storage, checked like a new one.
    pub fn restore(s: SupplierSnapshot) -> Result<Supplier, PurchasingError> {
        Ok(Supplier {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            fields: check(s.draft)?,
            changes: Changes::default(),
        })
    }
}

/// Plain-data form of a `Supplier` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierSnapshot {
    pub id: SupplierId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub draft: SupplierDraft,
}

#[cfg(test)]
mod tests {
    use hypha_core::{EmailError, TextError};
    use uuid::Uuid;

    use super::*;

    fn id() -> SupplierId {
        SupplierId::from_uuid(Uuid::from_u128(1))
    }

    fn contact(name: &str, primary: bool) -> ContactDraft {
        ContactDraft {
            name: name.to_string(),
            email: Some(format!("{}@mycelium.example", name.to_lowercase())),
            phone: None,
            primary,
        }
    }

    fn draft() -> SupplierDraft {
        SupplierDraft::new("Spore Labs", Currency::EUR)
    }

    #[test]
    fn a_new_supplier_records_created() {
        let mut s = Supplier::new(id(), draft()).unwrap();
        let events = s.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "supplier.created");
        assert!(s.take_events().is_empty());
    }

    #[test]
    fn the_name_is_required_and_trimmed() {
        let mut d = draft();
        d.name = "  Spore Labs ".to_string();
        assert_eq!(Supplier::new(id(), d).unwrap().name(), "Spore Labs");
        let mut d = draft();
        d.name = " ".to_string();
        assert_eq!(
            Supplier::new(id(), d),
            Err(PurchasingError::Text(TextError::Missing("name")))
        );
    }

    #[test]
    fn at_most_ten_contacts() {
        let mut d = draft();
        d.contacts = (0..10).map(|n| contact(&format!("C{n}"), false)).collect();
        assert!(Supplier::new(id(), d.clone()).is_ok());
        d.contacts.push(contact("Eleven", false));
        assert_eq!(
            Supplier::new(id(), d),
            Err(PurchasingError::TooManyContacts { max: 10 })
        );
    }

    #[test]
    fn at_most_one_primary_contact() {
        let mut d = draft();
        d.contacts = vec![contact("Ana", true), contact("Ben", false)];
        let s = Supplier::new(id(), d.clone()).unwrap();
        assert_eq!(s.primary_contact().map(Contact::name), Some("Ana"));
        d.contacts[1].primary = true;
        assert_eq!(
            Supplier::new(id(), d),
            Err(PurchasingError::SeveralPrimaryContacts)
        );
    }

    #[test]
    fn a_contact_needs_a_name_and_a_valid_email() {
        let mut d = draft();
        d.contacts = vec![ContactDraft {
            name: "Ana".to_string(),
            email: Some("not an email".to_string()),
            ..ContactDraft::default()
        }];
        assert!(matches!(
            Supplier::new(id(), d.clone()),
            Err(PurchasingError::Email(EmailError::Invalid(_)))
        ));
        d.contacts[0].email = Some("  ".to_string());
        assert_eq!(
            Supplier::new(id(), d.clone()).unwrap().contacts()[0].email(),
            None
        );
        d.contacts[0].name = String::new();
        assert_eq!(
            Supplier::new(id(), d),
            Err(PurchasingError::Text(TextError::Missing("contact name")))
        );
    }

    #[test]
    fn net_terms_are_one_to_365_days() {
        assert!(PaymentTerms::new(PaymentDue::Net { days: 1 }, None).is_ok());
        assert!(PaymentTerms::new(PaymentDue::Net { days: 365 }, None).is_ok());
        for days in [0, 366] {
            assert_eq!(
                PaymentTerms::new(PaymentDue::Net { days }, None),
                Err(PurchasingError::InvalidNetDays { days, max: 365 })
            );
        }
        assert_eq!(
            PaymentTerms::new(PaymentDue::OnReceipt, Some(" ".to_string()))
                .unwrap()
                .note(),
            None
        );
    }

    #[test]
    fn a_lead_time_is_at_most_a_year() {
        let mut d = draft();
        d.lead_time_days = Some(365);
        assert!(Supplier::new(id(), d.clone()).is_ok());
        d.lead_time_days = Some(366);
        assert_eq!(
            Supplier::new(id(), d),
            Err(PurchasingError::LeadTimeTooLong {
                days: 366,
                max: 365
            })
        );
    }

    #[test]
    fn notes_hold_at_most_5000_characters() {
        let mut d = draft();
        d.notes = Some("é".repeat(5000));
        assert!(Supplier::new(id(), d.clone()).is_ok());
        d.notes = Some("é".repeat(5001));
        assert_eq!(
            Supplier::new(id(), d),
            Err(PurchasingError::NotesTooLong { max: 5000 })
        );
    }

    #[test]
    fn update_records_only_what_changed() {
        let mut s = Supplier::new(id(), draft()).unwrap();
        s.take_events();
        let mut d = s.draft();
        s.update(d.clone()).unwrap();
        assert!(!s.is_changed());
        assert!(s.take_events().is_empty());
        d.archived = true;
        d.currency = Currency::USD;
        s.update(d).unwrap();
        let events = s.take_events();
        assert_eq!(events[0].event_type, "supplier.updated");
        assert_eq!(
            events[0].data["fields"],
            serde_json::json!(["archived", "currency"])
        );
    }

    #[test]
    fn a_failed_update_changes_nothing() {
        let mut s = Supplier::new(id(), draft()).unwrap();
        let mut d = s.draft();
        d.name = String::new();
        d.archived = true;
        assert!(s.update(d).is_err());
        assert!(!s.archived());
    }

    #[test]
    fn delete_records_deleted() {
        let mut s = Supplier::new(id(), draft()).unwrap();
        s.take_events();
        s.delete();
        assert_eq!(s.take_events()[0].event_type, "supplier.deleted");
    }

    #[test]
    fn restore_round_trips_the_snapshot() {
        let mut d = draft();
        d.contacts = vec![contact("Ana", true)];
        d.payment_terms = Some(PaymentTermsDraft {
            due: PaymentDue::Net { days: 30 },
            note: Some("2% off within 10 days".to_string()),
        });
        let s = Supplier::new(id(), d).unwrap();
        assert_eq!(Supplier::restore(s.snapshot()), Ok(s));
    }
}
