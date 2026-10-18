//! Notifications (design note 0028): stored templates, the store's one settings row,
//! and the record of messages. One row each, no child tables.

use hypha_core::{
    AssetId, Email, EventId, Language, NotificationId, OrderId, Page, PageRequest, RepoError,
    Version, Versioned, stamp_insert, stamp_update,
};
use hypha_notifications::{
    Colour, Notification, NotificationFilter, NotificationRepo, NotificationSettings,
    NotificationSettingsRepo, NotificationSettingsSnapshot, NotificationSnapshot,
    NotificationTemplateRepo, OrderUrl, Template, TemplateKey, TemplateSnapshot,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        Param, col, micros, opt_micros, opt_ts, stamp_columns, stamps_from_row, ts,
        version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, Queries, in_transaction},
};

/// The default language is stored as `''` (part of the primary key, so not NULL).
fn locale_to_db(locale: Option<&Language>) -> &str {
    locale.map_or("", Language::as_str)
}

fn locale_from_db(locale: &str) -> Result<Option<Language>, RepoError> {
    if locale.is_empty() {
        return Ok(None);
    }
    Language::new(locale).map(Some).map_err(corrupt)
}

fn key_from_db(key: &str) -> Result<TemplateKey, RepoError> {
    key.parse()
        .map_err(|k| corrupt(format!("unknown template {k:?}")))
}

fn template_columns() -> String {
    format!(
        "key, locale, version, subject, html, text, enabled, {}",
        stamp_columns("")
    )
}

fn template_from_row(row: &Row) -> Result<Template, RepoError> {
    let key = key_from_db(&col::<String>(row, "key")?)?;
    Template::restore(TemplateSnapshot {
        key,
        locale: locale_from_db(&col::<String>(row, "locale")?)?,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        subject: col(row, "subject")?,
        html: col(row, "html")?,
        text: col(row, "text")?,
        enabled: col(row, "enabled")?,
    })
    .map_err(|e| corrupt(format!("template {key}: {e}")))
}

