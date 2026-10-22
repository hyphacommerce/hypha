//! The record of one email: what went to whom, whether it went, and why not.
//!
//! A message about an event is rendered and recorded `queued` by the `Mailer` handler, with
//! what the send job needs to send it (`QueuedEmail`: the rendered bodies and the sender's
//! name); the job leases it (`sending`, with `lease_until`), sends it with no connection
//! held, and records the outcome: `sent`, back to `queued` for a later attempt, or `failed`
//! for good. The bodies go once it's sent or has failed: they hold addresses, order
//! contents and order links, so they're kept only while they're needed to send. Direct
//! sends (staff invites, resets) are `sending` without a lease or bodies while the request
//! sends them itself.

use std::{fmt, str::FromStr};

use hypha_core::{
    Email, EventId, NotificationId, OrderId, Stamped, Stamps, Timestamp, Version, Versioned,
    optional_text,
};

use crate::{NotificationError, TemplateKey};

/// Where a message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationStatus {
    /// Rendered and waiting for the send job, from its `next_attempt_at`.
    Queued,
    /// Handed to the sender: leased by the send job until `lease_until` (past it, a
    /// later lease takes it again: the runner holding it stopped), or being sent by a
    /// request (staff invites and resets, no lease).
    Sending,
    /// The provider accepted it.
    Sent,
    /// The provider refused it, or every attempt the schedule allows failed: final.
    Failed,
    /// Not sent on purpose (the template is turned off), so the order's emails can say
    /// why nothing went.
    Skipped,
}

impl NotificationStatus {
    pub const ALL: [NotificationStatus; 5] = [
        NotificationStatus::Queued,
        NotificationStatus::Sending,
        NotificationStatus::Sent,
        NotificationStatus::Failed,
        NotificationStatus::Skipped,
    ];

    /// The same string used in JSON and storage.
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationStatus::Queued => "queued",
            NotificationStatus::Sending => "sending",
            NotificationStatus::Sent => "sent",
            NotificationStatus::Failed => "failed",
            NotificationStatus::Skipped => "skipped",
        }
    }

    /// The moves a message may make: `queued` → `sending` (leased); `sending` → `sent`,
    /// `failed`, back to `queued` (a transient failure, tried again later) or `sending`
    /// again (its lease ran out: a new lease). `sent`, `failed` and `skipped` are final:
    /// resending a message makes a new one.
    pub fn can_become(self, to: NotificationStatus) -> bool {
        use NotificationStatus as S;
        matches!(
            (self, to),
            (S::Queued, S::Sending) | (S::Sending, S::Sent | S::Failed | S::Queued | S::Sending)
        )
    }

    /// Whether nothing more happens to it.
    pub fn is_final(self) -> bool {
        matches!(
            self,
            NotificationStatus::Sent | NotificationStatus::Failed | NotificationStatus::Skipped
        )
    }
}

impl fmt::Display for NotificationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NotificationStatus {
    /// The text that didn't match a status.
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        NotificationStatus::ALL
            .into_iter()
            .find(|status| status.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

/// What a new message's record is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNotification {
    pub id: NotificationId,
    pub template: TemplateKey,
    pub to: Email,
    /// The rendered subject line (empty for one skipped without rendering).
    pub subject: String,
    /// The event it answers; `None` for direct sends (invites, resets) and resends.
    pub event: Option<EventId>,
    /// The order it's about, for the order's emails.
    pub order: Option<OrderId>,
}

/// A queued message's rendered email, kept until it's sent or has failed for good: the
/// send job builds the `OutgoingEmail` from it (the from address is the operator's, the
/// host's at send time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedEmail {
    /// The merchant's sender name (or the store's) when it was rendered.
    pub sender_name: Option<String>,
    pub reply_to: Option<Email>,
    pub html: String,
    pub text: String,
}

