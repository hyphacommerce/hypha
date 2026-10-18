//! External apps (design note 0016, section 8), with the manifest and grant of those
//! installed through OAuth (design note 0029).

use hypha_apps::{App, AppKey, AppRepo, AppSnapshot, GrantSnapshot};
use hypha_core::{
    AppId, Recorded, RepoError, Sealed, Version, Versioned, stamp_insert, stamp_update,
};
use tokio_postgres::{Row, types::Type};

use crate::{
    columns::{
        actor_from_db, actor_to_db, col, micros, opt_micros, opt_ts, scopes_from_db, scopes_to_db,
        stamp_columns, stamps_from_row, ts, version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, Queries, in_transaction},
};

fn columns() -> String {
    format!(
        "a.key, a.id, a.version, a.name, a.enabled, a.secret, a.webhook_url, \
         a.webhook_timeout_ms, a.subscriptions, a.manifest_url, a.manifest::text AS manifest, \
         {}, {}, g.scopes AS grant_scopes, g.approved_by_kind, g.approved_by_id, {}, {}",
        micros("a.manifest_fetched_at"),
        micros("a.uninstalled_at"),
        micros("g.approved_at"),
        stamp_columns("a.")
    )
}

const FROM: &str = "apps a LEFT JOIN app_grants g ON g.app_key = a.key";

fn app_from_row(row: &Row) -> Result<App, RepoError> {
    let key: String = col(row, "key")?;
    let timeout: Option<i32> = col(row, "webhook_timeout_ms")?;
    let manifest: Option<String> = col(row, "manifest")?;
    let manifest = manifest
        .map(|m| serde_json::from_str(&m))
        .transpose()
        .map_err(|e| corrupt(format!("app {key} manifest: {e}")))?;
    let grant = match col::<Option<Vec<String>>>(row, "grant_scopes")? {
        Some(scopes) => {
            let kind: String = col(row, "approved_by_kind")?;
            let approved_at: Option<i64> = col(row, "approved_at")?;
            Some(GrantSnapshot {
                scopes: scopes_from_db(scopes)?,
                approved_by: actor_from_db(&kind, col(row, "approved_by_id")?)?,
                approved_at: opt_ts(approved_at)
                    .ok_or_else(|| corrupt(format!("app {key}: a grant without its time")))?,
            })
        }
        None => None,
    };
    App::restore(AppSnapshot {
        key: key.clone(),
        id: AppId::from_uuid(col(row, "id")?),
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        name: col(row, "name")?,
        enabled: col(row, "enabled")?,
        secret: Sealed::from_bytes(col(row, "secret")?),
        webhook_url: col(row, "webhook_url")?,
        webhook_timeout_ms: timeout.map(u32::try_from).transpose().map_err(corrupt)?,
        subscriptions: col(row, "subscriptions")?,
        manifest_url: col(row, "manifest_url")?,
        manifest,
        manifest_fetched_at: opt_ts(col(row, "manifest_fetched_at")?),
        publisher: None,
        privacy_url: None,
        grant,
        uninstalled_at: opt_ts(col(row, "uninstalled_at")?),
    })
    .map_err(|e| corrupt(format!("app {key}: {e}")))
}

/// Webhook timeouts are small; anything else in the column is a bug.
fn timeout_to_db(ms: Option<u32>) -> Result<Option<i32>, RepoError> {
    ms.map(i32::try_from).transpose().map_err(corrupt)
}

impl<C: PgConnection> AppRepo for PgStore<C> {
    async fn get(&self, key: &AppKey) -> Result<Option<App>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!("SELECT {} FROM {FROM} WHERE a.key = $1", columns()),
                &[(&key.as_str(), Type::TEXT)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(app_from_row).transpose()
    }

    async fn get_by_id(&self, id: AppId) -> Result<Option<App>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!("SELECT {} FROM {FROM} WHERE a.id = $1", columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(app_from_row).transpose()
    }

    async fn list(&self) -> Result<Vec<App>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!("SELECT {} FROM {FROM} ORDER BY a.key", columns()),
                &[],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(app_from_row)
            .collect()
    }

    async fn insert(&self, app: &mut App) -> Result<(), RepoError> {
        stamp_insert(app, self.now());
        let s = app.snapshot();
        let events = app.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            insert_app(tx, &s).await?;
            insert_events(tx, &events, Some(Version::NEW.next()), stamp).await
        })?;
        app.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, app: &mut App) -> Result<(), RepoError> {
        stamp_update(app, self.now());
        let s = app.snapshot();
        let events = app.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_app(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        app.set_version(version);
        Ok(())
    }

    async fn delete(&self, key: &AppKey) -> Result<(), RepoError> {
        let key = key.as_str().to_string();
        in_transaction!(self, |tx| {
            // Its rate settings, grant and tokens go with it (ON DELETE CASCADE).
            tx.query_typed("DELETE FROM apps WHERE key = $1", &[(&key, Type::TEXT)])
                .await
                .map_err(repo_error)?;
            // And its deliveries, so an app made later under the key starts clean.
            tx.query_typed(
                "DELETE FROM event_deliveries WHERE destination = 'app:' || $1",
                &[(&key, Type::TEXT)],
            )
            .await
            .map_err(repo_error)?;
            Ok(())
        })
    }
}

