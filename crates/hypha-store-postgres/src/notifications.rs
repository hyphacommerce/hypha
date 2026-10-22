//! Notifications: stored templates, the store's one settings row, and the record of
//! messages. One row each, no child tables.

use hypha_core::{
    AssetId, Email, EventId, Language, NotificationId, OrderId, Page, PageRequest, RepoError,
    Timestamp, Version, Versioned, stamp_insert, stamp_update,
};
use hypha_notifications::{
    Colour, Notification, NotificationFilter, NotificationRepo, NotificationSettings,
    NotificationSettingsRepo, NotificationSettingsSnapshot, NotificationSnapshot,
    NotificationTemplateRepo, OrderUrl, QueuedEmail, Template, TemplateKey, TemplateSnapshot,
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

fn notification_columns(prefix: &str) -> String {
    let p = prefix;
    format!(
        "{p}id, {p}version, {p}template, {p}recipient, {p}subject, {p}status, {p}attempts,
         {p}provider, {p}provider_message_id, {p}error, {p}note, {p}event_id, {p}order_id,
         {p}email_sender_name, {p}email_reply_to, {p}email_html, {p}email_text, {}, {}, {}, {}",
        micros(&format!("{p}sent_at")),
        micros(&format!("{p}next_attempt_at")),
        micros(&format!("{p}lease_until")),
        stamp_columns(p)
    )
}

fn notification_from_row(row: &Row) -> Result<Notification, RepoError> {
    let id = NotificationId::from_uuid(col(row, "id")?);
    let status: String = col(row, "status")?;
    let attempts: i32 = col(row, "attempts")?;
    let event: Option<Uuid> = col(row, "event_id")?;
    let order: Option<Uuid> = col(row, "order_id")?;
    let html: Option<String> = col(row, "email_html")?;
    let text: Option<String> = col(row, "email_text")?;
    let reply_to: Option<String> = col(row, "email_reply_to")?;
    let email = match (html, text) {
        (Some(html), Some(text)) => Some(QueuedEmail {
            sender_name: col(row, "email_sender_name")?,
            reply_to: reply_to.as_deref().map(email_from_db).transpose()?,
            html,
            text,
        }),
        _ => None,
    };
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
        note: col(row, "note")?,
        event: event.map(EventId::from_uuid),
        order: order.map(OrderId::from_uuid),
        sent_at: opt_ts(col(row, "sent_at")?),
        next_attempt_at: opt_ts(col(row, "next_attempt_at")?),
        lease_until: opt_ts(col(row, "lease_until")?),
        email,
    })
    .map_err(|e| corrupt(format!("notification {id}: {e}")))
}

/// A message's values as its columns hold them.
struct NotificationColumns<'a> {
    s: &'a NotificationSnapshot,
    template: &'static str,
    recipient: &'a str,
    status: &'static str,
    attempts: i32,
    event: Option<Uuid>,
    order: Option<Uuid>,
    sent_at: Option<i64>,
    next_attempt_at: Option<i64>,
    lease_until: Option<i64>,
    sender_name: Option<&'a str>,
    reply_to: Option<&'a str>,
    html: Option<&'a str>,
    text: Option<&'a str>,
}

/// The columns `INSERT` and `UPDATE` write after `id` and `version`, in this order.
const NOTIFICATION_FIELDS: [&str; 18] = [
    "template",
    "recipient",
    "subject",
    "status",
    "attempts",
    "provider",
    "provider_message_id",
    "error",
    "note",
    "event_id",
    "order_id",
    "email_sender_name",
    "email_reply_to",
    "email_html",
    "email_text",
    "sent_at",
    "next_attempt_at",
    "lease_until",
];

/// How many of `NOTIFICATION_FIELDS` are timestamps (the last ones), written with `ts`.
const NOTIFICATION_TIMES: usize = 3;

impl<'a> NotificationColumns<'a> {
    fn of(s: &'a NotificationSnapshot) -> Result<Self, RepoError> {
        let email = s.email.as_ref();
        Ok(Self {
            s,
            template: s.template.as_str(),
            recipient: s.to.as_str(),
            status: s.status.as_str(),
            attempts: i32::try_from(s.attempts)
                .map_err(|_| corrupt(format!("{} attempts too many", s.id)))?,
            event: s.event.map(|e| e.as_uuid()),
            order: s.order.map(|o| o.as_uuid()),
            sent_at: opt_micros(s.sent_at),
            next_attempt_at: opt_micros(s.next_attempt_at),
            lease_until: opt_micros(s.lease_until),
            sender_name: email.and_then(|e| e.sender_name.as_deref()),
            reply_to: email.and_then(|e| e.reply_to.as_ref().map(Email::as_str)),
            html: email.map(|e| e.html.as_str()),
            text: email.map(|e| e.text.as_str()),
        })
    }

    /// `NOTIFICATION_FIELDS`' values, in order.
    fn params(&self) -> Vec<Param<'_>> {
        vec![
            (&self.template, Type::TEXT),
            (&self.recipient, Type::TEXT),
            (&self.s.subject, Type::TEXT),
            (&self.status, Type::TEXT),
            (&self.attempts, Type::INT4),
            (&self.s.provider, Type::TEXT),
            (&self.s.provider_message_id, Type::TEXT),
            (&self.s.error, Type::TEXT),
            (&self.s.note, Type::TEXT),
            (&self.event, Type::UUID),
            (&self.order, Type::UUID),
            (&self.sender_name, Type::TEXT),
            (&self.reply_to, Type::TEXT),
            (&self.html, Type::TEXT),
            (&self.text, Type::TEXT),
            (&self.sent_at, Type::INT8),
            (&self.next_attempt_at, Type::INT8),
            (&self.lease_until, Type::INT8),
        ]
    }
}