/// One email's record. One per (event, template, recipient) where `event` is set, so a
/// handler that runs twice finds it and queues nothing more.
///
/// Invariants, checked by `restore`: `sent_at` set exactly when sent, a provider's
/// message ID only on a sent message, `next_attempt_at` exactly when queued,
/// `lease_until` only while sending, the queued email exactly while it's waiting or
/// leased, text within its length (longer subjects, errors and notes are cut, not
/// refused: a record should never fail over what a provider said).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    id: NotificationId,
    version: Version,
    stamps: Option<Stamps>,
    template: TemplateKey,
    to: Email,
    subject: String,
    status: NotificationStatus,
    /// How many times it was handed to a sender.
    attempts: u32,
    /// The sender's name (`log`, `smtp`, `resend`) on the last attempt.
    provider: Option<String>,
    provider_message_id: Option<String>,
    /// The last attempt's error, or why it was skipped.
    error: Option<String>,
    /// Something staff should know about how it was made: "sent with the default template:
    /// …" when the merchant's didn't render.
    note: Option<String>,
    event: Option<EventId>,
    order: Option<OrderId>,
    sent_at: Option<Timestamp>,
    /// When a queued message is next due.
    next_attempt_at: Option<Timestamp>,
    /// Until when the send job holds it.
    lease_until: Option<Timestamp>,
    email: Option<QueuedEmail>,
}

impl Versioned for Notification {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Notification {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Notification {
    /// Characters: more than any real subject line (RFC 5322 lines stop at 998).
    pub const MAX_SUBJECT_LEN: usize = 998;
    /// Characters: enough for a provider's explanation, or a template's error.
    pub const MAX_ERROR_LEN: usize = 2000;

    /// A message waiting for the send job from `at` (now, usually), with its rendered
    /// `email`. Not attempted yet.
    pub fn queued(new: NewNotification, email: QueuedEmail, at: Timestamp) -> Self {
        let mut n = Self::from_new(new, NotificationStatus::Queued, 0, None);
        n.next_attempt_at = Some(at);
        n.email = Some(email);
        n
    }

    /// A message a request sends itself, right away (staff invites and resets): `sending`
    /// without a lease, its first attempt counted.
    pub fn sending(new: NewNotification) -> Self {
        Self::from_new(new, NotificationStatus::Sending, 1, None)
    }

    /// A message not sent on purpose, with why (`reason`, e.g. "the template is turned
    /// off").
    pub fn skipped(new: NewNotification, reason: &str) -> Self {
        Self::from_new(
            new,
            NotificationStatus::Skipped,
            0,
            Some(cut(reason, Self::MAX_ERROR_LEN)),
        )
    }

    /// With `note` kept on the record (cut to `MAX_ERROR_LEN`).
    pub fn with_note(mut self, note: Option<&str>) -> Self {
        self.note = note.map(|n| cut(n.trim(), Self::MAX_ERROR_LEN));
        self
    }

    fn from_new(
        new: NewNotification,
        status: NotificationStatus,
        attempts: u32,
        error: Option<String>,
    ) -> Self {
        Self {
            id: new.id,
            version: Version::NEW,
            stamps: None,
            template: new.template,
            to: new.to,
            subject: cut(new.subject.trim(), Self::MAX_SUBJECT_LEN),
            status,
            attempts,
            provider: None,
            provider_message_id: None,
            error,
            note: None,
            event: new.event,
            order: new.order,
            sent_at: None,
            next_attempt_at: None,
            lease_until: None,
            email: None,
        }
    }

    fn move_to(&mut self, to: NotificationStatus) -> Result<(), NotificationError> {
        if !self.status.can_become(to) {
            return Err(NotificationError::InvalidTransition {
                notification: self.id,
                from: self.status,
                to,
            });
        }
        self.status = to;
        Ok(())
    }

    /// Whether the send job may lease it at `now`: queued and due, or leased by a runner
    /// whose lease has run out (it stopped before recording; this is the only case a
    /// provider may get a message twice).
    pub fn is_due(&self, now: Timestamp) -> bool {
        match self.status {
            NotificationStatus::Queued => self.next_attempt_at.is_some_and(|at| at <= now),
            NotificationStatus::Sending => self.lease_until.is_some_and(|until| until <= now),
            _ => false,
        }
    }

