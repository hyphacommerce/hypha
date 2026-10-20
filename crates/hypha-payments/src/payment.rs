use std::{fmt, str::FromStr};

use hypha_core::{
    FinancialStatus, Money, OrderId, PaymentId, PaymentMethodId, PendingEvent, Recorded, Recorder,
    Stamped, Stamps, Subject, Timestamp, Version, Versioned, optional_text,
};

use crate::{
    PaymentError, PaymentMethod, ProviderKey,
    events::{
        PAYMENT, PaymentAuthorized, PaymentCreated, PaymentFailed, PaymentPaid, PaymentRefunded,
        PaymentVoided,
    },
    method::instructions_text,
};

/// Where one payment is (design note 0027, PA7). Manual payments use `Pending`, `Paid`
/// and `Voided`; the rest wait for providers that move money that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaymentStatus {
    /// Started; nothing collected yet (cash on delivery before delivery).
    Pending,
    /// The money is held (a card authorization), not yet captured.
    Authorized,
    /// Collected.
    Paid,
    /// The provider declined it.
    Failed,
    /// Cancelled before money moved.
    Voided,
    /// Some of it was given back.
    PartiallyRefunded,
    /// All of it was given back.
    Refunded,
}

impl PaymentStatus {
    pub const ALL: [PaymentStatus; 7] = [
        PaymentStatus::Pending,
        PaymentStatus::Authorized,
        PaymentStatus::Paid,
        PaymentStatus::Failed,
        PaymentStatus::Voided,
        PaymentStatus::PartiallyRefunded,
        PaymentStatus::Refunded,
    ];

    /// The same string used in JSON and storage, e.g. "partially_refunded".
    pub fn as_str(&self) -> &'static str {
        match self {
            PaymentStatus::Pending => "pending",
            PaymentStatus::Authorized => "authorized",
            PaymentStatus::Paid => "paid",
            PaymentStatus::Failed => "failed",
            PaymentStatus::Voided => "voided",
            PaymentStatus::PartiallyRefunded => "partially_refunded",
            PaymentStatus::Refunded => "refunded",
        }
    }

    /// The moves the domain allows (§2): `pending` → `authorized` → `paid` →
    /// `partially_refunded` → `refunded`, `pending` straight to `paid`, and `voided` or
    /// `failed` from `pending` or `authorized`. A partial refund may follow another.
    pub fn can_become(self, to: PaymentStatus) -> bool {
        use PaymentStatus as S;
        matches!(
            (self, to),
            (S::Pending, S::Authorized | S::Paid | S::Failed | S::Voided)
                | (S::Authorized, S::Paid | S::Failed | S::Voided)
                | (S::Paid, S::PartiallyRefunded | S::Refunded)
                | (S::PartiallyRefunded, S::PartiallyRefunded | S::Refunded)
        )
    }

    /// Money was collected (and maybe partly or wholly given back since).
    pub fn captured(self) -> bool {
        matches!(
            self,
            PaymentStatus::Paid | PaymentStatus::PartiallyRefunded | PaymentStatus::Refunded
        )
    }

    /// Nothing collected yet, and it could still be: what marking an order paid
    /// records. Cancelling an order voids the `pending` ones; an `authorized` one holds
    /// money only its provider can release, so cancelling refuses it
    /// (`payment_authorized`) until providers can.
    pub fn is_open(self) -> bool {
        matches!(self, PaymentStatus::Pending | PaymentStatus::Authorized)
    }
}

impl fmt::Display for PaymentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PaymentStatus {
    /// The text that didn't match a status.
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        PaymentStatus::ALL
            .into_iter()
            .find(|status| status.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

/// What a new payment is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPayment {
    pub id: PaymentId,
    pub order: OrderId,
    pub method: Option<PaymentMethodId>,
    pub method_name: Option<String>,
    pub provider: ProviderKey,
    pub amount: Money,
    pub instructions: Option<String>,
}

impl NewPayment {
    /// A payment of `amount` for `order` by `method`, copying what the shopper is shown
    /// (its name and instructions), so later edits to the method don't change it.
    pub fn by(method: &PaymentMethod, id: PaymentId, order: OrderId, amount: Money) -> Self {
        Self {
            id,
            order,
            method: Some(method.id()),
            method_name: Some(method.name().to_string()),
            provider: method.provider().clone(),
            amount,
            instructions: method.instructions().map(str::to_string),
        }
    }

