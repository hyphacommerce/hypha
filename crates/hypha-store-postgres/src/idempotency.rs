use hypha_core::{Actor, IdempotencyKey, IdempotencyRecord, IdempotencyRepo, RepoError, Timestamp};
use tokio_postgres::{GenericClient, types::Type};
use uuid::Uuid;

use crate::{
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, consistent_read},
};

impl<C: PgConnection> IdempotencyRepo for PgStore<C> {
    async fn get(
        &self,
        actor: Actor,
        key: &IdempotencyKey,
    ) -> Result<Option<IdempotencyRecord>, RepoError> {
        let (kind, id) = actor_to_db(actor);
        let mut conn = self.conn().await;
        // One query, but inside a transaction anyway: a cached answer here (Hyperdrive's
        // query cache) would miss a key committed seconds ago and run the command twice.
        let row = consistent_read!(conn.client(), |tx| tx
            .query_typed_opt(
                "SELECT operation, fingerprint, result,
                        (extract(epoch FROM created_at) * 1000000)::bigint
                 FROM idempotency_keys
                 WHERE actor_kind = $1 AND actor_id = $2 AND key = $3",
                &[
                    (&kind, Type::TEXT),
                    (&id, Type::UUID),
                    (&key.as_str(), Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error))?;
        let Some(row) = row else { return Ok(None) };
        let fingerprint: Vec<u8> = row.get(1);
        Ok(Some(IdempotencyRecord {
            actor,
            key: key.clone(),
            operation: row.get(0),
            fingerprint: fingerprint
                .try_into()
                .map_err(|_| corrupt("idempotency fingerprint is not 32 bytes"))?,
            result: row.get(2),
            created_at: Timestamp::from_unix_micros(row.get(3)),
        }))
    }
}

/// Inserts `records` in one statement (inside `PgBatch`'s transaction). A key the
/// actor already used fails on the `idempotency_key` constraint.
pub(crate) async fn insert_records(
    tx: &impl GenericClient,
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

/// `actor_kind` and `actor_id`; the system actor has no ID and is stored as nil. No
/// reverse mapping is needed: `get` already knows the actor it asked for.
fn actor_to_db(actor: Actor) -> (&'static str, Uuid) {
    match actor {
        Actor::System => ("system", Uuid::nil()),
        Actor::User(id) => ("user", id.as_uuid()),
        Actor::Agent(id) => ("agent", id.as_uuid()),
        Actor::ApiKey(id) => ("api_key", id.as_uuid()),
    }
}