    /// The send job takes it at `now` until `until`: `sending`, its attempt counted.
    /// `NotDue` unless `is_due(now)`.
    pub fn lease(&mut self, now: Timestamp, until: Timestamp) -> Result<(), NotificationError> {
        if !self.is_due(now) || self.email.is_none() {
            return Err(NotificationError::NotDue(self.id));
        }
        self.move_to(NotificationStatus::Sending)?;
        self.attempts = self.attempts.saturating_add(1);
        self.next_attempt_at = None;
        self.lease_until = Some(until);
        Ok(())
    }

    /// The provider (`provider`, the sender's name) accepted it at `at`, under its own
    /// `message_id` when it gives one. Clears the last attempt's error, the lease and the
    /// bodies.
    pub fn sent(
        &mut self,
        provider: &str,
        message_id: Option<String>,
        at: Timestamp,
    ) -> Result<(), NotificationError> {
        let provider = optional_text("provider", Some(provider.to_string()))?;
        let message_id = optional_text("provider message ID", message_id)?;
        self.move_to(NotificationStatus::Sent)?;
        self.provider = provider;
        self.provider_message_id = message_id;
        self.error = None;
        self.sent_at = Some(at);
        self.finish();
        Ok(())
    }

    /// The attempt through `provider` failed with `error`, for good (refused, or the
    /// last attempt the schedule allows).
    pub fn failed(&mut self, provider: &str, error: &str) -> Result<(), NotificationError> {
        let provider = optional_text("provider", Some(provider.to_string()))?;
        self.move_to(NotificationStatus::Failed)?;
        self.provider = provider;
        self.error = Some(cut(error.trim(), Self::MAX_ERROR_LEN));
        self.finish();
        Ok(())
    }

    /// The attempt through `provider` failed with `error` and may work later: queued
    /// again, due at `next`. Only a leased message (the send job's) is tried again.
    pub fn retry_at(
        &mut self,
        provider: &str,
        error: &str,
        next: Timestamp,
    ) -> Result<(), NotificationError> {
        let provider = optional_text("provider", Some(provider.to_string()))?;
        if self.email.is_none() {
            return Err(NotificationError::InvalidTransition {
                notification: self.id,
                from: self.status,
                to: NotificationStatus::Queued,
            });
        }
        self.move_to(NotificationStatus::Queued)?;
        self.provider = provider;
        self.error = Some(cut(error.trim(), Self::MAX_ERROR_LEN));
        self.lease_until = None;
        self.next_attempt_at = Some(next);
        Ok(())
    }

    /// Done with: nothing to send any more, so the bodies go. Erasure: the recipient
    /// becomes `placeholder`, the subject and the rendered email go, and a message still
    /// waiting is `skipped` (it would go to an address that no longer exists). Says whether
    /// anything changed.
    pub fn redact(&mut self, placeholder: Email) -> bool {
        let mut changed = false;
        if self.to != placeholder {
            self.to = placeholder;
            changed = true;
        }
        if !self.subject.is_empty() {
            self.subject.clear();
            changed = true;
        }
        if matches!(
            self.status,
            NotificationStatus::Queued | NotificationStatus::Sending
        ) {
            self.status = NotificationStatus::Skipped;
            self.error = Some("the customer was erased".to_string());
            changed = true;
        }
        if self.email.is_some() || self.lease_until.is_some() || self.next_attempt_at.is_some() {
            self.finish();
            changed = true;
        }
        changed
    }

    fn finish(&mut self) {
        self.lease_until = None;
        self.next_attempt_at = None;
        self.email = None;
    }

    pub fn id(&self) -> NotificationId {
        self.id
    }

    pub fn template(&self) -> TemplateKey {
        self.template
    }

