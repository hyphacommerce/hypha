//! External apps (design note 0016, section 8): one record per app that Hypha signs
//! calls to, owning its signing secret, its webhook endpoint and the events it
//! subscribes to. Shipping's rate apps are apps too: their rate settings live in
//! `hypha-shipping` under the same key. Later, OAuth and an app's granted scopes join
//! this record.
//!
//! A leaf domain: it depends on core only.

pub mod app;
pub mod commands;
pub mod error;
pub mod repo;

pub use app::{App, AppSnapshot, EventPattern, NewApp, Webhook};
pub use commands::{
    CommandError, CreateApp, SECRET_PREFIX, UpdateApp, WebhookInput, create_app, delete_app,
    get_app, get_event, list_apps, list_deliveries, open_secret, retry_delivery, rotate_app_secret,
    update_app,
};
pub use error::AppError;
pub use hypha_core::{AppKey, AppKeyError};
pub use repo::{AppRepo, HasApps, keys};
