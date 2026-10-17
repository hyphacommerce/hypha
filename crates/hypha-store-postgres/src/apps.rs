//! External apps (design note 0016, section 8).

use hypha_apps::{App, AppKey, AppRepo, AppSnapshot};
use hypha_core::{RepoError, Sealed, Version, Versioned, stamp_insert, stamp_update};
use tokio_postgres::{Row, types::Type};

use crate::{
    columns::{col, stamp_columns, stamps_from_row, version_from_db, version_to_db, write_stamps},
    error::{corrupt, repo_error},
    store::Queries,
    store::{PgConnection, PgStore, in_transaction},
};

fn columns() -> String {
    format!(
        "key, version, name, enabled, secret, webhook_url, webhook_timeout_ms, subscriptions, {}",
        stamp_columns("")
    )
}

fn app_from_row(row: &Row) -> Result<App, RepoError> {
    let key: String = col(row, "key")?;
    let timeout: Option<i32> = col(row, "webhook_timeout_ms")?;
    App::restore(AppSnapshot {
        key: key.clone(),
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        name: col(row, "name")?,
        enabled: col(row, "enabled")?,
        secret: Sealed::from_bytes(col(row, "secret")?),
        webhook_url: col(row, "webhook_url")?,
        webhook_timeout_ms: timeout.map(u32::try_from).transpose().map_err(corrupt)?,
        subscriptions: col(row, "subscriptions")?,
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
                &format!("SELECT {} FROM apps WHERE key = $1", columns()),
                &[(&key.as_str(), Type::TEXT)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(app_from_row).transpose()
    }

    async fn list(&self) -> Result<Vec<App>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(&format!("SELECT {} FROM apps ORDER BY key", columns()), &[])
            .await
            .map_err(repo_error)?
            .iter()
            .map(app_from_row)
            .collect()
    }

    async fn insert(&self, app: &mut App) -> Result<(), RepoError> {
        stamp_insert(app, self.now());
        let s = app.snapshot();
        in_transaction!(self, |tx| insert_app(tx, &s).await)?;
        app.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, app: &mut App) -> Result<(), RepoError> {
        stamp_update(app, self.now());
        let s = app.snapshot();
        let version = in_transaction!(self, |tx| update_app(tx, &s).await)?;
        app.set_version(version);
        Ok(())
    }

    async fn delete(&self, key: &AppKey) -> Result<(), RepoError> {
        let key = key.as_str().to_string();
        in_transaction!(self, |tx| {
            // Its rate settings go with it (`shipping_provider_app`, ON DELETE CASCADE).
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

/// `AppRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_app(tx: &impl Queries, s: &AppSnapshot) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO apps (key, version, name, enabled, secret, webhook_url,
             webhook_timeout_ms, subscriptions)
         VALUES ($1, 1, $2, $3, $4, $5, $6, $7)",
        &[
            (&s.key, Type::TEXT),
            (&s.name, Type::TEXT),
            (&s.enabled, Type::BOOL),
            (&s.secret.as_bytes(), Type::BYTEA),
            (&s.webhook_url, Type::TEXT),
            (&timeout_to_db(s.webhook_timeout_ms)?, Type::INT4),
            (&s.subscriptions, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_stamps(tx, "apps", "key", &s.key, Type::TEXT, s.stamps).await
}

/// `AppRepo::update` inside the caller's transaction; the new version.
pub(crate) async fn update_app(tx: &impl Queries, s: &AppSnapshot) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE apps SET version = version + 1, name = $3, enabled = $4, secret = $5,
                 webhook_url = $6, webhook_timeout_ms = $7, subscriptions = $8
             WHERE key = $1 AND version = $2 RETURNING version",
            &[
                (&s.key, Type::TEXT),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&s.enabled, Type::BOOL),
                (&s.secret.as_bytes(), Type::BYTEA),
                (&s.webhook_url, Type::TEXT),
                (&timeout_to_db(s.webhook_timeout_ms)?, Type::INT4),
                (&s.subscriptions, Type::TEXT_ARRAY),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    let version = version_from_db(col(&row, "version")?)?;
    write_stamps(tx, "apps", "key", &s.key, Type::TEXT, s.stamps).await?;
    Ok(version)
}