    pub fn to(&self) -> &Email {
        &self.to
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn status(&self) -> NotificationStatus {
        self.status
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    pub fn provider_message_id(&self) -> Option<&str> {
        self.provider_message_id.as_deref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn event(&self) -> Option<EventId> {
        self.event
    }

    pub fn order(&self) -> Option<OrderId> {
        self.order
    }

    pub fn sent_at(&self) -> Option<Timestamp> {
        self.sent_at
    }

    pub fn next_attempt_at(&self) -> Option<Timestamp> {
        self.next_attempt_at
    }

    pub fn lease_until(&self) -> Option<Timestamp> {
        self.lease_until
    }

    /// The rendered email, while it's waiting to be sent.
    pub fn email(&self) -> Option<&QueuedEmail> {
        self.email.as_ref()
    }
}

/// `text` cut to at most `max` characters.
fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => text[..end].to_string(),
        None => text.to_string(),
    }
}

/// Plain-data form of a `Notification` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationSnapshot {
    pub id: NotificationId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub template: TemplateKey,
    pub to: Email,
    pub subject: String,
    pub status: NotificationStatus,
    pub attempts: u32,
    pub provider: Option<String>,
    pub provider_message_id: Option<String>,
    pub error: Option<String>,
    pub note: Option<String>,
    pub event: Option<EventId>,
    pub order: Option<OrderId>,
    pub sent_at: Option<Timestamp>,
    pub next_attempt_at: Option<Timestamp>,
    pub lease_until: Option<Timestamp>,
    pub email: Option<QueuedEmail>,
}

impl Notification {
    pub fn snapshot(&self) -> NotificationSnapshot {
        NotificationSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            template: self.template,
            to: self.to.clone(),
            subject: self.subject.clone(),
            status: self.status,
            attempts: self.attempts,
            provider: self.provider.clone(),
            provider_message_id: self.provider_message_id.clone(),
            error: self.error.clone(),
            note: self.note.clone(),
            event: self.event,
            order: self.order,
            sent_at: self.sent_at,
            next_attempt_at: self.next_attempt_at,
            lease_until: self.lease_until,
            email: self.email.clone(),
        }
    }

    /// Rebuilds a record from storage, checking every invariant.
    pub fn restore(s: NotificationSnapshot) -> Result<Self, NotificationError> {
        let inconsistent = |reason| NotificationError::Inconsistent {
            notification: s.id,
            reason,
        };
        let sent = s.status == NotificationStatus::Sent;
        let queued = s.status == NotificationStatus::Queued;
        if sent != s.sent_at.is_some() {
            return Err(inconsistent("sent_at is set exactly when it was sent"));
        }
        if !sent && s.provider_message_id.is_some() {
            return Err(inconsistent("only a sent message has a provider's ID"));
        }
        if queued != s.next_attempt_at.is_some() {
            return Err(inconsistent(
                "next_attempt_at is set exactly when it's queued",
            ));
        }
        if s.lease_until.is_some() && s.status != NotificationStatus::Sending {
            return Err(inconsistent("only a message being sent is leased"));
        }
        if s.email.is_some() != (queued || s.lease_until.is_some()) {
            return Err(inconsistent(
                "the email is kept exactly while it's queued or leased",
            ));
        }
        let too_long = |text: &Option<String>| {
            text.as_ref()
                .is_some_and(|e| e.chars().count() > Self::MAX_ERROR_LEN)
        };
        if s.subject.chars().count() > Self::MAX_SUBJECT_LEN
            || too_long(&s.error)
            || too_long(&s.note)
        {
            return Err(inconsistent("subject, error or note too long"));
        }
        Ok(Self {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            template: s.template,
            to: s.to,
            subject: s.subject,
            status: s.status,
            attempts: s.attempts,
            provider: optional_text("provider", s.provider)?,
            provider_message_id: optional_text("provider message ID", s.provider_message_id)?,
            error: s.error,
            note: s.note,
            event: s.event,
            order: s.order,
            sent_at: s.sent_at,
            next_attempt_at: s.next_attempt_at,
            lease_until: s.lease_until,
            email: s.email,
        })
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn new() -> NewNotification {
        NewNotification {
            id: NotificationId::from_uuid(Uuid::from_u128(1)),
            template: TemplateKey::OrderConfirmation,
            to: Email::new("ana@example.com").unwrap(),
            subject: " Order #1001 confirmed ".to_string(),
            event: Some(EventId::from_uuid(Uuid::from_u128(2))),
            order: Some(OrderId::from_uuid(Uuid::from_u128(3))),
        }
    }

    fn email() -> QueuedEmail {
        QueuedEmail {
            sender_name: Some("Fern & Field".to_string()),
            reply_to: None,
            html: "<p>Thanks</p>".to_string(),
            text: "Thanks".to_string(),
        }
    }

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_unix_micros(1_767_225_600_000_000).plus_secs(secs)
    }

