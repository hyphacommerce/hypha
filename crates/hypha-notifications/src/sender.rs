//! The `EmailSender` port: one way out for every email, picked by the operator (`log`,
//! `smtp`, `resend`). Like every port it has no `Send` bound; it's passed to commands and
//! the handler apart from the request's ports, like the `WebhookSender` and the search
//! engine.

use std::fmt;

use hypha_core::{Email, ErrorCode, NotificationId};
use thiserror::Error;

/// An address with an optional display name: `"Shop" <shop@mail.example.com>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mailbox {
    pub name: Option<String>,
    pub address: Email,
}

/// As a header writes it: the name quoted (with `"` and `\` escaped) so commas and
/// other specials in a shop's name can't split it into several addresses.
impl fmt::Display for Mailbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            Some(name) => {
                f.write_str("\"")?;
                for c in name.chars().filter(|c| !c.is_control()) {
                    if c == '"' || c == '\\' {
                        f.write_str("\\")?;
                    }
                    write!(f, "{c}")?;
                }
                write!(f, "\" <{}>", self.address)
            }
            None => write!(f, "{}", self.address),
        }
    }
}

/// One rendered email, ready to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingEmail {
    /// The message's record; the provider's idempotency key where it has one, so a
    /// retried send doesn't deliver twice.
    pub id: NotificationId,
    /// The operator's address (a domain the provider verified) with the merchant's
    /// sender name.
    pub from: Mailbox,
    pub reply_to: Option<Email>,
    pub to: Email,
    pub subject: String,
    pub html: String,
    pub text: String,
}

/// What the provider said when it took the message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sent {
    /// Its ID for the message, when it gives one (Resend does; the log has none).
    pub provider_message_id: Option<String>,
}

/// Why a send didn't happen. The record keeps the text; the kind decides whether the
/// dispatcher tries again.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SendError {
    /// Try again later (timeout, 5xx, rate limited).
    #[error("the email provider is unavailable: {0}")]
    Transient(String),
    /// Never going to work (address refused, sender not verified).
    #[error("the email provider refused the message: {0}")]
    Permanent(String),
}

impl SendError {
    /// The provider's words, without the kind.
    pub fn message(&self) -> &str {
        match self {
            SendError::Transient(message) | SendError::Permanent(message) => message,
        }
    }
}

impl ErrorCode for SendError {
    fn code(&self) -> &'static str {
        match self {
            SendError::Transient(_) => "email_unavailable",
            SendError::Permanent(_) => "email_refused",
        }
    }
}

/// Sends email. Implemented by the hosts' senders (`log`, `smtp`), `hypha-mail-resend`
/// and the testkit's `InMemoryEmailSender`.
#[allow(async_fn_in_trait)]
pub trait EmailSender {
    /// "log", "smtp", "resend": recorded on the message.
    fn name(&self) -> &'static str;
    async fn send(&self, email: &OutgoingEmail) -> Result<Sent, SendError>;
}

/// A borrowed sender is a sender, so hosts can lend theirs per request.
impl<T: EmailSender + ?Sized> EmailSender for &T {
    fn name(&self) -> &'static str {
        (**self).name()
    }

    async fn send(&self, email: &OutgoingEmail) -> Result<Sent, SendError> {
        (**self).send(email).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mailbox(name: Option<&str>) -> String {
        Mailbox {
            name: name.map(str::to_string),
            address: Email::new("shop@mail.example.com").unwrap(),
        }
        .to_string()
    }

    #[test]
    fn a_mailbox_quotes_its_name() {
        assert_eq!(
            mailbox(Some("Ana, \"the\" shop\\")),
            r#""Ana, \"the\" shop\\" <shop@mail.example.com>"#
        );
        assert_eq!(
            mailbox(Some("Line\nbreak")),
            r#""Linebreak" <shop@mail.example.com>"#
        );
    }

    #[test]
    fn a_mailbox_without_a_name_is_the_address() {
        assert_eq!(mailbox(None), "shop@mail.example.com");
        assert_eq!(mailbox(Some("  ")), "shop@mail.example.com");
    }
}
