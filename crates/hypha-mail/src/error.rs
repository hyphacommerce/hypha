use hypha_core::{
    AssetId, AuthError, ErrorCode, FulfillmentId, NotificationId, OrderId, RepoError, StoreId,
};
use hypha_media::AssetKind;
use hypha_notifications::{
    NotificationError, RenderError, SendError, SettingsError, TemplateError, TemplateKey,
};
use thiserror::Error;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Template(#[from] TemplateError),
    /// A template that doesn't render against its sample (saving, previews): its
    /// `problem` has the part and line.
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error(transparent)]
    Notification(#[from] NotificationError),
    /// The provider didn't take the message. A transient one is tried again by the
    /// dispatcher (the message's record says `failed` meanwhile).
    #[error(transparent)]
    Send(#[from] SendError),
    #[error("order {0} not found")]
    OrderNotFound(OrderId),
    #[error("fulfilment {0} not found")]
    FulfillmentNotFound(FulfillmentId),
    /// A shipping email for an order that has shipped nothing.
    #[error("order {0} has no fulfilment to write about")]
    NoFulfillment(OrderId),
    #[error("notification {0} not found")]
    NotificationNotFound(NotificationId),
    /// The store the host serves isn't in its database: a host set up wrongly.
    #[error("store {0} not found")]
    StoreNotFound(StoreId),
    #[error("asset {0} not found")]
    AssetNotFound(AssetId),
    /// Only an asset whose upload completed can be the logo.
    #[error("asset {0} isn't ready: its upload hasn't completed")]
    AssetNotReady(AssetId),
    /// Emails are read anywhere: the logo must be public.
    #[error("asset {0} is private")]
    AssetNotPublic(AssetId),
    #[error("asset {asset} is a {kind} file; the logo must be an image")]
    AssetKindNotAllowed { asset: AssetId, kind: AssetKind },
    /// A test send with the `log` sender: there'd be nothing to receive.
    #[error("no email provider is configured (the sender is \"log\"): set [email] sender")]
    EmailNotConfigured,
    /// A test goes to the calling staff member's own address, which only a staff
    /// session has.
    #[error("only a staff member can send a test email, to their own address")]
    StaffOnly,
    /// Messages that carry a secret (invites, resets) can't be sent again: the token
    /// isn't kept. Nor can one about no order.
    #[error("a {template} message can't be resent")]
    NotResendable { template: TemplateKey },
    /// An event whose payload isn't what its type promises.
    #[error("event {event} ({event_type}) has an unexpected payload: {reason}")]
    BadEvent {
        event: hypha_core::EventId,
        event_type: String,
        reason: String,
    },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Template(e) => e.code(),
            CommandError::Render(e) => e.code(),
            CommandError::Settings(e) => e.code(),
            CommandError::Notification(e) => e.code(),
            CommandError::Send(e) => e.code(),
            CommandError::OrderNotFound(_) => "order_not_found",
            CommandError::FulfillmentNotFound(_) => "fulfillment_not_found",
            CommandError::NoFulfillment(_) => "order_not_fulfilled",
            CommandError::NotificationNotFound(_) => "notification_not_found",
            CommandError::StoreNotFound(_) => "store_not_found",
            CommandError::AssetNotFound(_) => "asset_not_found",
            CommandError::AssetNotReady(_) => "asset_not_ready",
            CommandError::AssetNotPublic(_) => "asset_not_public",
            CommandError::AssetKindNotAllowed { .. } => "asset_kind_not_allowed",
            CommandError::EmailNotConfigured => "email_not_configured",
            CommandError::StaffOnly => "staff_only",
            CommandError::NotResendable { .. } => "notification_not_resendable",
            CommandError::BadEvent { .. } => "event_payload_invalid",
        }
    }
}

impl CommandError {
    /// The template problem with its part and line, for `details`.
    pub fn problem(&self) -> Option<&hypha_notifications::TemplateProblem> {
        match self {
            CommandError::Render(e) => e.problem(),
            CommandError::Template(TemplateError::Invalid(problem)) => Some(problem),
            _ => None,
        }
    }
}