    fn leased() -> Notification {
        let mut n = Notification::queued(new(), email(), at(0));
        n.lease(at(0), at(120)).unwrap();
        n
    }

    #[test]
    fn every_status_parses_back_from_its_string() {
        for status in NotificationStatus::ALL {
            assert_eq!(status.as_str().parse(), Ok(status));
        }
        assert_eq!(
            "Sent".parse::<NotificationStatus>(),
            Err("Sent".to_string())
        );
    }

    #[test]
    fn a_queued_message_waits_with_its_email_and_no_attempt() {
        let n = Notification::queued(new(), email(), at(0));
        assert_eq!(n.status(), NotificationStatus::Queued);
        assert_eq!(n.attempts(), 0);
        assert_eq!(n.next_attempt_at(), Some(at(0)));
        assert_eq!(n.email(), Some(&email()));
        assert_eq!(n.subject(), "Order #1001 confirmed");
    }

    #[test]
    fn a_queued_message_is_due_from_its_next_attempt() {
        let n = Notification::queued(new(), email(), at(10));
        assert!(!n.is_due(at(9)));
        assert!(n.is_due(at(10)));
    }

    #[test]
    fn a_lease_counts_the_attempt_and_holds_it_until_it_ends() {
        let n = leased();
        assert_eq!(n.status(), NotificationStatus::Sending);
        assert_eq!(n.attempts(), 1);
        assert_eq!(n.lease_until(), Some(at(120)));
        assert_eq!(n.next_attempt_at(), None);
        assert!(!n.is_due(at(119)));
        assert!(n.is_due(at(120)));
    }

    #[test]
    fn a_message_not_due_cant_be_leased() {
        let mut early = Notification::queued(new(), email(), at(10));
        assert_eq!(
            early.lease(at(9), at(100)),
            Err(NotificationError::NotDue(early.id()))
        );
        let mut held = leased();
        assert_eq!(
            held.lease(at(60), at(180)),
            Err(NotificationError::NotDue(held.id()))
        );
        // A direct send has no lease to run out: never the job's.
        let mut direct = Notification::sending(new());
        assert!(!direct.is_due(at(1_000_000)));
        assert_eq!(
            direct.lease(at(0), at(100)),
            Err(NotificationError::NotDue(direct.id()))
        );
    }

    #[test]
    fn an_expired_lease_is_leased_again_and_counted() {
        let mut n = leased();
        n.lease(at(120), at(240)).unwrap();
        assert_eq!(n.attempts(), 2);
        assert_eq!(n.lease_until(), Some(at(240)));
    }

    #[test]
    fn sent_records_the_provider_and_time_and_drops_the_email() {
        let mut n = leased();
        n.retry_at("resend", "timed out", at(60)).unwrap();
        n.lease(at(60), at(180)).unwrap();
        assert_eq!(n.attempts(), 2);
        n.sent("resend", Some("re_123".to_string()), at(61))
            .unwrap();
        assert_eq!(n.status(), NotificationStatus::Sent);
        assert_eq!(n.provider(), Some("resend"));
        assert_eq!(n.provider_message_id(), Some("re_123"));
        assert_eq!(n.error(), None);
        assert_eq!(n.sent_at(), Some(at(61)));
        assert_eq!(n.email(), None);
        assert_eq!(n.lease_until(), None);
    }

    #[test]
    fn a_transient_failure_queues_it_again_with_the_error() {
        let mut n = leased();
        n.retry_at("smtp", "421 try later", at(60)).unwrap();
        assert_eq!(n.status(), NotificationStatus::Queued);
        assert_eq!(n.next_attempt_at(), Some(at(60)));
        assert_eq!(n.lease_until(), None);
        assert_eq!(n.error(), Some("421 try later"));
        assert_eq!(n.email(), Some(&email()));
    }

