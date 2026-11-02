//! Spores' settings: one row per spore.

use hypha_apps::{AppKey, SporeSettings, SporeSettingsRepo};
use hypha_core::{RepoError, Stamped, Version, Versioned, stamp_insert, stamp_update};
use tokio_postgres::types::Type;

use crate::{
    columns::{col, stamp_columns, stamps_from_row, version_from_db, version_to_db, write_stamps},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, Queries, in_transaction},
};

impl<C: PgConnection> SporeSettingsRepo for PgStore<C> {
    async fn get(&self, key: &AppKey) -> Result<Option<SporeSettings>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT version, settings::text AS settings, {}
                     FROM built_in_settings WHERE key = $1",
                    stamp_columns("")
                ),
                &[(&key.as_str(), Type::TEXT)],
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = row else { return Ok(None) };
        let text: String = col(&row, "settings")?;
        let Ok(serde_json::Value::Object(values)) = serde_json::from_str(&text) else {
            return Err(corrupt(format!("spore {key}'s settings aren't an object")));
        };
        Ok(Some(SporeSettings::restore(
            key.clone(),
            values,
            version_from_db(col(&row, "version")?)?,
            stamps_from_row(&row)?,
        )))
    }

    async fn insert(&self, settings: &mut SporeSettings) -> Result<(), RepoError> {
        stamp_insert(settings, self.now());
        let snapshot = settings.clone();
        in_transaction!(self, |tx| insert(tx, &snapshot).await)?;
        settings.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, settings: &mut SporeSettings) -> Result<(), RepoError> {
        stamp_update(settings, self.now());
        let snapshot = settings.clone();
        let version = in_transaction!(self, |tx| update(tx, &snapshot).await)?;
        settings.set_version(version);
        Ok(())
    }
}

fn json(settings: &SporeSettings) -> String {
    serde_json::Value::Object(settings.values().clone()).to_string()
}

async fn insert(tx: &impl Queries, s: &SporeSettings) -> Result<(), RepoError> {
    let key = s.key().as_str();
    tx.query_typed(
        "INSERT INTO built_in_settings (key, version, settings) VALUES ($1, 1, $2::text::jsonb)",
        &[(&key, Type::TEXT), (&json(s), Type::TEXT)],
    )
    .await
    .map_err(repo_error)?;
    write_stamps(tx, "built_in_settings", "key", &key, Type::TEXT, s.stamps()).await
}

async fn update(tx: &impl Queries, s: &SporeSettings) -> Result<Version, RepoError> {
    let key = s.key().as_str();
    let row = tx
        .query_typed_opt(
            "UPDATE built_in_settings SET version = version + 1, settings = $3::text::jsonb
             WHERE key = $1 AND version = $2 RETURNING version",
            &[
                (&key, Type::TEXT),
                (&version_to_db(s.version())?, Type::INT8),
                (&json(s), Type::TEXT),
            ],
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    let version = version_from_db(col(&row, "version")?)?;
    write_stamps(tx, "built_in_settings", "key", &key, Type::TEXT, s.stamps()).await?;
    Ok(version)
}