    /// Money staff record on an order that has no payment to record it on (placed
    /// before payments existed, or with nothing to pay at checkout): `manual`, without a
    /// method.
    pub fn recorded_by_hand(id: PaymentId, order: OrderId, amount: Money) -> Self {
        Self {
            id,
            order,
            method: None,
            method_name: None,
            provider: ProviderKey::manual(),
            amount,
            instructions: None,
        }
    }
}

/// One attempt to collect an order's money (design note 0027, PA2).
///
/// Invariants, checked by `new` and `restore`: a non-negative amount; `refunded` in the
/// same currency, between zero and the amount, and above zero only once refunded (all
/// of it for `refunded`); `paid_at` set exactly when money was collected and
/// `voided_at` exactly when voided; text trimmed and within its length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    id: PaymentId,
    version: Version,
    stamps: Option<Stamps>,
    order: OrderId,
    /// `None` for money staff recorded on an order placed without a method.
    method: Option<PaymentMethodId>,
    /// The method's name when the payment was made.
    method_name: Option<String>,
    /// Copied from the method, so a deleted method can't orphan it.
    provider: ProviderKey,
    /// In the order's currency: the whole total for now.
    amount: Money,
    status: PaymentStatus,
    /// What was given back; zero until refunds exist.
    refunded: Money,
    /// The provider's ID; for manual payments, what staff typed (a bank reference).
    reference: Option<String>,
    /// The provider's IDs of the refunds counted in `refunded`, oldest first: a refund
    /// reported again under one of them changes nothing (`refund`).
    refund_references: Vec<String>,
    /// The provider's attempt the payment collects through (a Stripe PaymentIntent), set
    /// only by a provider's report naming it (`note_attempt`), never by staff or by an
    /// operation's own ID: what a report naming another attempt is compared with.
    attempt: Option<String>,
    /// The method's instructions at checkout.
    instructions: Option<String>,
    paid_at: Option<Timestamp>,
    voided_at: Option<Timestamp>,
    events: Recorder,
}

impl Recorded for Payment {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for Payment {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Payment {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Payment {
    /// A pending payment, recording `payment.created`.
    pub fn new(new: NewPayment) -> Result<Self, PaymentError> {
        let currency = new.amount.currency();
        let mut payment = Self::restore(PaymentSnapshot {
            id: new.id,
            version: Version::NEW,
            stamps: None,
            order: new.order,
            method: new.method,
            method_name: new.method_name,
            provider: new.provider,
            amount: new.amount,
            status: PaymentStatus::Pending,
            refunded: Money::zero(currency),
            reference: None,
            refund_references: Vec::new(),
            attempt: None,
            instructions: new.instructions,
            paid_at: None,
            voided_at: None,
        })?;
        let created = PaymentCreated {
            payment: payment.id,
            order: payment.order,
            method: payment.method,
            provider: payment.provider.to_string(),
            amount: payment.amount,
        };
        payment.events.record(payment.subject(), &created);
        Ok(payment)
    }

    fn subject(&self) -> Subject {
        Subject::new(PAYMENT, self.id.as_uuid())
    }

    fn move_to(&mut self, to: PaymentStatus) -> Result<(), PaymentError> {
        if !self.status.can_become(to) {
            return Err(PaymentError::InvalidTransition {
                payment: self.id,
                from: self.status,
                to,
            });
        }
        self.status = to;
        Ok(())
    }

    /// Records the money as collected at `at` (`payment.paid`), with `reference` when
    /// given (a blank one keeps none). Returns whether that changed anything: a paid
    /// payment stays as it was. Refused unless pending or authorized.
    pub fn pay(&mut self, at: Timestamp, reference: Option<String>) -> Result<bool, PaymentError> {
        if self.status == PaymentStatus::Paid {
            return Ok(false);
        }
        let reference = optional_text("reference", reference)?;
        self.move_to(PaymentStatus::Paid)?;
        self.paid_at = Some(at);
        if reference.is_some() {
            self.reference = reference;
        }
        let paid = PaymentPaid {
            payment: self.id,
            order: self.order,
            method: self.method,
            provider: self.provider.to_string(),
            amount: self.amount,
            reference: self.reference.clone(),
        };
        self.events.record(self.subject(), &paid);
        Ok(true)
    }

    /// Cancels it before money moved (`payment.voided`); returns whether that changed
    /// anything (a voided payment stays voided at its first time). Refused unless
    /// pending or authorized.
    pub fn void(&mut self, at: Timestamp) -> Result<bool, PaymentError> {
        if self.status == PaymentStatus::Voided {
            return Ok(false);
        }
        self.move_to(PaymentStatus::Voided)?;
        self.voided_at = Some(at);
        let voided = PaymentVoided {
            payment: self.id,
            order: self.order,
            method: self.method,
            provider: self.provider.to_string(),
            amount: self.amount,
        };
        self.events.record(self.subject(), &voided);
        Ok(true)
    }

