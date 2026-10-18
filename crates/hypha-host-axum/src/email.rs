//! The configured email sender (design note 0028, §5): the log, SMTP (`SmtpSender`, on
//! `lettre` with rustls, here only: Workers can't open SMTP connections) or Resend over
//! `reqwest`, as one `HostMail`.

use std::time::Duration;

use hypha_config::{EmailConfig, SenderConfig, SmtpConfig, SmtpTls};
use hypha_http::host::HostMail;
use hypha_mail_resend::{ReqwestClient, ResendSender};
use hypha_notifications::{EmailSender, OutgoingEmail, SendError, Sent};
use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// Builds the sender `config` names, reading its secret through `env`
/// (`HYPHA_SMTP_PASSWORD`, `HYPHA_RESEND_API_KEY`), and says which is in effect. An
/// error names what's missing, for startup to stop on.
pub fn email_sender(
    config: &EmailConfig,
    env: impl Fn(&str) -> Option<String>,
) -> Result<HostMail, String> {
    let env = |name: &str| env(name).filter(|v| !v.trim().is_empty());
    match &config.sender {
        SenderConfig::Log => {
            tracing::warn!(
                "emails are logged, not sent (email.sender = \"log\"): development only"
            );
            Ok(HostMail::log())
        }
        SenderConfig::Smtp(smtp) => {
            let password = env("HYPHA_SMTP_PASSWORD");
            let sender = SmtpSender::new(smtp, password)?;
            tracing::info!(
                host = smtp.host,
                port = smtp.port,
                from = %config.from,
                "emails: sent over SMTP"
            );
            Ok(HostMail::new("smtp", move |email| {
                let sender = sender.clone();
                Box::pin(async move { sender.send(&email).await })
            }))
        }
        SenderConfig::Resend => {
            let key = env("HYPHA_RESEND_API_KEY").ok_or(
                "HYPHA_RESEND_API_KEY is required with email.sender = \"resend\" \
                 (an API key from resend.com/api-keys, allowed to send)",
            )?;
            let http = ReqwestClient::new()?;
            let sender = std::sync::Arc::new(ResendSender::new(key, http));
            tracing::info!(from = %config.from, "emails: sent through Resend");
            Ok(HostMail::new("resend", move |email| {
                let sender = std::sync::Arc::clone(&sender);
                Box::pin(async move { sender.send(&email).await })
            }))
        }
    }
}

/// Sends over SMTP: any provider's server, or Mailpit locally. Connections are pooled
/// and reused between emails.
#[derive(Clone)]
pub struct SmtpSender {
    transport: AsyncSmtpTransport<Tokio1Executor>,
}

impl SmtpSender {
    /// How long connecting and each command may take.
    pub const TIMEOUT: Duration = Duration::from_secs(20);

    /// `password` is `HYPHA_SMTP_PASSWORD`, needed with `username` and only with it.
    /// Call it inside the Tokio runtime: the connection pool runs (and is dropped) there.
    pub fn new(config: &SmtpConfig, password: Option<String>) -> Result<Self, String> {
        let host = config.host.as_str();
        let builder = match config.tls {
            SmtpTls::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
            SmtpTls::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
                .map_err(|e| format!("email.smtp: {e}"))?,
            SmtpTls::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(host)
                .map_err(|e| format!("email.smtp: {e}"))?,
        };
        let mut builder = builder.port(config.port).timeout(Some(Self::TIMEOUT));
        match (&config.username, password) {
            (Some(username), Some(password)) => {
                builder = builder.credentials(Credentials::new(username.clone(), password));
            }
            (Some(_), None) => {
                return Err("HYPHA_SMTP_PASSWORD is required with email.smtp.username".to_string());
            }
            (None, Some(_)) => {
                return Err(
                    "HYPHA_SMTP_PASSWORD is set without email.smtp.username: set both, \
                     or neither for a server without login"
                        .to_string(),
                );
            }
            (None, None) => {}
        }
        Ok(Self {
            transport: builder.build(),
        })
    }
}

/// `email` as a MIME message: text and HTML as alternatives, the shop's name with its
/// address. An address `lettre` can't write is refused for good.
pub fn message(email: &OutgoingEmail) -> Result<Message, SendError> {
    let address = |a: &str| {
        a.parse::<lettre::Address>()
            .map_err(|e| SendError::Permanent(format!("{a:?} can't be sent to: {e}")))
    };
    let name = email
        .from
        .name
        .as_deref()
        .map(|n| n.chars().filter(|c| !c.is_control()).collect::<String>())
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    // The message's record names it, on the sender's domain, as providers expect.
    let domain = email
        .from
        .address
        .as_str()
        .rsplit('@')
        .next()
        .unwrap_or("localhost");
    let mut builder = Message::builder()
        .message_id(Some(format!("<{}@{domain}>", email.id)))
        .from(Mailbox::new(name, address(email.from.address.as_str())?))
        .to(Mailbox::new(None, address(email.to.as_str())?))
        .subject(email.subject.as_str());
    if let Some(reply_to) = &email.reply_to {
        builder = builder.reply_to(Mailbox::new(None, address(reply_to.as_str())?));
    }
    builder
        .multipart(MultiPart::alternative_plain_html(
            email.text.clone(),
            email.html.clone(),
        ))
        .map_err(|e| SendError::Permanent(format!("the message can't be written: {e}")))
}

/// A permanent refusal (5xx) never works; anything else (no connection, a timeout, a
/// 4xx "try later", TLS) may.
fn send_error(e: &lettre::transport::smtp::Error) -> SendError {
    if e.is_permanent() {
        SendError::Permanent(e.to_string())
    } else {
        SendError::Transient(e.to_string())
    }
}