/// `$n` for field `i` of `NOTIFICATION_FIELDS` whose parameters start at `$first`.
fn field_value(i: usize, first: usize) -> String {
    let n = first + i;
    if i >= NOTIFICATION_FIELDS.len() - NOTIFICATION_TIMES {
        ts(n)
    } else {
        format!("${n}")
    }
}

impl<C: PgConnection> NotificationRepo for PgStore<C> {
    async fn get(&self, id: NotificationId) -> Result<Option<Notification>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM notifications WHERE id = $1",
                    notification_columns("")
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
                    notification_columns("")
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
                    notification_columns(""),
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
        let c = NotificationColumns::of(&s)?;
        let id = s.id.as_uuid();
        let values: Vec<String> = (0..NOTIFICATION_FIELDS.len())
            .map(|i| field_value(i, 2))
            .collect();
        let mut params: Vec<Param<'_>> = vec![(&id, Type::UUID)];
        params.extend(c.params());
        in_transaction!(self, |tx| {
            tx.query_typed(
                &format!(
                    "INSERT INTO notifications (id, version, {}) VALUES ($1, 1, {})",
                    NOTIFICATION_FIELDS.join(", "),
                    values.join(", ")
                ),
                &params,
            )
            .await
            .map_err(repo_error)?;
            write_stamps(tx, "notifications", "id", &id, Type::UUID, s.stamps).await
        })?;
        notification.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, notification: &mut Notification) -> Result<(), RepoError> {
        stamp_update(notification, self.now());
        let s = notification.snapshot();
        let c = NotificationColumns::of(&s)?;
        let id = s.id.as_uuid();
        let version = version_to_db(s.version)?;
        let assignments: Vec<String> = NOTIFICATION_FIELDS
            .iter()
            .enumerate()
            .map(|(i, field)| format!("{field} = {}", field_value(i, 3)))
            .collect();
        let mut params: Vec<Param<'_>> = vec![(&id, Type::UUID), (&version, Type::INT8)];
        params.extend(c.params());
        let version = in_transaction!(self, |tx| {
            let row = tx
                .query_typed_opt(
                    &format!(
                        "UPDATE notifications SET version = version + 1, {}
                         WHERE id = $1 AND version = $2 RETURNING version",
                        assignments.join(", ")
                    ),
                    &params,
                )
                .await
                .map_err(repo_error)?
                .ok_or(RepoError::StaleVersion)?;
            let version = version_from_db(col(&row, "version")?)?;
            write_stamps(tx, "notifications", "id", &id, Type::UUID, s.stamps).await?;
            Ok(version)
        })?;
        notification.set_version(version);
        Ok(())
    }

    async fn lease_due(
        &self,
        now: Timestamp,
        lease_until: Timestamp,
        limit: u32,
    ) -> Result<Vec<Notification>, RepoError> {
        let (now, until, stamp) = (
            now.unix_micros(),
            lease_until.unix_micros(),
            self.now().unix_micros(),
        );
        // `Notification::lease` in SQL: due (queued and its attempt has come, or leased
        // by a lease that ended), earliest first; rows another lease is taking are
        // skipped. The CHECKs keep a leased row's email.
        in_transaction!(self, |tx| {
            let rows = tx
                .query_typed(
                    &format!(
                        "UPDATE notifications n SET version = n.version + 1, status = 'sending',
                             attempts = n.attempts + 1, next_attempt_at = NULL,
                             lease_until = {until}, updated_at = {stamp}
                         FROM (
                             SELECT id FROM notifications
                             WHERE (status = 'queued' AND next_attempt_at <= {now})
                                OR (lease_until IS NOT NULL AND lease_until <= {now})
                             ORDER BY COALESCE(next_attempt_at, lease_until), id
                             LIMIT $4 FOR UPDATE SKIP LOCKED
                         ) due
                         WHERE n.id = due.id
                         RETURNING {}",
                        notification_columns("n."),
                        now = ts(1),
                        until = ts(2),
                        stamp = ts(3),
                    ),
                    &[
                        (&now, Type::INT8),
                        (&until, Type::INT8),
                        (&stamp, Type::INT8),
                        (&i64::from(limit), Type::INT8),
                    ],
                )
                .await
                .map_err(repo_error)?;
            let mut leased = rows
                .iter()
                .map(notification_from_row)
                .collect::<Result<Vec<_>, _>>()?;
            // `RETURNING` has no order.
            leased.sort_by_key(|n| (n.lease_until(), n.id()));
            Ok(leased)
        })
    }

    async fn next_due(&self) -> Result<Option<Timestamp>, RepoError> {
        let mut conn = self.conn().await;
        // `LEAST` skips NULLs: nothing on one side.
        let row = conn
            .db()
            .query_typed_one(
                &format!(
                    "SELECT {} FROM (SELECT LEAST(
                         (SELECT min(next_attempt_at) FROM notifications WHERE status = 'queued'),
                         (SELECT min(lease_until) FROM notifications WHERE lease_until IS NOT NULL)
                     ) AS due) next",
                    micros("due")
                ),
                &[],
            )
            .await
            .map_err(repo_error)?;
        let due: Option<i64> = col(&row, "due")?;
        Ok(due.map(Timestamp::from_unix_micros))
    }
}