    /// The provider holds the money (a card authorization; `payment.authorized`), with
    /// `reference` when given. Returns whether that changed anything: an authorized
    /// payment stays as it was. Refused unless pending.
    pub fn authorize(&mut self, reference: Option<String>) -> Result<bool, PaymentError> {
        if self.status == PaymentStatus::Authorized {
            return Ok(false);
        }
        let reference = optional_text("reference", reference)?;
        self.move_to(PaymentStatus::Authorized)?;
        if reference.is_some() {
            self.reference = reference;
        }
        let authorized = PaymentAuthorized {
            payment: self.id,
            order: self.order,
            method: self.method,
            provider: self.provider.to_string(),
            amount: self.amount,
            reference: self.reference.clone(),
        };
        self.events.record(self.subject(), &authorized);
        Ok(true)
    }

    /// The provider declined it (`payment.failed`), with `reference` when given.
    /// Returns whether that changed anything: a failed payment stays failed. Refused
    /// unless pending or authorized.
    pub fn fail(&mut self, reference: Option<String>) -> Result<bool, PaymentError> {
        if self.status == PaymentStatus::Failed {
            return Ok(false);
        }
        let reference = optional_text("reference", reference)?;
        self.move_to(PaymentStatus::Failed)?;
        if reference.is_some() {
            self.reference = reference;
        }
        let failed = PaymentFailed {
            payment: self.id,
            order: self.order,
            method: self.method,
            provider: self.provider.to_string(),
            amount: self.amount,
            reference: self.reference.clone(),
        };
        self.events.record(self.subject(), &failed);
        Ok(true)
    }

    /// Records `amount` given back (`payment.refunded`), or everything left when
    /// `None`, leaving it `reported`: `refunded` must give back all that's left,
    /// `partially_refunded` less (`invalid_refund_amount` otherwise, as for a refund of
    /// nothing or of more than is left). Refused unless paid or partially refunded.
    ///
    /// `refund` is the provider's ID for this refund (not the payment's: `reference`
    /// stays what it was). One counted already changes nothing (`Ok(false)`), whatever
    /// the payment's status now: a provider's webhook delivered twice, or reported
    /// again under another idempotency key, must not give the money back twice.
    pub fn refund(
        &mut self,
        amount: Option<Money>,
        reported: PaymentStatus,
        refund: Option<String>,
    ) -> Result<bool, PaymentError> {
        let refund = optional_text("refund reference", refund)?;
        if let Some(refund) = &refund
            && self.refund_references.contains(refund)
        {
            return Ok(false);
        }
        if !matches!(
            reported,
            PaymentStatus::Refunded | PaymentStatus::PartiallyRefunded
        ) || !self.status.can_become(reported)
        {
            return Err(PaymentError::InvalidTransition {
                payment: self.id,
                from: self.status,
                to: reported,
            });
        }
        let currency = self.amount.currency();
        let left = self.amount.amount() - self.refunded.amount();
        let amount = match amount {
            Some(given) if given.currency() != currency => {
                return Err(PaymentError::CurrencyMismatch {
                    expected: currency,
                    found: given.currency(),
                });
            }
            Some(given) => given.amount(),
            None => left,
        };
        let after = self.refunded.amount() + amount;
        let fits = amount > 0
            && amount <= left
            && match reported {
                PaymentStatus::Refunded => after == self.amount.amount(),
                _ => after < self.amount.amount(),
            };
        if !fits {
            return Err(PaymentError::InvalidRefund {
                payment: self.id,
                amount,
                paid: self.amount.amount(),
                refunded: self.refunded.amount(),
                reported,
            });
        }
        let given = Money::new(amount, currency)?;
        self.refunded = Money::new(after, currency)?;
        self.status = reported;
        if let Some(refund) = &refund {
            self.refund_references.push(refund.clone());
        }
        let refunded = PaymentRefunded {
            payment: self.id,
            order: self.order,
            method: self.method,
            provider: self.provider.to_string(),
            amount: given,
            refunded: self.refunded,
            status: self.status.as_str(),
            reference: self.reference.clone(),
            refund,
        };
        self.events.record(self.subject(), &refunded);
        Ok(true)
    }

    /// Keeps the provider's ID for the payment (what its start answered); a blank one
    /// keeps the current. Returns whether that changed anything. Records nothing: the
    /// payment's own events carry it from then on.
    pub fn set_reference(&mut self, reference: Option<String>) -> Result<bool, PaymentError> {
        let reference = optional_text("reference", reference)?;
        if reference.is_none() || reference == self.reference {
            return Ok(false);
        }
        self.reference = reference;
        Ok(true)
    }

    pub fn id(&self) -> PaymentId {
        self.id
    }

