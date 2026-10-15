//! The outbox (design note 0016, section 3): events written in the transaction of the
//! change that recorded them.

use hypha_core::{
    ActingAs, Actor, OutboxRepo, PendingEvent, RepoError, StoredEvent, Timestamp, Version,
};
use tokio_postgres::{GenericClient, types::Type};
use uuid::Uuid;

use crate::{
    columns::{actor_from_db, actor_to_db, col, micros, version_from_db, version_to_db},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore},
};

/// Writes `events` in one statement, inside the caller's transaction, stamped with
/// `actor`. `subject_version` is the version the write gives the aggregate the events are
/// about (`None` for events without one). ID and time come from the database.
pub(crate) async fn insert_events(
    tx: &impl GenericClient,
    events: &[PendingEvent],
    subject_version: Option<Version>,
    actor: Actor,
) -> Result<(), RepoError> {
    if events.is_empty() {
        return Ok(());
    }
    let (actor_kind, actor_id) = actor_to_db(actor);
    let types: Vec<&str> = events.iter().map(|e| e.event_type).collect();
    let versions: Vec<i16> = events
        .iter()
        .map(|e| i16::try_from(e.version).map_err(corrupt))
        .collect::<Result<_, _>>()?;
    let subject_types: Vec<&str> = events.iter().map(|e| e.subject.kind).collect();
    let subject_ids: Vec<Uuid> = events.iter().map(|e| e.subject.id).collect();
    let subject_version = subject_version.map(version_to_db).transpose()?;
    // jsonb arrays don't bind as `jsonb[]` from serde values; text cast back is exact.
    let data: Vec<String> = events.iter().map(|e| e.data.to_string()).collect();
    tx.query_typed(
        "INSERT INTO outbox_events
            (type, version, subject_type, subject_id, subject_version, actor_kind, actor_id,
             data)
         SELECT t, v, st, si, $5, $6, $7, d::jsonb
         FROM unnest($1::text[], $2::smallint[], $3::text[], $4::uuid[], $8::text[])
              WITH ORDINALITY AS e (t, v, st, si, d, n)
         ORDER BY n",
        &[
            (&types, Type::TEXT_ARRAY),
            (&versions, Type::INT2_ARRAY),
            (&subject_types, Type::TEXT_ARRAY),
            (&subject_ids, Type::UUID_ARRAY),
            (&subject_version, Type::INT8),
            (&actor_kind, Type::TEXT),
            (&actor_id, Type::UUID),
            (&data, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

impl<C: PgConnection> ActingAs for PgStore<C> {
    fn act_as(&self, actor: Actor) {
        self.set_actor(actor);
    }
}

impl<C: PgConnection> OutboxRepo for PgStore<C> {
    async fn events_after(&self, after: i64, limit: u32) -> Result<Vec<StoredEvent>, RepoError> {
        let mut conn = self.conn().await;
        let rows = conn
            .client()
            .query_typed(
                &format!(
                    "SELECT seq, id, type, version, subject_type, subject_id, subject_version,
                         actor_kind, actor_id, data::text AS data, {}
                     FROM outbox_events WHERE seq > $1 ORDER BY seq LIMIT $2",
                    micros("occurred_at"),
                ),
                &[(&after, Type::INT8), (&i64::from(limit), Type::INT8)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter()
            .map(|row| {
                let version: i16 = col(row, "version")?;
                let data: String = col(row, "data")?;
                Ok(StoredEvent {
                    seq: col(row, "seq")?,
                    id: col(row, "id")?,
                    event_type: col(row, "type")?,
                    version: u16::try_from(version).map_err(corrupt)?,
                    subject_kind: col(row, "subject_type")?,
                    subject_id: col(row, "subject_id")?,
                    subject_version: col::<Option<i64>>(row, "subject_version")?
                        .map(version_from_db)
                        .transpose()?,
                    actor: actor_from_db(col(row, "actor_kind")?, col(row, "actor_id")?)?,
                    occurred_at: Timestamp::from_unix_micros(col(row, "occurred_at")?),
                    data: serde_json::from_str(&data).map_err(corrupt)?,
                })
            })
            .collect()
    }
}
