//! What happens to customers, as events (design note 0016, section 4). Payloads name
//! the customer and which fields changed, never their values: an email or an address is
//! personal data, and a consumer with `customers:read` fetches it (Q2).

use std::collections::BTreeSet;

use hypha_core::{CustomerId, Event, OrderId, PendingEvent, Subject, Timestamp};
use serde::Serialize;

use crate::consent::{Channel, ConsentState, OptInLevel};

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

/// `customer.deleted`: a customer without orders removed for good (design note 0032,
/// §7.2). Its custom field values follow (`CustomFieldCleanup`); its carts are unlinked
/// in the same write.
#[derive(Debug, Serialize)]
pub struct CustomerDeleted {
    pub customer: CustomerId,
}

impl Event for CustomerDeleted {
    const TYPE: &'static str = "customer.deleted";
    const VERSION: u16 = 1;
}

/// `customer.marketing_consent_updated` (design note 0032, §5): a channel's consent
/// state after a write that changed it. States aren't personal data, and they're what a
/// sync needs.
#[derive(Debug, Serialize)]
pub struct MarketingConsentUpdated {
    pub customer: CustomerId,
    pub channel: Channel,
    pub state: ConsentState,
    pub opt_in_level: OptInLevel,
}

impl Event for MarketingConsentUpdated {
    const TYPE: &'static str = "customer.marketing_consent_updated";
    const VERSION: u16 = 1;
}

/// `customer.erasure_requested` (design note 0032, §7.3): the customer's personal data
/// goes at `due_at` unless the request is cancelled.
#[derive(Debug, Serialize)]
pub struct CustomerErasureRequested {
    pub customer: CustomerId,
    /// Microseconds since the Unix epoch, as `Timestamp` counts.
    pub due_at: i64,
}

impl Event for CustomerErasureRequested {
    const TYPE: &'static str = "customer.erasure_requested";
    const VERSION: u16 = 1;
}

/// `customer.erasure_cancelled`: within the window, staff changed their mind.
#[derive(Debug, Serialize)]
pub struct CustomerErasureCancelled {
    pub customer: CustomerId,
}

impl Event for CustomerErasureCancelled {
    const TYPE: &'static str = "customer.erasure_cancelled";
    const VERSION: u16 = 1;
}

/// `customer.redacted` (design note 0032, §7.3; Shopify's `customers/redact`): the
/// person's data was removed from the customer and these orders. Every app with
/// `customers:read` gets it, subscribed or not, and must delete what it holds
/// (`docs/apps/privacy.md`).
#[derive(Debug, Serialize)]
pub struct CustomerRedacted {
    pub customer: CustomerId,
    pub orders: Vec<OrderId>,
}

impl Event for CustomerRedacted {
    const TYPE: &'static str = "customer.redacted";
    const VERSION: u16 = 1;
}

/// `customer.data_requested` (design note 0032, §7.4; Shopify's
/// `customers/data_request`): staff exported the customer's data, answering a request
/// to see what's held.
#[derive(Debug, Serialize)]
pub struct CustomerDataRequested {
    pub customer: CustomerId,
}

impl Event for CustomerDataRequested {
    const TYPE: &'static str = "customer.data_requested";
    const VERSION: u16 = 1;
}

pub fn customer_subject(customer: CustomerId) -> Subject {
    Subject::new(CUSTOMER, customer.as_uuid())
}

/// What changed since the customer was loaded: one event per write (plus one per
/// channel whose consent changed). Compares as always equal, so it never makes two
/// customers unequal.
#[derive(Debug, Clone, Default)]
pub(crate) struct Changes {
    created: bool,
    deleted: bool,
    fields: BTreeSet<&'static str>,
    consents: Vec<(Channel, ConsentState, OptInLevel)>,
    /// Events with their own shape (an erasure request), in order.
    extra: Vec<PendingEvent>,
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
            deleted: false,
            fields: BTreeSet::new(),
            consents: Vec::new(),
            extra: Vec::new(),
        }
    }

    pub(crate) fn field(&mut self, field: &'static str) {
        self.fields.insert(field);
    }

    pub(crate) fn deleted(&mut self) {
        self.deleted = true;
    }

    pub(crate) fn erasure_requested(&mut self, customer: CustomerId, due_at: Timestamp) {
        self.extra.push(PendingEvent::new(
            customer_subject(customer),
            &CustomerErasureRequested {
                customer,
                due_at: due_at.unix_micros(),
            },
        ));
    }

    pub(crate) fn erasure_cancelled(&mut self, customer: CustomerId) {
        self.extra.push(PendingEvent::new(
            customer_subject(customer),
            &CustomerErasureCancelled { customer },
        ));
    }

    pub(crate) fn consent(&mut self, channel: Channel, state: ConsentState, level: OptInLevel) {
        self.consents.retain(|(c, ..)| *c != channel);
        self.consents.push((channel, state, level));
    }

    pub(crate) fn take(&mut self, customer: CustomerId) -> Vec<PendingEvent> {
        let changes = std::mem::take(self);
        let subject = Subject::new(CUSTOMER, customer.as_uuid());
        if changes.deleted {
            return vec![PendingEvent::new(subject, &CustomerDeleted { customer })];
        }
        let mut events = Vec::new();
        if changes.created {
            events.push(PendingEvent::new(subject, &CustomerCreated { customer }));
        } else if !changes.fields.is_empty() {
            let fields = changes.fields.into_iter().collect();
            events.push(PendingEvent::new(
                subject,
                &CustomerUpdated { customer, fields },
            ));
        }
        for (channel, state, opt_in_level) in changes.consents {
            events.push(PendingEvent::new(
                subject,
                &MarketingConsentUpdated {
                    customer,
                    channel,
                    state,
                    opt_in_level,
                },
            ));
        }
        events.extend(changes.extra);
        events
    }
}