    pub fn order(&self) -> OrderId {
        self.order
    }

    pub fn method(&self) -> Option<PaymentMethodId> {
        self.method
    }

    pub fn method_name(&self) -> Option<&str> {
        self.method_name.as_deref()
    }

    pub fn provider(&self) -> &ProviderKey {
        &self.provider
    }

    pub fn amount(&self) -> Money {
        self.amount
    }

    pub fn status(&self) -> PaymentStatus {
        self.status
    }

    pub fn refunded(&self) -> Money {
        self.refunded
    }

    pub fn reference(&self) -> Option<&str> {
        self.reference.as_deref()
    }

    /// The provider's IDs of the refunds counted so far, oldest first.
    pub fn refund_references(&self) -> &[String] {
        &self.refund_references
    }

    pub fn attempt(&self) -> Option<&str> {
        self.attempt.as_deref()
    }

    /// Keeps `attempt` as the one the payment collects through (a provider's report named
    /// it); a blank one keeps the current. Records nothing.
    pub fn note_attempt(&mut self, attempt: Option<String>) -> Result<(), PaymentError> {
        if let Some(attempt) = optional_text("attempt", attempt)? {
            self.attempt = Some(attempt);
        }
        Ok(())
    }

    pub fn instructions(&self) -> Option<&str> {
        self.instructions.as_deref()
    }

    pub fn paid_at(&self) -> Option<Timestamp> {
        self.paid_at
    }

    pub fn voided_at(&self) -> Option<Timestamp> {
        self.voided_at
    }
}

/// Plain-data form of a `Payment` for storage adapters (design note 0001, 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentSnapshot {
    pub id: PaymentId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub order: OrderId,
    pub method: Option<PaymentMethodId>,
    pub method_name: Option<String>,
    pub provider: ProviderKey,
    pub amount: Money,
    pub status: PaymentStatus,
    pub refunded: Money,
    pub reference: Option<String>,
    pub refund_references: Vec<String>,
    pub attempt: Option<String>,
    pub instructions: Option<String>,
    pub paid_at: Option<Timestamp>,
    pub voided_at: Option<Timestamp>,
}

impl Payment {
    pub fn snapshot(&self) -> PaymentSnapshot {
        PaymentSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            order: self.order,
            method: self.method,
            method_name: self.method_name.clone(),
            provider: self.provider.clone(),
            amount: self.amount,
            status: self.status,
            refunded: self.refunded,
            reference: self.reference.clone(),
            refund_references: self.refund_references.clone(),
            attempt: self.attempt.clone(),
            instructions: self.instructions.clone(),
            paid_at: self.paid_at,
            voided_at: self.voided_at,
        }
    }

