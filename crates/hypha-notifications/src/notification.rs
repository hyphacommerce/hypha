//! The record of one email (design note 0028, NT8): what went to whom, whether it went,
//! and why not. Bodies aren't kept (Q1): they hold addresses, order contents and, for
//! staff, tokens.

use std::{fmt, str::FromStr};

use hypha_core::{
    Email, EventId, NotificationId, OrderId, Stamped, Stamps, Timestamp, Version, Versioned,
    optional_text,
};

use crate::{NotificationError, TemplateKey};

/// Where a message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationStatus {
    /// Recorded and handed to the sender (or about to be); also what a send that
    /// crashed half way leaves, which the next run sends again.
    Sending,
    /// The provider accepted it.
    Sent,
    /// The provider refused it, or couldn't be reached; it may be tried again.
    Failed,
    /// Not sent on purpose (the template is turned off), so the order's emails can say
    /// why nothing went.
    Skipped,
}

impl NotificationStatus {
    pub const ALL: [NotificationStatus; 4] = [
        NotificationStatus::Sending,
        NotificationStatus::Sent,
        NotificationStatus::Failed,
        NotificationStatus::Skipped,
    ];

    /// The same string used in JSON and storage.
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationStatus::Sending => "sending",
            NotificationStatus::Sent => "sent",
            NotificationStatus::Failed => "failed",
            NotificationStatus::Skipped => "skipped",
        }
    }

    /// The moves a message may make: `sending` → `sent` or `failed`, and back to
    /// `sending` for another attempt (from `failed`, or from `sending` when the last
    /// attempt never reported). `sent` and `skipped` are final: resending a message
    /// makes a new one.
    pub fn can_become(self, to: NotificationStatus) -> bool {
        use NotificationStatus as S;
        matches!(
            (self, to),
            (S::Sending, S::Sent | S::Failed | S::Sending) | (S::Failed, S::Sending)
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
    /// The rendered subject line.
    pub subject: String,
    /// The event it answers; `None` for direct sends (invites, resets) and resends.
    pub event: Option<EventId>,
    /// The order it's about, for the order's emails.
    pub order: Option<OrderId>,
}

/// One email's record (design note 0028, §2). One per (event, template, recipient)
/// where `event` is set, so a handler that runs twice finds it and sends nothing more.
///
/// Invariants, checked by `restore`: `sent_at` set exactly when sent, a provider's
/// message ID only on a sent message, text within its length (longer subjects and
/// errors are cut, not refused: a record should never fail over what a provider said).
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
    event: Option<EventId>,
    order: Option<OrderId>,
    sent_at: Option<Timestamp>,
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
    /// Characters: enough for a provider's explanation.
    pub const MAX_ERROR_LEN: usize = 2000;

    /// A message about to be sent: `sending`, its first attempt counted.
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
            event: new.event,
            order: new.order,
            sent_at: None,
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

    /// Another attempt (a retry of a failed one, or of one whose attempt never
    /// reported): back to `sending`, counted.
    pub fn retry(&mut self) -> Result<(), NotificationError> {
        self.move_to(NotificationStatus::Sending)?;
        self.attempts = self.attempts.saturating_add(1);
        Ok(())
    }

    /// The provider (`provider`, the sender's name) accepted it at `at`, under its own
    /// `message_id` when it gives one. Clears the last attempt's error.
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
        Ok(())
    }

    /// The attempt through `provider` failed with `error`.
    pub fn failed(&mut self, provider: &str, error: &str) -> Result<(), NotificationError> {
        let provider = optional_text("provider", Some(provider.to_string()))?;
        self.move_to(NotificationStatus::Failed)?;
        self.provider = provider;
        self.error = Some(cut(error.trim(), Self::MAX_ERROR_LEN));
        Ok(())
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

    pub fn event(&self) -> Option<EventId> {
        self.event
    }

    pub fn order(&self) -> Option<OrderId> {
        self.order
    }

    pub fn sent_at(&self) -> Option<Timestamp> {
        self.sent_at
    }
}

/// `text` cut to at most `max` characters.
fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => text[..end].to_string(),
        None => text.to_string(),
    }
}

