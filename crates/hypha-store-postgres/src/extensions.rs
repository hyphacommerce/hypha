//! The extension call log (`0049_extension_calls.sql`): one row per call, written once.

use hypha_core::{AppKey, ExtensionCallId, Page, PageRequest, RepoError, Timestamp};
use hypha_extensions::{CallOutcome, ExtensionCall, ExtensionCallRepo};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{Param, col, micros, ts},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, Queries},
};

fn columns() -> String {
    format!(
        "id, app, point, version, {}, duration_ms, outcome, status, error, correlation",
        micros("started_at")
    )
}

fn from_row(row: &Row) -> Result<ExtensionCall, RepoError> {
    let id: Uuid = col(row, "id")?;
    let app: String = col(row, "app")?;
    let version: i32 = col(row, "version")?;
    let duration: i64 = col(row, "duration_ms")?;
    let outcome: String = col(row, "outcome")?;
    let status: Option<i32> = col(row, "status")?;
    Ok(ExtensionCall {
        id: ExtensionCallId::from_uuid(id),
        app: AppKey::new(&app).map_err(corrupt)?,
        point: col(row, "point")?,
        version: u16::try_from(version).map_err(corrupt)?,
        started_at: Timestamp::from_unix_micros(col(row, "started_at")?),
        duration_ms: u32::try_from(duration).map_err(corrupt)?,
        outcome: CallOutcome::parse(&outcome)
            .map_err(|o| corrupt(format!("unknown call outcome {o:?}")))?,
        status: status.map(u16::try_from).transpose().map_err(corrupt)?,
        error: col(row, "error")?,
        correlation: col(row, "correlation")?,
    })
}

impl<C: PgConnection> ExtensionCallRepo for PgStore<C> {
    async fn insert(&self, call: &ExtensionCall) -> Result<(), RepoError> {
        let id = call.id.as_uuid();
        let version = i32::from(call.version);
        let started = call.started_at.unix_micros();
        let duration = i64::from(call.duration_ms);
        let status = call.status.map(i32::from);
        let mut conn = self.conn().await;
        conn.db()
            .execute_typed(
                &format!(
                    "INSERT INTO extension_calls (id, app, point, version, started_at,
                         duration_ms, outcome, status, error, correlation)
                     VALUES ($1, $2, $3, $4, {}, $6, $7, $8, $9, $10)",
                    ts(5)
                ),
                &[
                    (&id, Type::UUID),
                    (&call.app.as_str(), Type::TEXT),
                    (&call.point, Type::TEXT),
                    (&version, Type::INT4),
                    (&started, Type::INT8),
                    (&duration, Type::INT8),
                    (&call.outcome.as_str(), Type::TEXT),
                    (&status, Type::INT4),
                    (&call.error, Type::TEXT),
                    (&call.correlation, Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }

    async fn list_for_app(
        &self,
        app: &AppKey,
        page: PageRequest<ExtensionCallId>,
    ) -> Result<Page<ExtensionCall>, RepoError> {
        let key = app.as_str();
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut params: Vec<Param<'_>> = vec![(&key, Type::TEXT)];
        let mut after_condition = String::new();
        if let Some(after) = &after {
            params.push((after, Type::UUID));
            after_condition = format!("AND id < ${}", params.len());
        }
        params.push((&fetch, Type::INT8));
        let mut conn = self.conn().await;
        // `extension_calls_by_app` serves this.
        let found = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM extension_calls WHERE app = $1 {after_condition}
                     ORDER BY id DESC LIMIT ${}",
                    columns(),
                    params.len()
                ),
                &params,
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn prune(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError> {
        let before = before.unix_micros();
        let limit = i64::from(limit);
        let mut conn = self.conn().await;
        // `extension_calls_started_at` serves the inner scan.
        let deleted = conn
            .db()
            .execute_typed(
                &format!(
                    "DELETE FROM extension_calls WHERE id IN (
                         SELECT id FROM extension_calls WHERE started_at < {} LIMIT $2
                     )",
                    ts(1)
                ),
                &[(&before, Type::INT8), (&limit, Type::INT8)],
            )
            .await
            .map_err(repo_error)?;
        u32::try_from(deleted).map_err(corrupt)
    }
}