    /// Rebuilds a payment from storage (and builds new ones, through `new`), checking
    /// every invariant.
    pub fn restore(s: PaymentSnapshot) -> Result<Self, PaymentError> {
        let inconsistent = |reason| PaymentError::Inconsistent {
            payment: s.id,
            reason,
        };
        if s.refunded.currency() != s.amount.currency() {
            return Err(PaymentError::CurrencyMismatch {
                expected: s.amount.currency(),
                found: s.refunded.currency(),
            });
        }
        if s.amount.amount() < 0 {
            return Err(inconsistent("a negative amount"));
        }
        let refunded = s.refunded.amount();
        let refunds_fit = match s.status {
            PaymentStatus::Refunded => refunded == s.amount.amount(),
            PaymentStatus::PartiallyRefunded => refunded > 0 && refunded < s.amount.amount(),
            _ => refunded == 0,
        };
        if !refunds_fit {
            return Err(inconsistent("refunds don't match the status"));
        }
        if s.paid_at.is_some() != s.status.captured() {
            return Err(inconsistent("paid_at doesn't match the status"));
        }
        if s.voided_at.is_some() != (s.status == PaymentStatus::Voided) {
            return Err(inconsistent("voided_at doesn't match the status"));
        }
        let instructions = instructions_text(s.instructions)?;
        let mut refund_references = Vec::with_capacity(s.refund_references.len());
        for refund in s.refund_references {
            let refund = optional_text("refund reference", Some(refund))?
                .ok_or(inconsistent("a blank refund reference"))?;
            if refund_references.contains(&refund) {
                return Err(inconsistent("a refund reference counted twice"));
            }
            refund_references.push(refund);
        }
        if !s.status.captured() && !refund_references.is_empty() {
            return Err(inconsistent("refund references without a refund"));
        }
        Ok(Self {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            order: s.order,
            method: s.method,
            method_name: optional_text("method_name", s.method_name)?,
            provider: s.provider,
            amount: s.amount,
            status: s.status,
            refunded: s.refunded,
            reference: optional_text("reference", s.reference)?,
            refund_references,
            attempt: optional_text("attempt", s.attempt)?,
            instructions,
            paid_at: s.paid_at,
            voided_at: s.voided_at,
            events: Recorder::default(),
        })
    }
}

/// The payment status an order's `payments` add up to against its `total` (design note
/// 0027, §2). Failed and voided payments count for nothing; an order whose every
/// payment was voided is `voided`. Then refunds decide (`refunded` once everything
/// collected was given back, else `partially_refunded`), then collected money (`paid`
/// once it covers the total, `partially_paid` below it), then authorizations
/// (`authorized` once they cover it); else `pending`. No payments at all is `pending`.
pub fn financial_status(
    total: Money,
    payments: &[Payment],
) -> Result<FinancialStatus, PaymentError> {
    let currency = total.currency();
    if let Some(other) = payments.iter().find(|p| p.amount.currency() != currency) {
        return Err(PaymentError::CurrencyMismatch {
            expected: currency,
            found: other.amount.currency(),
        });
    }
    let counted: Vec<&Payment> = payments
        .iter()
        .filter(|p| !matches!(p.status, PaymentStatus::Failed | PaymentStatus::Voided))
        .collect();
    if counted.is_empty() {
        let voided = payments.iter().any(|p| p.status == PaymentStatus::Voided);
        return Ok(if voided {
            FinancialStatus::Voided
        } else {
            FinancialStatus::Pending
        });
    }
    let sum = |amounts: Vec<Money>| Money::sum(&amounts, currency);
    let captured: Vec<&&Payment> = counted.iter().filter(|p| p.status.captured()).collect();
    let collected = sum(captured.iter().map(|p| p.amount).collect())?;
    let refunded = sum(captured.iter().map(|p| p.refunded).collect())?;
    if refunded.amount() > 0 {
        return Ok(if refunded.amount() >= collected.amount() {
            FinancialStatus::Refunded
        } else {
            FinancialStatus::PartiallyRefunded
        });
    }
    if !captured.is_empty() {
        return Ok(if collected.amount() >= total.amount() {
            FinancialStatus::Paid
        } else {
            FinancialStatus::PartiallyPaid
        });
    }
    let held: Vec<Money> = counted
        .iter()
        .filter(|p| p.status == PaymentStatus::Authorized)
        .map(|p| p.amount)
        .collect();
    if !held.is_empty() && sum(held)?.amount() >= total.amount() {
        return Ok(FinancialStatus::Authorized);
    }
    Ok(FinancialStatus::Pending)
}

#[cfg(test)]
mod tests {
    use hypha_core::{Currency, TextError};
    use uuid::Uuid;

    use super::*;

    fn eur(amount: i64) -> Money {
        Money::new(amount, Currency::EUR).unwrap()
    }

    fn at() -> Timestamp {
        Timestamp::from_unix_micros(1_767_225_600_000_000)
    }

    fn new_payment(n: u128, amount: i64) -> Payment {
        let mut p = Payment::new(NewPayment {
            id: PaymentId::from_uuid(Uuid::from_u128(n)),
            order: OrderId::from_uuid(Uuid::from_u128(100)),
            method: Some(PaymentMethodId::from_uuid(Uuid::from_u128(200))),
            method_name: Some("Bank transfer".to_string()),
            provider: ProviderKey::manual(),
            amount: eur(amount),
            instructions: Some(" IBAN HR12 ".to_string()),
        })
        .unwrap();
        p.take_events();
        p
    }

    fn with_status(n: u128, amount: i64, status: PaymentStatus, refunded: i64) -> Payment {
        let mut s = new_payment(n, amount).snapshot();
        s.status = status;
        s.refunded = eur(refunded);
        s.paid_at = status.captured().then(at);
        s.voided_at = (status == PaymentStatus::Voided).then(at);
        Payment::restore(s).unwrap()
    }

    fn types(p: &mut Payment) -> Vec<&'static str> {
        p.take_events().iter().map(|e| e.event_type).collect()
    }

    #[test]
    fn a_new_payment_is_pending_and_recorded() {
        let mut p = Payment::new(NewPayment::recorded_by_hand(
            PaymentId::from_uuid(Uuid::from_u128(1)),
            OrderId::from_uuid(Uuid::from_u128(2)),
            eur(1000),
        ))
        .unwrap();
        assert_eq!(p.status(), PaymentStatus::Pending);
        assert_eq!(p.refunded(), eur(0));
        let events = p.take_events();
        assert_eq!(events[0].event_type, "payment.created");
        assert_eq!(events[0].data["provider"], "manual");
        assert_eq!(events[0].data["method"], serde_json::Value::Null);
    }

