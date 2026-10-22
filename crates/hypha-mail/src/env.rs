//! What sending needs besides the store's repositories: the sender, the operator's from
//! address, how the host makes links (the admin's, an order's access token, a file's URL),
//! the store it serves, time and IDs, and how the send job is woken. A host implements
//! `MailEnv` once, over what it holds; the `Mailer` handler keeps one, and the commands
//! take one like `ids` and `clock`.

use std::sync::Arc;

use hypha_core::{Clock, Email, IdGenerator, OrderId, StoreId};
use hypha_media::Asset;
use hypha_notifications::EmailSender;

/// The non-storage inputs of every send. No `Send` bound, like every port: a host on
/// concrete types proves its futures `Send`.
#[allow(async_fn_in_trait)]
pub trait MailEnv {
    type Sender: EmailSender;
    type Clock: Clock;
    type Ids: IdGenerator;

    /// Where emails go out (`[email] sender`).
    fn sender(&self) -> &Self::Sender;

    /// When a message was sent.
    fn clock(&self) -> &Self::Clock;

    /// The messages' IDs (`ntf_…`).
    fn ids(&self) -> &Self::Ids;

    /// The operator's address (`[email] from`, a domain the provider verified); the
    /// merchant's sender name goes with it.
    fn sender_address(&self) -> &Email;

    /// The store this deployment serves (the host resolved it), read for its name, contact
    /// email and time zone.
    fn store(&self) -> StoreId;

    /// Where the admin panel lives (`server.admin_url`, no trailing `/`), for links in
    /// staff emails; `None`: they show the bare token, and `staff_new_order` has no
    /// `admin_url`.
    fn admin_url(&self) -> Option<&str>;

    /// The token that lets the buyer read `order` (`HostEnv::order_token`), for the
    /// `{token}` of the settings' order URL. A function of the host's, so the workflow
    /// doesn't know how it's made.
    fn order_token(&self, order: OrderId) -> String;

    /// A URL an email client can load `asset` from (the logo), or `None` when there's
    /// none (no media storage, not ready). Prefer a public, unsigned URL: an email is
    /// read long after it's sent.
    async fn asset_url(&self, asset: &Asset) -> Option<String>;

    /// Messages were queued: wake the send job (natively its runner; on Workers the
    /// event's after-write run). Called after the records are stored.
    fn queued(&self);
}

/// Forwards every input to the environment pointed at, for the pointers a host may hold
/// its environment through.
macro_rules! forward {
    ($($ty:ty),+) => {$(
        impl<T: MailEnv + ?Sized> MailEnv for $ty {
            type Sender = T::Sender;
            type Clock = T::Clock;
            type Ids = T::Ids;

            fn sender(&self) -> &Self::Sender {
                (**self).sender()
            }

            fn clock(&self) -> &Self::Clock {
                (**self).clock()
            }

            fn ids(&self) -> &Self::Ids {
                (**self).ids()
            }

            fn sender_address(&self) -> &Email {
                (**self).sender_address()
            }

            fn store(&self) -> StoreId {
                (**self).store()
            }

            fn admin_url(&self) -> Option<&str> {
                (**self).admin_url()
            }

            fn order_token(&self, order: OrderId) -> String {
                (**self).order_token(order)
            }

            async fn asset_url(&self, asset: &Asset) -> Option<String> {
                (**self).asset_url(asset).await
            }

            fn queued(&self) {
                (**self).queued()
            }
        }
    )+};
}

// A borrowed or shared environment is one, so a host can hand the handler its own.
forward!(&T, Arc<T>);
