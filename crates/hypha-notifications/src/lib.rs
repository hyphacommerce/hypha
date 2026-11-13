//! Notifications: the emails a store sends, their Jinja templates (the build's defaults
//! until a merchant edits one), the store's branding and sender settings, the record of
//! every message, rendering with minijinja, and the `EmailSender` port.
//!
//! A leaf domain: it depends on core only. The context each template gets, built from
//! orders, fulfilments, payments and the store, and the handler that sends on events are
//! the workflow's (`hypha-mail`).

pub mod commands;
pub mod defaults;
pub mod error;
mod generated {
    pub(crate) mod defaults;
}
pub mod key;
pub mod notification;
pub mod render;
pub mod repo;
pub mod sender;
pub mod sending;
pub mod settings;
pub mod template;

pub use commands::{
    CommandError, EffectiveTemplate, ListedTemplate, Origin, Step, TemplateLanguages,
    get_notification, get_notification_settings, get_notification_template,
    list_notification_templates, list_notifications, lookup_chain, revert_notification_template,
    set_notification_template_enabled, settings_or_default, template_in_effect,
};
pub use defaults::{
    ContextStrings, DefaultTemplate, default_languages, default_template, default_template_in,
};
pub use error::{
    NotificationError, RenderError, SettingsError, TemplateError, TemplatePart, TemplateProblem,
};
pub use key::{Audience, TemplateKey};
pub use notification::{
    NewNotification, Notification, NotificationSnapshot, NotificationStatus, QueuedEmail,
};
pub use render::{FUEL, Rendered, html_to_text, render};
pub use repo::{
    HasNotificationSettings, HasNotificationTemplates, HasNotifications, NotificationFilter,
    NotificationRepo, NotificationSettingsRepo, NotificationTemplateRepo,
    StageNotificationSettings, keys,
};
pub use sender::{EmailSender, Mailbox, OutgoingEmail, SendError, Sent};
pub use sending::{LONGEST_SEND, SEND_TIMEOUT, SendSettings};
pub use settings::{
    Colour, NOTIFICATION_SETTINGS, NotificationSettings, NotificationSettingsSnapshot,
    NotificationSettingsUpdated, OrderUrl,
};
pub use template::{NewTemplate, Template, TemplateSnapshot, TemplateSources};
