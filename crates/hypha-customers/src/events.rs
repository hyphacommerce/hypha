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
    /// RFC 3339 in UTC.
    pub due_at: String,
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
        let due_at = jiff_free_rfc3339(due_at);
        self.extra.push(PendingEvent::new(
            customer_subject(customer),
            &CustomerErasureRequested { customer, due_at },
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

/// `at` as RFC 3339 in UTC with microseconds, without a date library: event payloads are
/// a contract and this crate stays small.
fn jiff_free_rfc3339(at: Timestamp) -> String {
    let micros = at.unix_micros();
    let secs = micros.div_euclid(1_000_000);
    let sub = micros.rem_euclid(1_000_000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}.{sub:06}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_without_a_date_library() {
        assert_eq!(
            jiff_free_rfc3339(Timestamp::from_unix_micros(1_767_225_600_000_000)),
            "2026-01-01T00:00:00.000000Z"
        );
        assert_eq!(
            jiff_free_rfc3339(Timestamp::from_unix_micros(951_782_400_000_001)),
            "2000-02-29T00:00:00.000001Z"
        );
    }
}