impl<C: PgConnection> NotificationTemplateRepo for PgStore<C> {
    async fn get(
        &self,
        key: TemplateKey,
        locale: Option<&Language>,
    ) -> Result<Option<Template>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM notification_templates WHERE key = $1 AND locale = $2",
                    template_columns()
                ),
                &[
                    (&key.as_str(), Type::TEXT),
                    (&locale_to_db(locale), Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(template_from_row).transpose()
    }

    async fn list(&self) -> Result<Vec<Template>, RepoError> {
        let mut conn = self.conn().await;
        let mut templates = conn
            .db()
            .query_typed(
                &format!("SELECT {} FROM notification_templates", template_columns()),
                &[],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(template_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        // By key in `TemplateKey::ALL` order (not the text's), then locale: a handful of
        // rows, sorted here rather than spelling the order out in SQL.
        templates.sort_by_key(|t| (t.key(), locale_to_db(t.locale()).to_string()));
        Ok(templates)
    }

    async fn insert(&self, template: &mut Template) -> Result<(), RepoError> {
        stamp_insert(template, self.now());
        let s = template.snapshot();
        let locale = locale_to_db(s.locale.as_ref()).to_string();
        in_transaction!(self, |tx| {
            tx.query_typed(
                "INSERT INTO notification_templates (key, locale, version, subject, html,
                     text, enabled)
                 VALUES ($1, $2, 1, $3, $4, $5, $6)",
                &[
                    (&s.key.as_str(), Type::TEXT),
                    (&locale, Type::TEXT),
                    (&s.subject, Type::TEXT),
                    (&s.html, Type::TEXT),
                    (&s.text, Type::TEXT),
                    (&s.enabled, Type::BOOL),
                ],
            )
            .await
            .map_err(repo_error)?;
            template_stamps(tx, &s, &locale).await
        })?;
        template.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, template: &mut Template) -> Result<(), RepoError> {
        stamp_update(template, self.now());
        let s = template.snapshot();
        let locale = locale_to_db(s.locale.as_ref()).to_string();
        let version = in_transaction!(self, |tx| {
            let row = tx
                .query_typed_opt(
                    "UPDATE notification_templates SET version = version + 1, subject = $4,
                         html = $5, text = $6, enabled = $7
                     WHERE key = $1 AND locale = $2 AND version = $3 RETURNING version",
                    &[
                        (&s.key.as_str(), Type::TEXT),
                        (&locale, Type::TEXT),
                        (&version_to_db(s.version)?, Type::INT8),
                        (&s.subject, Type::TEXT),
                        (&s.html, Type::TEXT),
                        (&s.text, Type::TEXT),
                        (&s.enabled, Type::BOOL),
                    ],
                )
                .await
                .map_err(repo_error)?
                .ok_or(RepoError::StaleVersion)?;
            let version = version_from_db(col(&row, "version")?)?;
            template_stamps(tx, &s, &locale).await?;
            Ok(version)
        })?;
        template.set_version(version);
        Ok(())
    }

    async fn delete(&self, template: &Template) -> Result<(), RepoError> {
        let mut conn = self.conn().await;
        let deleted = conn
            .db()
            .query_typed_opt(
                "DELETE FROM notification_templates
                 WHERE key = $1 AND locale = $2 AND version = $3 RETURNING key",
                &[
                    (&template.key().as_str(), Type::TEXT),
                    (&locale_to_db(template.locale()), Type::TEXT),
                    (&version_to_db(template.version())?, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        deleted.map(|_| ()).ok_or(RepoError::StaleVersion)
    }
}

/// `write_stamps` for a template, whose key is two columns.
async fn template_stamps(
    tx: &impl Queries,
    s: &TemplateSnapshot,
    locale: &str,
) -> Result<(), RepoError> {
    let Some(stamps) = s.stamps else {
        return Ok(());
    };
    tx.query_typed(
        &format!(
            "UPDATE notification_templates SET created_at = {}, updated_at = {}
             WHERE key = $1 AND locale = $2",
            ts(3),
            ts(4)
        ),
        &[
            (&s.key.as_str(), Type::TEXT),
            (&locale, Type::TEXT),
            (&stamps.created_at.unix_micros(), Type::INT8),
            (&stamps.updated_at.unix_micros(), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

fn email_from_db(address: &str) -> Result<Email, RepoError> {
    Email::new(address).map_err(corrupt)
}

impl<C: PgConnection> NotificationSettingsRepo for PgStore<C> {
    async fn get(&self) -> Result<Option<NotificationSettings>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT version, logo_asset_id, accent, footer, sender_name, reply_to,
                         staff_recipients, order_url, {}
                     FROM notification_settings",
                    stamp_columns("")
                ),
                &[],
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = row else { return Ok(None) };
        let logo: Option<Uuid> = col(&row, "logo_asset_id")?;
        let reply_to: Option<String> = col(&row, "reply_to")?;
        let recipients: Vec<String> = col(&row, "staff_recipients")?;
        let order_url: Option<String> = col(&row, "order_url")?;
        NotificationSettings::restore(NotificationSettingsSnapshot {
            version: version_from_db(col(&row, "version")?)?,
            stamps: stamps_from_row(&row)?,
            logo: logo.map(AssetId::from_uuid),
            accent: Colour::new(&col::<String>(&row, "accent")?).map_err(corrupt)?,
            footer: col(&row, "footer")?,
            sender_name: col(&row, "sender_name")?,
            reply_to: reply_to.as_deref().map(email_from_db).transpose()?,
            staff_recipients: recipients
                .iter()
                .map(|r| email_from_db(r))
                .collect::<Result<_, _>>()?,
            order_url: order_url
                .as_deref()
                .map(OrderUrl::new)
                .transpose()
                .map_err(corrupt)?,
        })
        .map(Some)
        .map_err(|e| corrupt(format!("notification settings: {e}")))
    }

    async fn insert(&self, settings: &mut NotificationSettings) -> Result<(), RepoError> {
        stamp_insert(settings, self.now());
        let s = settings.snapshot();
        in_transaction!(self, |tx| insert_settings(tx, &s).await)?;
        settings.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, settings: &mut NotificationSettings) -> Result<(), RepoError> {
        stamp_update(settings, self.now());
        let s = settings.snapshot();
        let version = in_transaction!(self, |tx| update_settings(tx, &s).await)?;
        settings.set_version(version);
        Ok(())
    }
}

/// The settings' columns after `version`, as parameters from `$2`.
fn settings_params(s: &NotificationSettingsSnapshot) -> SettingsColumns {
    SettingsColumns {
        logo: s.logo.map(|l| l.as_uuid()),
        accent: s.accent.as_str().to_string(),
        reply_to: s.reply_to.as_ref().map(|e| e.as_str().to_string()),
        recipients: s
            .staff_recipients
            .iter()
            .map(|e| e.as_str().to_string())
            .collect(),
        order_url: s.order_url.as_ref().map(|u| u.as_str().to_string()),
    }
}

/// The settings' values as their columns hold them.
struct SettingsColumns {
    logo: Option<Uuid>,
    accent: String,
    reply_to: Option<String>,
    recipients: Vec<String>,
    order_url: Option<String>,
}

/// `NotificationSettingsRepo::insert` inside the caller's transaction (also used by
/// `PgBatch`).
pub(crate) async fn insert_settings(
    tx: &impl Queries,
    s: &NotificationSettingsSnapshot,
) -> Result<(), RepoError> {
    let c = settings_params(s);
    tx.query_typed(
        "INSERT INTO notification_settings (id, version, logo_asset_id, accent, footer,
             sender_name, reply_to, staff_recipients, order_url)
         VALUES (1, 1, $1, $2, $3, $4, $5, $6, $7)",
        &[
            (&c.logo, Type::UUID),
            (&c.accent, Type::TEXT),
            (&s.footer, Type::TEXT),
            (&s.sender_name, Type::TEXT),
            (&c.reply_to, Type::TEXT),
            (&c.recipients, Type::TEXT_ARRAY),
            (&c.order_url, Type::TEXT),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_stamps(
        tx,
        "notification_settings",
        "id",
        &1i16,
        Type::INT2,
        s.stamps,
    )
    .await
}

/// `NotificationSettingsRepo::update` inside the caller's transaction (also used by
/// `PgBatch`); the new version.
pub(crate) async fn update_settings(
    tx: &impl Queries,
    s: &NotificationSettingsSnapshot,
) -> Result<Version, RepoError> {
    let c = settings_params(s);
    let row = tx
        .query_typed_opt(
            "UPDATE notification_settings SET version = version + 1, logo_asset_id = $2,
                 accent = $3, footer = $4, sender_name = $5, reply_to = $6,
                 staff_recipients = $7, order_url = $8
             WHERE version = $1 RETURNING version",
            &[
                (&version_to_db(s.version)?, Type::INT8),
                (&c.logo, Type::UUID),
                (&c.accent, Type::TEXT),
                (&s.footer, Type::TEXT),
                (&s.sender_name, Type::TEXT),
                (&c.reply_to, Type::TEXT),
                (&c.recipients, Type::TEXT_ARRAY),
                (&c.order_url, Type::TEXT),
            ],
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    let version = version_from_db(col(&row, "version")?)?;
    write_stamps(
        tx,
        "notification_settings",
        "id",
        &1i16,
        Type::INT2,
        s.stamps,
    )
    .await?;
    Ok(version)
}

fn notification_columns() -> String {
    format!(
        "id, version, template, recipient, subject, status, attempts, provider,
         provider_message_id, error, event_id, order_id, {}, {}",
        micros("sent_at"),
        stamp_columns("")
    )
}

fn notification_from_row(row: &Row) -> Result<Notification, RepoError> {
    let id = NotificationId::from_uuid(col(row, "id")?);
    let status: String = col(row, "status")?;
    let attempts: i32 = col(row, "attempts")?;
    let event: Option<Uuid> = col(row, "event_id")?;
    let order: Option<Uuid> = col(row, "order_id")?;
    Notification::restore(NotificationSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        template: key_from_db(&col::<String>(row, "template")?)?,
        to: email_from_db(&col::<String>(row, "recipient")?)?,
        subject: col(row, "subject")?,
        status: status
            .parse()
            .map_err(|s| corrupt(format!("notification {id} status {s:?}")))?,
        attempts: u32::try_from(attempts)
            .map_err(|_| corrupt(format!("notification {id} attempts {attempts}")))?,
        provider: col(row, "provider")?,
        provider_message_id: col(row, "provider_message_id")?,
        error: col(row, "error")?,
        event: event.map(EventId::from_uuid),
        order: order.map(OrderId::from_uuid),
        sent_at: opt_ts(col(row, "sent_at")?),
    })
    .map_err(|e| corrupt(format!("notification {id}: {e}")))
}

fn attempts_to_db(n: &NotificationSnapshot) -> Result<i32, RepoError> {
    i32::try_from(n.attempts).map_err(|_| corrupt(format!("{} attempts too many", n.id)))
}

impl<C: PgConnection> NotificationRepo for PgStore<C> {
    async fn get(&self, id: NotificationId) -> Result<Option<Notification>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM notifications WHERE id = $1",
                    notification_columns()
                ),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(notification_from_row).transpose()
    }

    async fn find(
        &self,
        event: EventId,
        template: TemplateKey,
        to: &Email,
    ) -> Result<Option<Notification>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM notifications
                     WHERE event_id = $1 AND template = $2 AND recipient = $3",
                    notification_columns()
                ),
                &[
                    (&event.as_uuid(), Type::UUID),
                    (&template.as_str(), Type::TEXT),
                    (&to.as_str(), Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(notification_from_row).transpose()
    }

    async fn list(
        &self,
        filter: NotificationFilter,
        page: PageRequest<NotificationId>,
    ) -> Result<Page<Notification>, RepoError> {
        let after = page.after.map(|id| id.as_uuid());
        let order = filter.order.map(|o| o.as_uuid());
        let template = filter.template.map(TemplateKey::as_str);
        let status = filter.status.map(|s| s.as_str());
        let fetch = i64::from(page.limit.get()) + 1;
        // Only the filters given, so each query is one the planner can match to an
        // index (an order's emails: `notifications_by_order`) instead of a generic
        // `$1 IS NULL OR …` plan.
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Param<'_>> = Vec::new();
        if let Some(after) = &after {
            params.push((after, Type::UUID));
            conditions.push(format!("id < ${}", params.len()));
        }
        if let Some(order) = &order {
            params.push((order, Type::UUID));
            conditions.push(format!("order_id = ${}", params.len()));
        }
        if let Some(template) = &template {
            params.push((template, Type::TEXT));
            conditions.push(format!("template = ${}", params.len()));
        }
        if let Some(status) = &status {
            params.push((status, Type::TEXT));
            conditions.push(format!("status = ${}", params.len()));
        }
        params.push((&fetch, Type::INT8));
        let filters = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let mut conn = self.conn().await;
        let found = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM notifications {filters} ORDER BY id DESC LIMIT ${}",
                    notification_columns(),
                    params.len()
                ),
                &params,
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(notification_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn insert(&self, notification: &mut Notification) -> Result<(), RepoError> {
        stamp_insert(notification, self.now());
        let s = notification.snapshot();
        let attempts = attempts_to_db(&s)?;
        in_transaction!(self, |tx| {
            tx.query_typed(
                &format!(
                    "INSERT INTO notifications (id, version, template, recipient, subject,
                         status, attempts, provider, provider_message_id, error, event_id,
                         order_id, sent_at)
                     VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, {})",
                    ts(12)
                ),
                &[
                    (&s.id.as_uuid(), Type::UUID),
                    (&s.template.as_str(), Type::TEXT),
                    (&s.to.as_str(), Type::TEXT),
                    (&s.subject, Type::TEXT),
                    (&s.status.as_str(), Type::TEXT),
                    (&attempts, Type::INT4),
                    (&s.provider, Type::TEXT),
                    (&s.provider_message_id, Type::TEXT),
                    (&s.error, Type::TEXT),
                    (&s.event.map(|e| e.as_uuid()), Type::UUID),
                    (&s.order.map(|o| o.as_uuid()), Type::UUID),
                    (&opt_micros(s.sent_at), Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
            write_stamps(
                tx,
                "notifications",
                "id",
                &s.id.as_uuid(),
                Type::UUID,
                s.stamps,
            )
            .await
        })?;
        notification.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, notification: &mut Notification) -> Result<(), RepoError> {
        stamp_update(notification, self.now());
        let s = notification.snapshot();
        let attempts = attempts_to_db(&s)?;
        let version = in_transaction!(self, |tx| {
            let row = tx
                .query_typed_opt(
                    &format!(
                        "UPDATE notifications SET version = version + 1, template = $3,
                             recipient = $4, subject = $5, status = $6, attempts = $7,
                             provider = $8, provider_message_id = $9, error = $10,
                             event_id = $11, order_id = $12, sent_at = {}
                         WHERE id = $1 AND version = $2 RETURNING version",
                        ts(13)
                    ),
                    &[
                        (&s.id.as_uuid(), Type::UUID),
                        (&version_to_db(s.version)?, Type::INT8),
                        (&s.template.as_str(), Type::TEXT),
                        (&s.to.as_str(), Type::TEXT),
                        (&s.subject, Type::TEXT),
                        (&s.status.as_str(), Type::TEXT),
                        (&attempts, Type::INT4),
                        (&s.provider, Type::TEXT),
                        (&s.provider_message_id, Type::TEXT),
                        (&s.error, Type::TEXT),
                        (&s.event.map(|e| e.as_uuid()), Type::UUID),
                        (&s.order.map(|o| o.as_uuid()), Type::UUID),
                        (&opt_micros(s.sent_at), Type::INT8),
                    ],
                )
                .await
                .map_err(repo_error)?
                .ok_or(RepoError::StaleVersion)?;
            let version = version_from_db(col(&row, "version")?)?;
            write_stamps(
                tx,
                "notifications",
                "id",
                &s.id.as_uuid(),
                Type::UUID,
                s.stamps,
            )
            .await?;
            Ok(version)
        })?;
        notification.set_version(version);
        Ok(())
    }
}