/// The manifest as JSON text for `$n::text::jsonb` (`None`: no manifest).
fn manifest_text(s: &AppSnapshot) -> Option<String> {
    s.manifest.as_ref().map(|m| m.to_string())
}

/// `AppRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_app(tx: &impl Queries, s: &AppSnapshot) -> Result<(), RepoError> {
    tx.query_typed(
        &format!(
            "INSERT INTO apps (key, id, version, name, enabled, secret, webhook_url,
                 webhook_timeout_ms, subscriptions, manifest_url, manifest, manifest_fetched_at,
                 publisher, privacy_url, uninstalled_at)
             VALUES ($1, $2, 1, $3, $4, $5, $6, $7, $8, $9, $10::text::jsonb, {}, $12, $13, {})",
            ts(11),
            ts(14)
        ),
        &[
            (&s.key, Type::TEXT),
            (&s.id.as_uuid(), Type::UUID),
            (&s.name, Type::TEXT),
            (&s.enabled, Type::BOOL),
            (&s.secret.as_bytes(), Type::BYTEA),
            (&s.webhook_url, Type::TEXT),
            (&timeout_to_db(s.webhook_timeout_ms)?, Type::INT4),
            (&s.subscriptions, Type::TEXT_ARRAY),
            (&s.manifest_url, Type::TEXT),
            (&manifest_text(s), Type::TEXT),
            (&opt_micros(s.manifest_fetched_at), Type::INT8),
            (&s.publisher, Type::TEXT),
            (&s.privacy_url, Type::TEXT),
            (&opt_micros(s.uninstalled_at), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_grant(tx, &s.key, s.grant.as_ref()).await?;
    write_stamps(tx, "apps", "key", &s.key, Type::TEXT, s.stamps).await
}

/// `AppRepo::update` inside the caller's transaction; the new version.
pub(crate) async fn update_app(tx: &impl Queries, s: &AppSnapshot) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            &format!(
                "UPDATE apps SET version = version + 1, name = $3, enabled = $4, secret = $5,
                     webhook_url = $6, webhook_timeout_ms = $7, subscriptions = $8,
                     manifest_url = $9, manifest = $10::text::jsonb, manifest_fetched_at = {},
                     publisher = $12, privacy_url = $13, uninstalled_at = {}
                 WHERE key = $1 AND version = $2 RETURNING version",
                ts(11),
                ts(14)
            ),
            &[
                (&s.key, Type::TEXT),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&s.enabled, Type::BOOL),
                (&s.secret.as_bytes(), Type::BYTEA),
                (&s.webhook_url, Type::TEXT),
                (&timeout_to_db(s.webhook_timeout_ms)?, Type::INT4),
                (&s.subscriptions, Type::TEXT_ARRAY),
                (&s.manifest_url, Type::TEXT),
                (&manifest_text(s), Type::TEXT),
                (&opt_micros(s.manifest_fetched_at), Type::INT8),
                (&s.publisher, Type::TEXT),
                (&s.privacy_url, Type::TEXT),
                (&opt_micros(s.uninstalled_at), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    let version = version_from_db(col(&row, "version")?)?;
    write_grant(tx, &s.key, s.grant.as_ref()).await?;
    write_stamps(tx, "apps", "key", &s.key, Type::TEXT, s.stamps).await?;
    Ok(version)
}

/// Replaces the app's grant row with `grant` (none: no row).
async fn write_grant(
    tx: &impl Queries,
    key: &str,
    grant: Option<&GrantSnapshot>,
) -> Result<(), RepoError> {
    let key = key.to_string();
    tx.query_typed(
        "DELETE FROM app_grants WHERE app_key = $1",
        &[(&key, Type::TEXT)],
    )
    .await
    .map_err(repo_error)?;
    let Some(grant) = grant else {
        return Ok(());
    };
    let (kind, id) = actor_to_db(grant.approved_by);
    tx.query_typed(
        &format!(
            "INSERT INTO app_grants (app_key, scopes, approved_by_kind, approved_by_id,
                 approved_at)
             VALUES ($1, $2, $3, $4, {})",
            ts(5)
        ),
        &[
            (&key, Type::TEXT),
            (&scopes_to_db(&grant.scopes), Type::TEXT_ARRAY),
            (&kind, Type::TEXT),
            (&id, Type::UUID),
            (&grant.approved_at.unix_micros(), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}
