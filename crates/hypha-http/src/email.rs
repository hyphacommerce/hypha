//! The `log` email sender, the default: every email is written to the log instead of being
//! sent, as before notifications. Development only; the hosts warn on startup when it's in
//! effect.

use hypha_notifications::{EmailSender, OutgoingEmail, SendError, Sent};

use crate::host::HostMail;

/// Logs each email's recipient, subject and text (the HTML is left out: the text says
/// the same, and the links in it are what a developer copies).
#[derive(Debug, Clone, Copy, Default)]
pub struct LogSender;

impl EmailSender for LogSender {
    fn name(&self) -> &'static str {
        "log"
    }

    async fn send(&self, email: &OutgoingEmail) -> Result<Sent, SendError> {
        tracing::info!(
            to = %email.to,
            from = %email.from,
            subject = email.subject,
            body = email.text,
            "email (logged, not sent)"
        );
        Ok(Sent::default())
    }
}

impl HostMail {
    /// The log sender, for both hosts (and tests).
    ///
    /// A host wraps its other senders the same way, for their concrete types (the
    /// compiler proves an `async fn`'s future `Send` only for a concrete type; on
    /// Workers, inside `worker::send::SendFuture`).
    pub fn log() -> Self {
        Self::new(LogSender.name(), |email| {
            Box::pin(async move { LogSender.send(&email).await })
        })
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{Email, NotificationId};
    use hypha_notifications::Mailbox;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn the_log_sends_nothing_and_says_so() {
        let mail = HostMail::log();
        assert_eq!(mail.name(), "log");
        let email = OutgoingEmail {
            id: NotificationId::from_uuid(Uuid::from_u128(1)),
            from: Mailbox {
                name: None,
                address: Email::new("shop@example.com").unwrap(),
            },
            reply_to: None,
            to: Email::new("buyer@example.com").unwrap(),
            subject: "Hi".to_string(),
            html: "<p>Hi</p>".to_string(),
            text: "Hi".to_string(),
        };
        assert_eq!(pollster::block_on(mail.send(&email)), Ok(Sent::default()));
    }

    #[test]
    fn a_host_mail_is_its_senders_name_and_answer() {
        let refusing = HostMail::new("test", |_| {
            Box::pin(async { Err(SendError::Permanent("no".to_string())) })
        });
        assert_eq!(refusing.name(), "test");
        let email = OutgoingEmail {
            id: NotificationId::from_uuid(Uuid::from_u128(2)),
            from: Mailbox {
                name: Some("Shop".to_string()),
                address: Email::new("shop@example.com").unwrap(),
            },
            reply_to: None,
            to: Email::new("buyer@example.com").unwrap(),
            subject: String::new(),
            html: String::new(),
            text: String::new(),
        };
        assert_eq!(
            pollster::block_on(refusing.send(&email)),
            Err(SendError::Permanent("no".to_string()))
        );
        assert_eq!(format!("{refusing:?}"), "HostMail(\"test\")");
    }
}