    #[test]
    fn a_direct_send_isnt_queued_again() {
        let mut n = Notification::sending(new());
        assert_eq!(
            n.retry_at("smtp", "421", at(60)),
            Err(NotificationError::InvalidTransition {
                notification: n.id(),
                from: NotificationStatus::Sending,
                to: NotificationStatus::Queued,
            })
        );
    }

    #[test]
    fn sent_failed_and_skipped_are_final() {
        let mut sent = leased();
        sent.sent("log", None, at(1)).unwrap();
        let mut failed = leased();
        failed.failed("smtp", "550 refused").unwrap();
        assert_eq!(failed.email(), None);
        for n in [&mut sent, &mut failed] {
            assert!(n.status().is_final());
            assert_eq!(
                n.lease(at(1_000), at(2_000)),
                Err(NotificationError::NotDue(n.id()))
            );
            let from = n.status();
            assert_eq!(
                n.failed("smtp", "again"),
                Err(NotificationError::InvalidTransition {
                    notification: n.id(),
                    from,
                    to: NotificationStatus::Failed,
                })
            );
        }
        let mut skipped = Notification::skipped(new(), "the template is turned off");
        assert_eq!(skipped.attempts(), 0);
        assert_eq!(skipped.error(), Some("the template is turned off"));
        assert_eq!(
            skipped.sent("log", None, at(1)),
            Err(NotificationError::InvalidTransition {
                notification: skipped.id(),
                from: NotificationStatus::Skipped,
                to: NotificationStatus::Sent,
            })
        );
    }

    #[test]
    fn a_skipped_message_may_have_no_subject() {
        let skipped = Notification::skipped(
            NewNotification {
                subject: String::new(),
                ..new()
            },
            "off",
        );
        assert_eq!(skipped.subject(), "");
        assert_eq!(Notification::restore(skipped.snapshot()), Ok(skipped));
    }

    #[test]
    fn long_errors_and_notes_are_cut_by_characters() {
        let mut n = leased().with_note(Some(&"ü".repeat(Notification::MAX_ERROR_LEN + 1)));
        n.failed("smtp", &"é".repeat(Notification::MAX_ERROR_LEN + 5))
            .unwrap();
        assert_eq!(
            n.error().map(|e| e.chars().count()),
            Some(Notification::MAX_ERROR_LEN)
        );
        assert_eq!(
            n.note().map(|e| e.chars().count()),
            Some(Notification::MAX_ERROR_LEN)
        );
    }

    #[test]
    fn restore_round_trips_every_state() {
        let mut sent = leased().with_note(Some("sent with the default template"));
        sent.sent("resend", Some("re_1".to_string()), at(1))
            .unwrap();
        for n in [
            Notification::queued(new(), email(), at(0)),
            leased(),
            Notification::sending(new()),
            sent,
        ] {
            assert_eq!(Notification::restore(n.snapshot()), Ok(n.clone()));
        }
    }

    #[test]
    fn restore_checks_the_times_and_the_email() {
        let mut sent = leased();
        sent.sent("resend", Some("re_1".to_string()), at(1))
            .unwrap();
        let broken = |change: fn(&mut NotificationSnapshot), from: &Notification| {
            let mut s = from.snapshot();
            change(&mut s);
            match Notification::restore(s) {
                Err(NotificationError::Inconsistent { reason, .. }) => reason,
                other => panic!("expected inconsistent, got {other:?}"),
            }
        };
        assert_eq!(
            broken(|s| s.sent_at = None, &sent),
            "sent_at is set exactly when it was sent"
        );
        let queued = Notification::queued(new(), email(), at(0));
        assert_eq!(
            broken(|s| s.next_attempt_at = None, &queued),
            "next_attempt_at is set exactly when it's queued"
        );
        assert_eq!(
            broken(|s| s.email = None, &queued),
            "the email is kept exactly while it's queued or leased"
        );
        assert_eq!(
            broken(|s| s.lease_until = Some(at(5)), &sent),
            "only a message being sent is leased"
        );
        assert_eq!(
            broken(|s| s.email = Some(email()), &sent),
            "the email is kept exactly while it's queued or leased"
        );
    }
}
