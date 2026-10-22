use hypha_core::{Actor, IdempotencyKey, IdempotencyRecord, IdempotencyRepo, RepoError, Timestamp};
use tokio_postgres::types::Type;
use uuid::Uuid;

use crate::{
    columns::{actor_to_db, col, micros, ts},
    error::{corrupt, repo_error},
    store::Queries,
    store::{PgConnection, PgStore, in_transaction},
};

impl<C: PgConnection> IdempotencyRepo for PgStore<C> {
    async fn get(
        &self,
        actor: Actor,
        key: &IdempotencyKey,
    ) -> Result<Option<IdempotencyRecord>, RepoError> {
        let (kind, id) = actor_to_db(actor);
        let mut conn = self.conn().await;
        // One query, outside a transaction: consistent on its own. It must not be served
        // from a cache (a cached miss would run the command twice), which is why Hyperdrive
        // is always created with its query cache off, as session and key lookups need too.
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT operation, fingerprint, result, {}
                     FROM idempotency_keys
                     WHERE actor_kind = $1 AND actor_id = $2 AND key = $3",
                    micros("created_at"),
                ),
                &[
                    (&kind, Type::TEXT),
                    (&id, Type::UUID),
                    (&key.as_str(), Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = row else { return Ok(None) };
        let fingerprint: Vec<u8> = col(&row, "fingerprint")?;
        Ok(Some(IdempotencyRecord {
            actor,
            key: key.clone(),
            operation: col(&row, "operation")?,
            fingerprint: fingerprint
                .try_into()
                .map_err(|_| corrupt("idempotency fingerprint is not 32 bytes"))?,
            result: col(&row, "result")?,
            created_at: Timestamp::from_unix_micros(col(&row, "created_at")?),
        }))
    }

    async fn prune(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError> {
        let before = before.unix_micros();
        in_transaction!(self, |tx| {
            // `idempotency_keys_created_at` (migration) serves this.
            let deleted = tx
                .execute_typed(
                    &format!(
                        "DELETE FROM idempotency_keys WHERE (actor_kind, actor_id, key) IN (
                             SELECT actor_kind, actor_id, key FROM idempotency_keys
                             WHERE created_at < {} LIMIT $2
                         )",
                        ts(1)
                    ),
                    &[(&before, Type::INT8), (&i64::from(limit), Type::INT8)],
                )
                .await
                .map_err(repo_error)?;
            u32::try_from(deleted).map_err(corrupt)
        })
    }
}

/// Inserts `records` in one statement (inside `PgBatch`'s transaction). A key the
/// actor already used fails on the `idempotency_key` constraint.
pub(crate) async fn insert_records(
    tx: &impl Queries,
    records: &[IdempotencyRecord],
) -> Result<(), RepoError> {
    if records.is_empty() {
        return Ok(());
    }
    let (kinds, ids): (Vec<&str>, Vec<Uuid>) = records.iter().map(|r| actor_to_db(r.actor)).unzip();
    let keys: Vec<&str> = records.iter().map(|r| r.key.as_str()).collect();
    let operations: Vec<&str> = records.iter().map(|r| r.operation.as_str()).collect();
    let fingerprints: Vec<&[u8]> = records.iter().map(|r| r.fingerprint.as_slice()).collect();
    let results: Vec<Uuid> = records.iter().map(|r| r.result).collect();
    let created: Vec<i64> = records.iter().map(|r| r.created_at.unix_micros()).collect();
    tx.query_typed(
        "INSERT INTO idempotency_keys
            (actor_kind, actor_id, key, operation, fingerprint, result, created_at)
         SELECT kind, id, key, operation, fingerprint, result,
                'epoch'::timestamptz + micros * interval '1 microsecond'
         FROM unnest($1::text[], $2::uuid[], $3::text[], $4::text[], $5::bytea[], $6::uuid[],
                     $7::bigint[])
              AS t (kind, id, key, operation, fingerprint, result, micros)",
        &[
            (&kinds, Type::TEXT_ARRAY),
            (&ids, Type::UUID_ARRAY),
            (&keys, Type::TEXT_ARRAY),
            (&operations, Type::TEXT_ARRAY),
            (&fingerprints, Type::BYTEA_ARRAY),
            (&results, Type::UUID_ARRAY),
            (&created, Type::INT8_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}
