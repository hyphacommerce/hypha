//! Notifications (design note 0028): the emails a store sends, their Jinja templates
//! (the build's defaults until a merchant edits one), the store's branding and sender
//! settings, the record of every message, rendering with minijinja, and the
//! `EmailSender` port.
//!
//! A leaf domain: it depends on core only. The context each template gets, built from
//! orders, fulfilments, payments and the store, and the handler that sends on events are
//! the workflow's (`hypha-mail`).

pub mod commands;
pub mod defaults;
pub mod error;
pub mod key;
pub mod notification;
pub mod render;
pub mod repo;
pub mod sender;
pub mod settings;
pub mod template;

pub use commands::{
    CommandError, EffectiveTemplate, get_notification, get_notification_settings,
    get_notification_template, list_notification_templates, list_notifications,
    revert_notification_template, set_notification_template_enabled, settings_or_default,
    template_in_effect,
};
pub use defaults::{DefaultTemplate, default_template};
pub use error::{
    NotificationError, RenderError, SettingsError, TemplateError, TemplatePart, TemplateProblem,
};
pub use key::{Audience, TemplateKey};
pub use notification::{NewNotification, Notification, NotificationSnapshot, NotificationStatus};
pub use render::{FUEL, Rendered, html_to_text, render};
pub use repo::{
    HasNotificationSettings, HasNotificationTemplates, HasNotifications, NotificationFilter,
    NotificationRepo, NotificationSettingsRepo, NotificationTemplateRepo,
    StageNotificationSettings, keys,
};
pub use sender::{EmailSender, Mailbox, OutgoingEmail, SendError, Sent};
pub use settings::{Colour, NotificationSettings, NotificationSettingsSnapshot, OrderUrl};
pub use template::{NewTemplate, Template, TemplateSnapshot, TemplateSources};
