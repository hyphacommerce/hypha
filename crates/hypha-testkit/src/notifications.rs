use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, MutexGuard},
};

use hypha_core::{
    Email, EventId, Language, NotificationId, Page, PageRequest, RepoError, Timestamp, Versioned,
    stamp_insert, stamp_update,
};
use hypha_notifications::{
    EmailSender, Notification, NotificationFilter, NotificationRepo, NotificationSettings,
    NotificationSettingsRepo, NotificationTemplateRepo, OutgoingEmail, SendError, Sent, Template,
    TemplateKey, keys,
};

use crate::{
    InMemoryOutbox,
    table::{Table, insert_row, no_unique_keys, update_row},
};

/// A stored template's row key: (key, locale), `None` the default language.
type TemplateRow = (TemplateKey, Option<Language>);

/// Templates aren't on `Table` (its keys are `Copy`, and a locale isn't), but use the
/// same write rules through `insert_row` and `update_row`.
#[derive(Default)]
pub struct InMemoryNotificationTemplateRepo {
    pub(crate) templates: Mutex<HashMap<TemplateRow, Template>>,
    /// Shared with `InMemoryPorts`' other repos: the clock writes are stamped with.
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryNotificationTemplateRepo {
    /// A repo stamping its writes with `outbox`'s clock (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    fn rows(&self) -> MutexGuard<'_, HashMap<TemplateRow, Template>> {
        self.templates.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn row_of(template: &Template) -> TemplateRow {
    (template.key(), template.locale().cloned())
}

impl NotificationTemplateRepo for InMemoryNotificationTemplateRepo {
    async fn get(
        &self,
        key: TemplateKey,
        locale: Option<&Language>,
    ) -> Result<Option<Template>, RepoError> {
        Ok(self.rows().get(&(key, locale.cloned())).cloned())
    }

    async fn list(&self) -> Result<Vec<Template>, RepoError> {
        let mut all: Vec<Template> = self.rows().values().cloned().collect();
        all.sort_by_key(|t| (t.key(), t.locale().map(|l| l.as_str().to_string())));
        Ok(all)
    }

    async fn insert(&self, template: &mut Template) -> Result<(), RepoError> {
        stamp_insert(template, self.outbox.now());
        let row = row_of(template);
        insert_row(
            &mut self.rows(),
            row,
            template,
            keys::TEMPLATE_KEY,
            &no_unique_keys,
        )
    }

    async fn update(&self, template: &mut Template) -> Result<(), RepoError> {
        stamp_update(template, self.outbox.now());
        let row = row_of(template);
        update_row(&mut self.rows(), row, template, &no_unique_keys)
    }

    async fn delete(&self, template: &Template) -> Result<(), RepoError> {
        let mut rows = self.rows();
        let row = row_of(template);
        match rows.get(&row) {
            Some(stored) if stored.version() == template.version() => {}
            _ => return Err(RepoError::StaleVersion),
        }
        rows.remove(&row);
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryNotificationSettingsRepo {
    pub(crate) settings: Table<(), NotificationSettings>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryNotificationSettingsRepo {
    /// A repo stamping its writes with `outbox`'s clock (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }
}

impl NotificationSettingsRepo for InMemoryNotificationSettingsRepo {
    async fn get(&self) -> Result<Option<NotificationSettings>, RepoError> {
        Ok(self.settings.get(&()))
    }

    async fn insert(&self, settings: &mut NotificationSettings) -> Result<(), RepoError> {
        stamp_insert(settings, self.outbox.now());
        self.settings
            .insert((), settings, keys::SETTINGS, &no_unique_keys)
    }

    async fn update(&self, settings: &mut NotificationSettings) -> Result<(), RepoError> {
        stamp_update(settings, self.outbox.now());
        self.settings.update((), settings, &no_unique_keys)
    }
}

#[derive(Default)]
pub struct InMemoryNotificationRepo {
    pub(crate) notifications: Table<NotificationId, Notification>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryNotificationRepo {
    /// A repo stamping its writes with `outbox`'s clock (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }
}

/// `keys::NOTIFICATION_MESSAGE`: one message per (event, template, recipient) where
/// the event is set.
fn unique_message(a: &Notification, b: &Notification) -> Option<&'static str> {
    (a.event().is_some()
        && a.event() == b.event()
        && a.template() == b.template()
        && a.to() == b.to())
    .then_some(keys::NOTIFICATION_MESSAGE)
}

impl NotificationRepo for InMemoryNotificationRepo {
    async fn get(&self, id: NotificationId) -> Result<Option<Notification>, RepoError> {
        Ok(self.notifications.get(&id))
    }

    async fn find(
        &self,
        event: EventId,
        template: TemplateKey,
        to: &Email,
    ) -> Result<Option<Notification>, RepoError> {
        Ok(self
            .notifications
            .find(|n| n.event() == Some(event) && n.template() == template && n.to() == to))
    }

    async fn list(
        &self,
        filter: NotificationFilter,
        page: PageRequest<NotificationId>,
    ) -> Result<Page<Notification>, RepoError> {
        Ok(self
            .notifications
            .page(|n| filter.matches(n), page, Clone::clone))
    }

    async fn insert(&self, notification: &mut Notification) -> Result<(), RepoError> {
        stamp_insert(notification, self.outbox.now());
        self.notifications.insert(
            notification.id(),
            notification,
            keys::NOTIFICATION_ID,
            &unique_message,
        )
    }

    async fn update(&self, notification: &mut Notification) -> Result<(), RepoError> {
        stamp_update(notification, self.outbox.now());
        self.notifications
            .update(notification.id(), notification, &unique_message)
    }

    async fn lease_due(
        &self,
        now: Timestamp,
        lease_until: Timestamp,
        limit: u32,
    ) -> Result<Vec<Notification>, RepoError> {
        let mut rows = self.notifications.lock();
        let mut due: Vec<Notification> = rows.values().filter(|n| n.is_due(now)).cloned().collect();
        // Earliest due first (a queued one's attempt, else its lease's end), then by ID.
        due.sort_by_key(|n| (n.next_attempt_at().or(n.lease_until()), n.id()));
        due.truncate(limit as usize);
        let mut leased = Vec::with_capacity(due.len());
        for mut n in due {
            n.lease(now, lease_until)
                .map_err(|e| RepoError::Backend(e.to_string()))?;
            stamp_update(&mut n, self.outbox.now());
            update_row(&mut rows, n.id(), &mut n, &unique_message)?;
            leased.push(n);
        }
        Ok(leased)
    }

    async fn next_due(&self) -> Result<Option<Timestamp>, RepoError> {
        Ok(self
            .notifications
            .all()
            .iter()
            .filter_map(|n| match n.status() {
                hypha_notifications::NotificationStatus::Queued => n.next_attempt_at(),
                hypha_notifications::NotificationStatus::Sending => n.lease_until(),
                _ => None,
            })
            .min())
    }
}

/// An `EmailSender` that keeps what it's given (design note 0028, §6), and fails the
/// next sends with the errors it's told to.
#[derive(Default)]
pub struct InMemoryEmailSender {
    sent: Mutex<Vec<OutgoingEmail>>,
    failures: Mutex<VecDeque<SendError>>,
}

impl InMemoryEmailSender {
    /// Its name on the messages it sends.
    pub const NAME: &str = "memory";

    /// Every email it took, oldest first (failed sends aren't kept).
    pub fn sent(&self) -> Vec<OutgoingEmail> {
        self.sent.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Makes the next send fail with `error` (queued after any already set).
    pub fn fail_next(&self, error: SendError) {
        self.failures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(error);
    }
}

impl EmailSender for InMemoryEmailSender {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    async fn send(&self, email: &OutgoingEmail) -> Result<Sent, SendError> {
        let failure = self
            .failures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front();
        if let Some(error) = failure {
            return Err(error);
        }
        let mut sent = self.sent.lock().unwrap_or_else(|e| e.into_inner());
        sent.push(email.clone());
        Ok(Sent {
            provider_message_id: Some(format!("memory_{}", sent.len())),
        })
    }
}