    #[test]
    fn instructions_are_copied_trimmed() {
        assert_eq!(new_payment(1, 10).instructions(), Some("IBAN HR12"));
    }

    #[test]
    fn paying_records_it_once_with_the_reference() {
        let mut p = new_payment(1, 1000);
        assert_eq!(p.pay(at(), Some(" RF-123 ".to_string())), Ok(true));
        assert_eq!(p.status(), PaymentStatus::Paid);
        assert_eq!((p.paid_at(), p.reference()), (Some(at()), Some("RF-123")));
        let events = p.take_events();
        assert_eq!(events[0].event_type, "payment.paid");
        assert_eq!(events[0].data["reference"], "RF-123");
        assert_eq!(
            p.pay(Timestamp::UNIX_EPOCH, Some("other".to_string())),
            Ok(false)
        );
        assert_eq!((p.paid_at(), p.reference()), (Some(at()), Some("RF-123")));
        assert_eq!(types(&mut p), Vec::<&str>::new());
    }

    #[test]
    fn a_reference_is_short_text() {
        let mut p = new_payment(1, 1000);
        assert_eq!(
            p.pay(at(), Some("x".repeat(256))),
            Err(PaymentError::Text(TextError::TooLong("reference")))
        );
        assert_eq!(p.status(), PaymentStatus::Pending);
    }

    #[test]
    fn voiding_records_it_once() {
        let mut p = new_payment(1, 1000);
        assert_eq!(p.void(at()), Ok(true));
        assert_eq!(
            (p.status(), p.voided_at()),
            (PaymentStatus::Voided, Some(at()))
        );
        assert_eq!(types(&mut p), ["payment.voided"]);
        assert_eq!(p.void(Timestamp::UNIX_EPOCH), Ok(false));
        assert_eq!(p.voided_at(), Some(at()));
    }

    #[test]
    fn a_voided_payment_cant_be_paid_nor_a_paid_one_voided() {
        let mut voided = new_payment(1, 1000);
        voided.void(at()).unwrap();
        assert_eq!(
            voided.pay(at(), None),
            Err(PaymentError::InvalidTransition {
                payment: voided.id(),
                from: PaymentStatus::Voided,
                to: PaymentStatus::Paid,
            })
        );
        let mut paid = new_payment(2, 1000);
        paid.pay(at(), None).unwrap();
        assert_eq!(
            paid.void(at()),
            Err(PaymentError::InvalidTransition {
                payment: paid.id(),
                from: PaymentStatus::Paid,
                to: PaymentStatus::Voided,
            })
        );
    }

    #[test]
    fn the_allowed_moves() {
        use PaymentStatus as S;
        let allowed = [
            (S::Pending, S::Authorized),
            (S::Pending, S::Paid),
            (S::Pending, S::Failed),
            (S::Pending, S::Voided),
            (S::Authorized, S::Paid),
            (S::Authorized, S::Failed),
            (S::Authorized, S::Voided),
            (S::Paid, S::PartiallyRefunded),
            (S::Paid, S::Refunded),
            (S::PartiallyRefunded, S::PartiallyRefunded),
            (S::PartiallyRefunded, S::Refunded),
        ];
        for from in S::ALL {
            for to in S::ALL {
                assert_eq!(
                    from.can_become(to),
                    allowed.contains(&(from, to)),
                    "{from} -> {to}"
                );
            }
        }
    }

    #[test]
    fn authorized_then_paid_or_failed() {
        let mut p = new_payment(1, 1000);
        p.authorize(Some("pi_1".to_string())).unwrap();
        assert_eq!(p.status(), PaymentStatus::Authorized);
        assert_eq!(p.pay(at(), None), Ok(true));
        assert_eq!(p.reference(), Some("pi_1"));
        let mut declined = new_payment(2, 1000);
        assert_eq!(declined.fail(None), Ok(true));
        assert_eq!(declined.fail(None), Ok(false));
        assert_eq!(declined.status(), PaymentStatus::Failed);
    }

    #[test]
    fn authorizing_records_it_once() {
        let mut p = new_payment(1, 1000);
        types(&mut p);
        assert_eq!(p.authorize(Some("pi_1".to_string())), Ok(true));
        assert_eq!(p.authorize(None), Ok(false));
        assert_eq!(types(&mut p), ["payment.authorized"]);
    }

    fn paid(n: u128, amount: i64) -> Payment {
        let mut p = new_payment(n, amount);
        p.pay(at(), None).unwrap();
        types(&mut p);
        p
    }

