//! Apps' extension endpoints (design note 0029, `0051_app_endpoints.sql`): one row per
//! point an app answers, replaced all at once.

use hypha_apps::{ExtensionEndpoint, ExtensionEndpointRepo};
use hypha_core::{AppKey, CallbackUrl, RepoError};
use tokio_postgres::{Row, types::Type};

use crate::{
    columns::col,
    error::{corrupt, repo_error},
    store::Queries,
    store::{PgConnection, PgStore, in_transaction},
};

fn from_row(row: &Row) -> Result<ExtensionEndpoint, RepoError> {
    let version: i32 = col(row, "version")?;
    let url: String = col(row, "url")?;
    Ok(ExtensionEndpoint {
        point: col(row, "point")?,
        version: u16::try_from(version).map_err(corrupt)?,
        url: CallbackUrl::new(&url).map_err(corrupt)?,
    })
}

impl<C: PgConnection> ExtensionEndpointRepo for PgStore<C> {
    async fn list_for_app(&self, app: &AppKey) -> Result<Vec<ExtensionEndpoint>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "SELECT point, version, url FROM app_extension_endpoints
                 WHERE app = $1 ORDER BY point",
                &[(&app.as_str(), Type::TEXT)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(from_row)
            .collect()
    }

    async fn get(&self, app: &AppKey, point: &str) -> Result<Option<ExtensionEndpoint>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                "SELECT point, version, url FROM app_extension_endpoints
                 WHERE app = $1 AND point = $2",
                &[(&app.as_str(), Type::TEXT), (&point, Type::TEXT)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(from_row).transpose()
    }

    async fn replace_for_app(
        &self,
        app: &AppKey,
        endpoints: &[ExtensionEndpoint],
    ) -> Result<(), RepoError> {
        let app = app.as_str().to_string();
        let points: Vec<&str> = endpoints.iter().map(|e| e.point.as_str()).collect();
        let versions: Vec<i32> = endpoints.iter().map(|e| i32::from(e.version)).collect();
        let urls: Vec<&str> = endpoints.iter().map(|e| e.url.as_str()).collect();
        in_transaction!(self, |tx| {
            // The app's row, locked, so a delete racing this one can't leave rows
            // behind and a missing app is the reference error even with no rows to add.
            let found = tx
                .query_typed_opt(
                    "SELECT key FROM apps WHERE key = $1 FOR UPDATE",
                    &[(&app, Type::TEXT)],
                )
                .await
                .map_err(repo_error)?;
            if found.is_none() {
                return Err(RepoError::Reference(
                    hypha_apps::endpoints::keys::ENDPOINT_APP,
                ));
            }
            tx.query_typed(
                "DELETE FROM app_extension_endpoints WHERE app = $1",
                &[(&app, Type::TEXT)],
            )
            .await
            .map_err(repo_error)?;
            tx.query_typed(
                "INSERT INTO app_extension_endpoints (app, point, version, url)
                 SELECT $1, * FROM unnest($2::text[], $3::int4[], $4::text[])",
                &[
                    (&app, Type::TEXT),
                    (&points, Type::TEXT_ARRAY),
                    (&versions, Type::INT4_ARRAY),
                    (&urls, Type::TEXT_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?;
            Ok(())
        })
    }
}