impl EmailSender for SmtpSender {
    fn name(&self) -> &'static str {
        "smtp"
    }

    async fn send(&self, email: &OutgoingEmail) -> Result<Sent, SendError> {
        let message = message(email)?;
        self.transport
            .send(message)
            .await
            .map_err(|e| send_error(&e))?;
        // SMTP's answer ("250 OK: queued as …") has no ID worth keeping.
        Ok(Sent::default())
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{Email, NotificationId};
    use uuid::Uuid;

    use super::*;

    fn email() -> OutgoingEmail {
        OutgoingEmail {
            id: NotificationId::from_uuid(Uuid::from_u128(1)),
            from: hypha_notifications::Mailbox {
                name: Some("Ana, \"the\" Shop".to_string()),
                address: Email::new("shop@hypha.localhost").unwrap(),
            },
            reply_to: Some(Email::new("help@example.com").unwrap()),
            to: Email::new("buyer@example.com").unwrap(),
            subject: "Order #1001 confirmed".to_string(),
            html: "<p>Thanks for your order</p>".to_string(),
            text: "Thanks for your order".to_string(),
        }
    }

    fn smtp(tls: SmtpTls, username: Option<&str>) -> SmtpConfig {
        SmtpConfig {
            host: "localhost".to_string(),
            port: 1025,
            tls,
            username: username.map(str::to_string),
        }
    }

    #[test]
    fn a_message_has_both_bodies_the_shops_name_and_the_reply_to() {
        let formatted = String::from_utf8(message(&email()).unwrap().formatted()).unwrap();
        for header in [
            // A name with specials is written encoded (RFC 2047), so its comma can't
            // split it into two addresses: base64 of `Ana, "the" Shop`.
            "From: =?utf-8?b?QW5hLCAidGhlIiBTaG9w?= <shop@hypha.localhost>\r\n",
            "To: buyer@example.com\r\n",
            "Message-ID: <ntf_00000000000000000000000000000001@hypha.localhost>\r\n",
            "Reply-To: help@example.com\r\n",
            "Subject: Order #1001 confirmed\r\n",
            "Content-Type: multipart/alternative;",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "Content-Type: text/html; charset=utf-8\r\n",
            "Thanks for your order\r\n",
            "<p>Thanks for your order</p>\r\n",
        ] {
            assert!(formatted.contains(header), "{header:?} in:\n{formatted}");
        }
    }

    #[test]
    fn without_a_name_or_reply_to_the_headers_are_plain() {
        let mut email = email();
        email.from.name = Some(" ".to_string());
        email.reply_to = None;
        let formatted = String::from_utf8(message(&email).unwrap().formatted()).unwrap();
        assert!(
            formatted.contains("From: shop@hypha.localhost\r\n"),
            "{formatted}"
        );
        assert!(!formatted.contains("Reply-To"), "{formatted}");
    }

    // The transport's pool lives on Tokio, so it's made (and dropped) in a runtime.
    #[tokio::test]
    async fn a_login_needs_its_password_and_a_password_its_login() {
        let new = |username, password: Option<&str>| {
            SmtpSender::new(&smtp(SmtpTls::None, username), password.map(str::to_string))
                .map(|_| ())
        };
        assert_eq!(
            new(Some("me"), None),
            Err("HYPHA_SMTP_PASSWORD is required with email.smtp.username".to_string())
        );
        assert!(new(None, Some("secret")).is_err());
        assert_eq!(new(Some("me"), Some("secret")), Ok(()));
        assert_eq!(new(None, None), Ok(()));
    }

    #[tokio::test]
    async fn each_sender_is_built_from_the_config() {
        let from = Email::new("shop@example.com").unwrap();
        let config = |sender| EmailConfig {
            sender,
            from: from.clone(),
        };
        let no_env = |_: &str| None;
        let log = email_sender(&config(SenderConfig::Log), no_env).unwrap();
        assert_eq!(log.name(), "log");
        let smtp = email_sender(
            &config(SenderConfig::Smtp(smtp(SmtpTls::Starttls, None))),
            no_env,
        )
        .unwrap();
        assert_eq!(smtp.name(), "smtp");
        assert_eq!(
            email_sender(&config(SenderConfig::Resend), |_| Some(" ".to_string()))
                .map(|m| m.name()),
            Err(
                "HYPHA_RESEND_API_KEY is required with email.sender = \"resend\" \
                 (an API key from resend.com/api-keys, allowed to send)"
                    .to_string()
            )
        );
        let resend = email_sender(&config(SenderConfig::Resend), |name| {
            (name == "HYPHA_RESEND_API_KEY").then(|| "re_test".to_string())
        })
        .unwrap();
        assert_eq!(resend.name(), "resend");
    }

    #[tokio::test]
    async fn nothing_listening_is_transient() {
        // Port 9 (discard) on loopback: nothing answers SMTP there.
        let mut config = smtp(SmtpTls::None, None);
        config.port = 9;
        let sender = SmtpSender::new(&config, None).unwrap();
        let result = sender.send(&email()).await;
        assert!(matches!(result, Err(SendError::Transient(_))), "{result:?}");
    }

    /// Against Mailpit (`docker compose up -d`): the message lands in its inbox at
    /// http://localhost:8025. `cargo test -p hypha-host-axum -- --ignored mailpit`.
    #[tokio::test]
    #[ignore = "needs Mailpit on localhost:1025"]
    async fn mailpit_takes_a_message() {
        let sender = SmtpSender::new(&smtp(SmtpTls::None, None), None).unwrap();
        assert_eq!(sender.send(&email()).await, Ok(Sent::default()));
    }
}