    #[test]
    fn refunds_add_up_to_the_status_reported() {
        let mut p = paid(1, 1000);
        assert_eq!(
            p.refund(Some(eur(300)), PaymentStatus::PartiallyRefunded, None),
            Ok(true)
        );
        assert_eq!(
            (p.status(), p.refunded()),
            (PaymentStatus::PartiallyRefunded, eur(300))
        );
        // The rest, without an amount.
        assert_eq!(p.refund(None, PaymentStatus::Refunded, None), Ok(true));
        assert_eq!(
            (p.status(), p.refunded()),
            (PaymentStatus::Refunded, eur(1000))
        );
        assert_eq!(types(&mut p), ["payment.refunded", "payment.refunded"]);
        assert!(Payment::restore(p.snapshot()).is_ok());
    }

    #[test]
    fn a_refund_counted_under_its_reference_counts_once() {
        let mut p = paid(1, 1000);
        let partly = PaymentStatus::PartiallyRefunded;
        let re = |id: &str| Some(id.to_string());
        assert_eq!(p.refund(Some(eur(300)), partly, re("re_1")), Ok(true));
        // The same refund again: nothing, even with another amount or status.
        assert_eq!(p.refund(Some(eur(300)), partly, re("re_1")), Ok(false));
        assert_eq!(
            p.refund(None, PaymentStatus::Refunded, re("re_1")),
            Ok(false)
        );
        assert_eq!(p.refunded(), eur(300));
        assert_eq!(
            p.refund(Some(eur(700)), PaymentStatus::Refunded, re("re_2")),
            Ok(true)
        );
        // Refunded wholly, an earlier refund reported again is still nothing.
        assert_eq!(p.refund(Some(eur(300)), partly, re("re_1")), Ok(false));
        assert_eq!(p.refund_references(), ["re_1", "re_2"]);
        assert_eq!(p.reference(), None, "the payment's own reference stays");
        assert_eq!(types(&mut p), ["payment.refunded", "payment.refunded"]);
        assert_eq!(Payment::restore(p.snapshot()), Ok(p));
    }

    #[test]
    fn a_refund_reference_counted_twice_or_on_an_uncaptured_payment_doesnt_restore() {
        let mut twice = paid(1, 1000).snapshot();
        twice.status = PaymentStatus::PartiallyRefunded;
        twice.refunded = eur(300);
        twice.refund_references = vec!["re_1".to_string(), "re_1".to_string()];
        assert!(Payment::restore(twice).is_err());
        let mut pending = new_payment(2, 1000).snapshot();
        pending.refund_references = vec!["re_1".to_string()];
        assert!(Payment::restore(pending).is_err());
    }

    #[test]
    fn a_refund_must_fit_what_was_reported() {
        let refused = |amount: i64, reported| {
            let mut p = paid(1, 1000);
            p.refund(Some(eur(amount)), reported, None)
        };
        let invalid = |amount, reported| {
            Err(PaymentError::InvalidRefund {
                payment: PaymentId::from_uuid(Uuid::from_u128(1)),
                amount,
                paid: 1000,
                refunded: 0,
                reported,
            })
        };
        use PaymentStatus::{PartiallyRefunded as Partly, Refunded as All};
        assert_eq!(refused(0, Partly), invalid(0, Partly));
        assert_eq!(refused(1000, Partly), invalid(1000, Partly));
        assert_eq!(refused(999, All), invalid(999, All));
        assert_eq!(refused(1001, All), invalid(1001, All));
        assert_eq!(refused(1000, All), Ok(true));
        let mut pending = new_payment(2, 1000);
        assert_eq!(
            pending.refund(None, All, None),
            Err(PaymentError::InvalidTransition {
                payment: PaymentId::from_uuid(Uuid::from_u128(2)),
                from: PaymentStatus::Pending,
                to: All,
            })
        );
    }

    #[test]
    fn a_reference_is_kept_unless_blank() {
        let mut p = new_payment(1, 1000);
        assert_eq!(p.set_reference(Some("pi_1".to_string())), Ok(true));
        assert_eq!(p.set_reference(Some(" ".to_string())), Ok(false));
        assert_eq!(p.set_reference(Some("pi_1".to_string())), Ok(false));
        assert_eq!(p.reference(), Some("pi_1"));
    }

    #[test]
    fn statuses_parse_back_from_their_strings() {
        for status in PaymentStatus::ALL {
            assert_eq!(status.as_str().parse(), Ok(status));
        }
        assert_eq!("lost".parse::<PaymentStatus>(), Err("lost".to_string()));
    }