/// Plain-data form of a `Notification` for storage adapters (design note 0001, 3.4).
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
    pub event: Option<EventId>,
    pub order: Option<OrderId>,
    pub sent_at: Option<Timestamp>,
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
            event: self.event,
            order: self.order,
            sent_at: self.sent_at,
        }
    }

    /// Rebuilds a record from storage, checking every invariant.
    pub fn restore(s: NotificationSnapshot) -> Result<Self, NotificationError> {
        let inconsistent = |reason| NotificationError::Inconsistent {
            notification: s.id,
            reason,
        };
        let sent = s.status == NotificationStatus::Sent;
        if sent != s.sent_at.is_some() {
            return Err(inconsistent("sent_at is set exactly when it was sent"));
        }
        if !sent && s.provider_message_id.is_some() {
            return Err(inconsistent("only a sent message has a provider's ID"));
        }
        if s.subject.chars().count() > Self::MAX_SUBJECT_LEN
            || s.error
                .as_ref()
                .is_some_and(|e| e.chars().count() > Self::MAX_ERROR_LEN)
        {
            return Err(inconsistent("subject or error too long"));
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
            event: s.event,
            order: s.order,
            sent_at: s.sent_at,
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

    fn at() -> Timestamp {
        Timestamp::from_unix_micros(1_767_225_600_000_000)
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
    fn a_new_message_is_sending_its_first_attempt() {
        let n = Notification::sending(new());
        assert_eq!(n.status(), NotificationStatus::Sending);
        assert_eq!(n.attempts(), 1);
        assert_eq!(n.subject(), "Order #1001 confirmed");
    }

    #[test]
    fn sent_records_the_provider_and_time_and_clears_the_error() {
        let mut n = Notification::sending(new());
        n.failed("resend", "timed out").unwrap();
        n.retry().unwrap();
        assert_eq!(n.attempts(), 2);
        n.sent("resend", Some("re_123".to_string()), at()).unwrap();
        assert_eq!(n.status(), NotificationStatus::Sent);
        assert_eq!(n.provider(), Some("resend"));
        assert_eq!(n.provider_message_id(), Some("re_123"));
        assert_eq!(n.error(), None);
        assert_eq!(n.sent_at(), Some(at()));
    }

    #[test]
    fn sent_and_skipped_are_final() {
        let mut sent = Notification::sending(new());
        sent.sent("log", None, at()).unwrap();
        assert_eq!(
            sent.retry(),
            Err(NotificationError::InvalidTransition {
                notification: sent.id(),
                from: NotificationStatus::Sent,
                to: NotificationStatus::Sending,
            })
        );
        let mut skipped = Notification::skipped(new(), "the template is turned off");
        assert_eq!(skipped.attempts(), 0);
        assert_eq!(skipped.error(), Some("the template is turned off"));
        assert_eq!(
            skipped.sent("log", None, at()),
            Err(NotificationError::InvalidTransition {
                notification: skipped.id(),
                from: NotificationStatus::Skipped,
                to: NotificationStatus::Sent,
            })
        );
    }

    #[test]
    fn a_failed_message_is_retried_not_marked_failed_again() {
        let mut n = Notification::sending(new());
        n.failed("smtp", "refused").unwrap();
        assert_eq!(
            n.failed("smtp", "again"),
            Err(NotificationError::InvalidTransition {
                notification: n.id(),
                from: NotificationStatus::Failed,
                to: NotificationStatus::Failed,
            })
        );
    }

    #[test]
    fn long_errors_are_cut_by_characters() {
        let mut n = Notification::sending(new());
        n.failed("smtp", &"é".repeat(Notification::MAX_ERROR_LEN + 5))
            .unwrap();
        assert_eq!(
            n.error().map(|e| e.chars().count()),
            Some(Notification::MAX_ERROR_LEN)
        );
    }

    #[test]
    fn restore_round_trips_and_checks_sent_at() {
        let mut n = Notification::sending(new());
        n.sent("resend", Some("re_1".to_string()), at()).unwrap();
        assert_eq!(Notification::restore(n.snapshot()), Ok(n.clone()));
        let mut s = n.snapshot();
        s.sent_at = None;
        assert_eq!(
            Notification::restore(s),
            Err(NotificationError::Inconsistent {
                notification: n.id(),
                reason: "sent_at is set exactly when it was sent"
            })
        );
    }
}
