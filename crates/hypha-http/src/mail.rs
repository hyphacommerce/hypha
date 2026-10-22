//! Email in the host: `MailHost`, the `hypha_mail::MailEnv` both hosts send with (the
//! `Mailer` handler holds one, the routes borrow the state's), and `StaffEnv`, which gives
//! the staff flows (invites, password resets) their `Notifier`.

use std::sync::Arc;

use hypha_auth::{
    AuthSettings, HasAuthSettings, HashError, Password, PasswordHash, PasswordHasher, SealError,
    Sealed, SecretBox,
};
use hypha_core::{
    Clock, Email, IdGenerator, Notifier, NotifyError, OrderId, Random, StoreId, Timestamp,
};
use hypha_customer_accounts::{AccountSettings, CustomerEmail, HasAccountSettings};
use hypha_mail::{MailEnv, StaffMailer};
use hypha_media::Asset;
use hypha_users::StaffEmail;
use uuid::Uuid;

use crate::host::{HostClock, HostIds, HostMail, MailQueued};
use crate::media::Media;
use crate::state::{HostEnv, OrderTokens, Runtime};

/// What sending needs from the host: the configured sender (`[email] sender`), the
/// operator's from address (`email.from`), the store, the admin's URL
/// (`server.admin_url`), orders' access tokens, the media storage for the logo's URL and
/// the send job's wake-up.
/// Built once at startup, before the handlers, and shared (`Arc`) by the `Mailer` and
/// `AppState::mail`.
pub struct MailHost {
    sender: HostMail,
    clock: HostClock,
    ids: HostIds,
    from: Email,
    store: StoreId,
    /// Without a trailing `/`, as `hypha-config` gives it.
    admin_url: Option<String>,
    order_tokens: OrderTokens,
    media: Option<Arc<Media>>,
    queued: MailQueued,
}

/// The parts of a `MailHost` (`MailHost::new`).
pub struct MailParts {
    pub sender: HostMail,
    /// `email.from`.
    pub from: Email,
    pub store: StoreId,
    /// `server.admin_url`.
    pub admin_url: Option<String>,
    pub order_tokens: OrderTokens,
    pub media: Option<Arc<Media>>,
    /// Wakes the send job when emails are queued.
    pub queued: MailQueued,
}

impl MailHost {
    /// Its clock and IDs are the runtime's, as the state's are.
    pub fn new(parts: MailParts, runtime: &Runtime) -> Self {
        Self {
            sender: parts.sender,
            clock: runtime.clock.clone(),
            ids: runtime.ids.clone(),
            from: parts.from,
            store: parts.store,
            admin_url: parts.admin_url,
            order_tokens: parts.order_tokens,
            media: parts.media,
            queued: parts.queued,
        }
    }

    /// The sender's name (`log`, `smtp`, `resend`), for the settings' response.
    pub fn sender_name(&self) -> &'static str {
        hypha_notifications::EmailSender::name(&self.sender)
    }
}

impl MailEnv for MailHost {
    type Sender = HostMail;
    type Clock = HostClock;
    type Ids = HostIds;

    fn sender(&self) -> &HostMail {
        &self.sender
    }

    fn clock(&self) -> &HostClock {
        &self.clock
    }

    fn ids(&self) -> &HostIds {
        &self.ids
    }

    fn sender_address(&self) -> &Email {
        &self.from
    }

    fn store(&self) -> StoreId {
        self.store
    }

    fn admin_url(&self) -> Option<&str> {
        self.admin_url.as_deref()
    }

    fn order_token(&self, order: OrderId) -> String {
        self.order_tokens.token(order)
    }

    /// As product responses make them (`Media::url`): public under `media.public_url`,
    /// else signed for `media.download_expires`, which an email read later outlives;
    /// set a public URL in production. `None` without media storage, or when the URL
    /// can't be made (logged): the email goes without its logo.
    async fn asset_url(&self, asset: &Asset) -> Option<String> {
        let media = self.media.as_deref()?;
        match media.url(asset).await {
            Ok(url) => url,
            Err(e) => {
                tracing::warn!(asset = %asset.id(), error = ?e, "email: no URL for the logo");
                None
            }
        }
    }

    fn queued(&self) {
        self.queued.notify();
    }
}

/// The staff flows' environment (`hypha_users::invite_user`, `request_password_reset`), and
/// the customer flows' (`hypha_customer_accounts`): the host's auth ports, and their emails
/// sent through the store's templates (`hypha_mail::send_staff`, `send_customer`) and
/// recorded in `ports`, right where the flow notifies, which is after its own writes
/// committed.
pub struct StaffEnv<'a, P> {
    env: &'a HostEnv,
    mail: StaffMailer<'a, MailHost, P>,
}

impl<'a, P> StaffEnv<'a, P> {
    pub fn new(env: &'a HostEnv, mail: &'a MailHost, ports: &'a P) -> Self {
        Self {
            env,
            mail: StaffMailer { env: mail, ports },
        }
    }
}

impl<P> HasAuthSettings for StaffEnv<'_, P> {
    fn auth_settings(&self) -> &AuthSettings {
        self.env.auth_settings()
    }
}

impl<P> HasAccountSettings for StaffEnv<'_, P> {
    fn account_settings(&self) -> &AccountSettings {
        self.env.account_settings()
    }
}

impl<P> IdGenerator for StaffEnv<'_, P> {
    fn new_uuid(&self) -> Uuid {
        self.env.new_uuid()
    }
}

impl<P> Clock for StaffEnv<'_, P> {
    fn now(&self) -> Timestamp {
        self.env.now()
    }
}

impl<P> Random for StaffEnv<'_, P> {
    fn fill(&self, bytes: &mut [u8]) {
        self.env.fill(bytes);
    }
}

impl<P> PasswordHasher for StaffEnv<'_, P> {
    async fn hash_password(
        &self,
        password: &Password,
        salt: [u8; 16],
    ) -> Result<PasswordHash, HashError> {
        self.env.hash_password(password, salt).await
    }

    async fn verify_password(
        &self,
        password: &Password,
        hash: &PasswordHash,
    ) -> Result<bool, HashError> {
        self.env.verify_password(password, hash).await
    }
}

impl<P> SecretBox for StaffEnv<'_, P> {
    fn seal(&self, plaintext: &[u8], context: &[u8], nonce: [u8; 24]) -> Sealed {
        self.env.seal(plaintext, context, nonce)
    }

    fn open(&self, sealed: &Sealed, context: &[u8]) -> Result<Vec<u8>, SealError> {
        self.env.open(sealed, context)
    }
}

impl<P> Notifier<CustomerEmail> for StaffEnv<'_, P>
where
    P: hypha_stores::StoreRepo
        + hypha_media::HasAssets
        + hypha_notifications::HasNotificationTemplates
        + hypha_notifications::HasNotificationSettings
        + hypha_notifications::HasNotifications,
{
    async fn notify(&self, message: CustomerEmail) -> Result<(), NotifyError> {
        hypha_mail::send_customer(self.mail.env, self.mail.ports, message)
            .await
            .map(drop)
    }
}

impl<P> Notifier<StaffEmail> for StaffEnv<'_, P>
where
    P: hypha_stores::StoreRepo
        + hypha_media::HasAssets
        + hypha_notifications::HasNotificationTemplates
        + hypha_notifications::HasNotificationSettings
        + hypha_notifications::HasNotifications,
{
    async fn notify(&self, message: StaffEmail) -> Result<(), NotifyError> {
        hypha_mail::send_staff(self.mail.env, self.mail.ports, message)
            .await
            .map(drop)
    }
}
