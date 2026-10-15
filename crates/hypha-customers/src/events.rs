//! What happens to customers, as events (design note 0016, section 4). Payloads name
//! the customer and which fields changed, never their values: an email or an address is
//! personal data, and a consumer with `customers:read` fetches it (Q2).

use std::collections::BTreeSet;

use hypha_core::{CustomerId, Event, PendingEvent, Subject};
use serde::Serialize;

pub const CUSTOMER: &str = "customer";

/// `customer.created`: at signup or at a guest's first order.
#[derive(Debug, Serialize)]
pub struct CustomerCreated {
    pub customer: CustomerId,
}

impl Event for CustomerCreated {
    const TYPE: &'static str = "customer.created";
    const VERSION: u16 = 1;
}

/// `customer.updated`: which of `email`, `first_name`, `last_name`, `phone`, `addresses`
/// and `defaults` changed.
#[derive(Debug, Serialize)]
pub struct CustomerUpdated {
    pub customer: CustomerId,
    pub fields: Vec<&'static str>,
}

impl Event for CustomerUpdated {
    const TYPE: &'static str = "customer.updated";
    const VERSION: u16 = 1;
}

/// What changed since the customer was loaded: one event per write. Compares as always
/// equal, so it never makes two customers unequal.
#[derive(Debug, Clone, Default)]
pub(crate) struct Changes {
    created: bool,
    fields: BTreeSet<&'static str>,
}

impl PartialEq for Changes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changes {}

impl Changes {
    pub(crate) fn created() -> Self {
        Self {
            created: true,
            fields: BTreeSet::new(),
        }
    }

    pub(crate) fn field(&mut self, field: &'static str) {
        self.fields.insert(field);
    }

    pub(crate) fn take(&mut self, customer: CustomerId) -> Vec<PendingEvent> {
        let changes = std::mem::take(self);
        let subject = Subject::new(CUSTOMER, customer.as_uuid());
        if changes.created {
            return vec![PendingEvent::new(subject, &CustomerCreated { customer })];
        }
        if changes.fields.is_empty() {
            return Vec::new();
        }
        let fields = changes.fields.into_iter().collect();
        vec![PendingEvent::new(
            subject,
            &CustomerUpdated { customer, fields },
        )]
    }
}
