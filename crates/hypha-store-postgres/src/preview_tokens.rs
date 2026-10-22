//! Preview tokens (`0070_preview_tokens.sql`).

use hypha_core::{PreviewToken, PreviewTokenRepo, RepoError, Timestamp};
use tokio_postgres::types::Type;
use uuid::Uuid;

use crate::{
    columns::{actor_from_db, actor_to_db, col, micros, ts},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, Queries},
};

impl<C: PgConnection> PreviewTokenRepo for PgStore<C> {
    async fn insert(&self, token: &PreviewToken) -> Result<(), RepoError> {
        let mut conn = self.conn().await;
        insert_token(&conn.db(), token).await
    }

    async fn get(&self, hash: &[u8; 32]) -> Result<Option<PreviewToken>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT actor_kind, actor_id, request, ids, digest, {}, {}
                     FROM preview_tokens WHERE hash = $1",
                    micros("expires_at"),
                    micros("used_at"),
                ),
                &[(&hash.as_slice(), Type::BYTEA)],
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = row else { return Ok(None) };
        let kind: String = col(&row, "actor_kind")?;
        let request: Vec<u8> = col(&row, "request")?;
        Ok(Some(PreviewToken {
            hash: *hash,
            actor: actor_from_db(&kind, col(&row, "actor_id")?)?,
            request: request
                .try_into()
                .map_err(|_| corrupt("a preview token's request is not 32 bytes"))?,
            ids: col(&row, "ids")?,
            digest: col(&row, "digest")?,
            expires_at: Timestamp::from_unix_micros(col(&row, "expires_at")?),
            used_at: col::<Option<i64>>(&row, "used_at")?.map(Timestamp::from_unix_micros),
        }))
    }

    async fn mark_used(&self, hash: &[u8; 32], at: Timestamp) -> Result<bool, RepoError> {
        let mut conn = self.conn().await;
        // One statement: of two writes racing with one token, the second waits on the
        // first's row lock, then finds it used (or unused again, if the first rolled back).
        let marked = conn
            .db()
            .execute_typed(
                &format!(
                    "UPDATE preview_tokens SET used_at = {}
                     WHERE hash = $1 AND used_at IS NULL",
                    ts(2)
                ),
                &[
                    (&hash.as_slice(), Type::BYTEA),
                    (&at.unix_micros(), Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        Ok(marked == 1)
    }

    async fn prune(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError> {
        let mut conn = self.conn().await;
        let deleted = conn
            .db()
            .execute_typed(
                &format!(
                    "DELETE FROM preview_tokens WHERE hash IN (
                         SELECT hash FROM preview_tokens WHERE expires_at < {} LIMIT $2
                     )",
                    ts(1)
                ),
                &[
                    (&before.unix_micros(), Type::INT8),
                    (&i64::from(limit), Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        u32::try_from(deleted).map_err(corrupt)
    }
}

/// Inserts `token`: through the repository, and when a preview ends (`PgPreview`), after
/// its rollback.
pub(crate) async fn insert_token(db: &impl Queries, token: &PreviewToken) -> Result<(), RepoError> {
    let (kind, id) = actor_to_db(token.actor);
    let ids: &[Uuid] = &token.ids;
    db.execute_typed(
        &format!(
            "INSERT INTO preview_tokens
                (hash, actor_kind, actor_id, request, ids, digest, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, {})",
            ts(7)
        ),
        &[
            (&token.hash.as_slice(), Type::BYTEA),
            (&kind, Type::TEXT),
            (&id, Type::UUID),
            (&token.request.as_slice(), Type::BYTEA),
            (&ids, Type::UUID_ARRAY),
            (&token.digest.as_str(), Type::TEXT),
            (&token.expires_at.unix_micros(), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}
