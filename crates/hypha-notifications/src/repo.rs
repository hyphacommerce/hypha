use hypha_core::{Email, EventId, Language, NotificationId, OrderId, Page, PageRequest, RepoError};

use crate::{Notification, NotificationSettings, NotificationStatus, Template, TemplateKey};

/// Uniqueness rules the notification repos enforce, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    /// One stored template per (key, locale).
    pub const TEMPLATE_KEY: &str = "notification_template_key";
    /// There's one settings record per store.
    pub const SETTINGS: &str = "notification_settings";
    pub const NOTIFICATION_ID: &str = "notification_id";
    /// One message per (event, template, recipient) where the event is set (NT8): a
    /// handler that runs twice can't send twice.
    pub const NOTIFICATION_MESSAGE: &str = "notification_message";
}

/// The templates a merchant edited (or turned off); a key without one uses the build's
/// default.
#[allow(async_fn_in_trait)]
pub trait NotificationTemplateRepo {
    /// `locale: None` is the default language (stored as `''`).
    async fn get(
        &self,
        key: TemplateKey,
        locale: Option<&Language>,
    ) -> Result<Option<Template>, RepoError>;
    /// Every stored template, by key (`TemplateKey::ALL` order) then locale (none first).
    /// A store has a handful.
    async fn list(&self) -> Result<Vec<Template>, RepoError>;
    /// Stores a new template (version 1). Enforces `TEMPLATE_KEY`.
    async fn insert(&self, template: &mut Template) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, template: &mut Template) -> Result<(), RepoError>;
    /// Removes `template` at the version it was loaded at, or fails with `StaleVersion`
    /// (changed or gone since).
    async fn delete(&self, template: &Template) -> Result<(), RepoError>;
}

/// The store's notification settings: none stored until the merchant first saves.
#[allow(async_fn_in_trait)]
pub trait NotificationSettingsRepo {
    async fn get(&self) -> Result<Option<NotificationSettings>, RepoError>;
    /// Stores them for the first time (version 1); `Duplicate(keys::SETTINGS)` when
    /// they're stored already.
    async fn insert(&self, settings: &mut NotificationSettings) -> Result<(), RepoError>;
    /// `StaleVersion` unless the stored settings have `settings`' version.
    async fn update(&self, settings: &mut NotificationSettings) -> Result<(), RepoError>;
}

/// Which messages a list shows; every field set narrows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NotificationFilter {
    pub order: Option<OrderId>,
    pub template: Option<TemplateKey>,
    pub status: Option<NotificationStatus>,
}

impl NotificationFilter {
    pub fn matches(&self, n: &Notification) -> bool {
        self.order.is_none_or(|order| n.order() == Some(order))
            && self.template.is_none_or(|t| n.template() == t)
            && self.status.is_none_or(|s| n.status() == s)
    }
}

/// The record of every message.
#[allow(async_fn_in_trait)]
pub trait NotificationRepo {
    async fn get(&self, id: NotificationId) -> Result<Option<Notification>, RepoError>;
    /// The message answering `event` with `template` to `to`, if one was recorded: how
    /// a handler that runs again finds what it did.
    async fn find(
        &self,
        event: EventId,
        template: TemplateKey,
        to: &Email,
    ) -> Result<Option<Notification>, RepoError>;
    /// A page of the messages `filter` keeps, newest first (design note 0012).
    async fn list(
        &self,
        filter: NotificationFilter,
        page: PageRequest<NotificationId>,
    ) -> Result<Page<Notification>, RepoError>;
    /// Stores a new message (version 1). Enforces `NOTIFICATION_ID` and
    /// `NOTIFICATION_MESSAGE`.
    async fn insert(&self, notification: &mut Notification) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, notification: &mut Notification) -> Result<(), RepoError>;
}

/// Stages settings writes in a `hypha_core::HasBatch` batch, so the workflow can write
/// them with the logo's asset usage (`hypha_media::StageAssetUsages`) in one commit, as
/// a category's image is (design note 0028, §11).
pub trait StageNotificationSettings {
    /// Stages what `NotificationSettingsRepo::insert` would write; `settings` is at
    /// version 1 once the batch commits (`Duplicate(keys::SETTINGS)` if stored already).
    fn insert_notification_settings(&mut self, settings: &mut NotificationSettings);
    /// Stages what `NotificationSettingsRepo::update` would write, and moves `settings`
    /// to the version it has once the batch commits (stale then: `StaleVersion`,
    /// nothing written).
    fn update_notification_settings(&mut self, settings: &mut NotificationSettings);
}

/// Whatever holds the `NotificationTemplateRepo`: a host's per-request ports, a test
/// world.
pub trait HasNotificationTemplates {
    type NotificationTemplates: NotificationTemplateRepo;
    fn notification_templates(&self) -> &Self::NotificationTemplates;
}

/// Whatever holds the `NotificationSettingsRepo`.
pub trait HasNotificationSettings {
    type NotificationSettings: NotificationSettingsRepo;
    fn notification_settings(&self) -> &Self::NotificationSettings;
}

/// Whatever holds the `NotificationRepo`.
pub trait HasNotifications {
    type Notifications: NotificationRepo;
    fn notifications(&self) -> &Self::Notifications;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasNotificationTemplates + ?Sized> HasNotificationTemplates for &T {
    type NotificationTemplates = T::NotificationTemplates;
    fn notification_templates(&self) -> &Self::NotificationTemplates {
        (**self).notification_templates()
    }
}

impl<T: HasNotificationSettings + ?Sized> HasNotificationSettings for &T {
    type NotificationSettings = T::NotificationSettings;
    fn notification_settings(&self) -> &Self::NotificationSettings {
        (**self).notification_settings()
    }
}

impl<T: HasNotifications + ?Sized> HasNotifications for &T {
    type Notifications = T::Notifications;
    fn notifications(&self) -> &Self::Notifications {
        (**self).notifications()
    }
}