    #[test]
    fn restore_round_trips_every_status() {
        for (status, refunded) in [
            (PaymentStatus::Pending, 0),
            (PaymentStatus::Authorized, 0),
            (PaymentStatus::Paid, 0),
            (PaymentStatus::Failed, 0),
            (PaymentStatus::Voided, 0),
            (PaymentStatus::PartiallyRefunded, 400),
            (PaymentStatus::Refunded, 1000),
        ] {
            let p = with_status(1, 1000, status, refunded);
            assert_eq!(Payment::restore(p.snapshot()), Ok(p), "{status}");
        }
    }

    #[test]
    fn restore_rejects_inconsistent_data() {
        let id = PaymentId::from_uuid(Uuid::from_u128(1));
        let base = new_payment(1, 1000).snapshot();
        let reject = |s: PaymentSnapshot, reason| {
            assert_eq!(
                Payment::restore(s),
                Err(PaymentError::Inconsistent {
                    payment: id,
                    reason
                })
            );
        };
        let mut negative = base.clone();
        negative.amount = eur(-1);
        negative.refunded = eur(0);
        reject(negative, "a negative amount");
        let mut refunded_while_pending = base.clone();
        refunded_while_pending.refunded = eur(1);
        reject(refunded_while_pending, "refunds don't match the status");
        let mut paid_without_time = base.clone();
        paid_without_time.status = PaymentStatus::Paid;
        reject(paid_without_time, "paid_at doesn't match the status");
        let mut voided_without_time = base.clone();
        voided_without_time.status = PaymentStatus::Voided;
        reject(voided_without_time, "voided_at doesn't match the status");
        let mut other_currency = base;
        other_currency.refunded = Money::zero(Currency::USD);
        assert_eq!(
            Payment::restore(other_currency),
            Err(PaymentError::CurrencyMismatch {
                expected: Currency::EUR,
                found: Currency::USD
            })
        );
    }

    fn status_of(total: i64, payments: &[Payment]) -> FinancialStatus {
        financial_status(eur(total), payments).unwrap()
    }

    #[test]
    fn no_payments_is_pending() {
        assert_eq!(status_of(1000, &[]), FinancialStatus::Pending);
        assert_eq!(
            status_of(1000, &[new_payment(1, 1000)]),
            FinancialStatus::Pending
        );
    }

    #[test]
    fn collected_money_against_the_total() {
        use PaymentStatus as S;
        assert_eq!(
            status_of(1000, &[with_status(1, 1000, S::Paid, 0)]),
            FinancialStatus::Paid
        );
        assert_eq!(
            status_of(1000, &[with_status(1, 999, S::Paid, 0)]),
            FinancialStatus::PartiallyPaid
        );
        assert_eq!(
            status_of(
                1000,
                &[
                    with_status(1, 400, S::Paid, 0),
                    with_status(2, 600, S::Paid, 0)
                ]
            ),
            FinancialStatus::Paid
        );
        // Nothing to pay, and a zero payment recorded: paid.
        assert_eq!(
            status_of(0, &[with_status(1, 0, S::Paid, 0)]),
            FinancialStatus::Paid
        );
    }

    #[test]
    fn refunds_decide_once_there_are_any() {
        use PaymentStatus as S;
        assert_eq!(
            status_of(1000, &[with_status(1, 1000, S::PartiallyRefunded, 1)]),
            FinancialStatus::PartiallyRefunded
        );
        assert_eq!(
            status_of(1000, &[with_status(1, 1000, S::Refunded, 1000)]),
            FinancialStatus::Refunded
        );
    }

    #[test]
    fn authorizations_count_once_they_cover_the_total() {
        use PaymentStatus as S;
        assert_eq!(
            status_of(1000, &[with_status(1, 1000, S::Authorized, 0)]),
            FinancialStatus::Authorized
        );
        assert_eq!(
            status_of(1000, &[with_status(1, 999, S::Authorized, 0)]),
            FinancialStatus::Pending
        );
    }

    #[test]
    fn voided_and_failed_payments_count_for_nothing() {
        use PaymentStatus as S;
        assert_eq!(
            status_of(1000, &[with_status(1, 1000, S::Voided, 0)]),
            FinancialStatus::Voided
        );
        assert_eq!(
            status_of(1000, &[with_status(1, 1000, S::Failed, 0)]),
            FinancialStatus::Pending
        );
        assert_eq!(
            status_of(
                1000,
                &[
                    with_status(1, 1000, S::Voided, 0),
                    with_status(2, 1000, S::Paid, 0)
                ]
            ),
            FinancialStatus::Paid
        );
    }

    #[test]
    fn payments_in_another_currency_are_refused() {
        let usd = Money::new(1000, Currency::USD).unwrap();
        assert_eq!(
            financial_status(usd, &[new_payment(1, 1000)]),
            Err(PaymentError::CurrencyMismatch {
                expected: Currency::USD,
                found: Currency::EUR
            })
        );
    }
}
