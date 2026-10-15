//! The outbox (design note 0016, section 3): events written in the transaction of the
//! change that recorded them.

use hypha_core::{
    ActingAs, Actor, Delivery, DeliveryOutcome, DeliveryRecord, DeliveryStatus, OutboxRepo, Page,
    PageRequest, PendingEvent, RepoError, StoredEvent, Timestamp, Version,
};
use tokio_postgres::{GenericClient, Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{actor_from_db, actor_to_db, col, micros, ts, version_from_db, version_to_db},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, in_transaction},
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

/// `outbox_events`' columns, as `event_from_row` reads them.
fn event_columns() -> String {
    format!(
        "seq, id, type, version, subject_type, subject_id, subject_version, actor_kind,
         actor_id, data::text AS data, {}",
        micros("occurred_at")
    )
}

fn event_from_row(row: &Row) -> Result<StoredEvent, RepoError> {
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
}

/// A delivery joined with its event (`d` and `e`), as `delivery_from_row` reads it.
fn delivery_columns() -> String {
    format!(
        "d.id, d.event_seq, e.id AS event_id, e.type, {}, d.destination, d.status,
         d.attempts, {}, d.last_error",
        micros("e.occurred_at"),
        micros("d.next_attempt_at")
    )
}

fn delivery_from_row(row: &Row) -> Result<DeliveryRecord, RepoError> {
    let status: &str = col(row, "status")?;
    let attempts: i32 = col(row, "attempts")?;
    Ok(DeliveryRecord {
        id: col(row, "id")?,
        event_seq: col(row, "event_seq")?,
        event_id: col(row, "event_id")?,
        event_type: col(row, "type")?,
        occurred_at: Timestamp::from_unix_micros(col(row, "occurred_at")?),
        destination: col(row, "destination")?,
        status: DeliveryStatus::parse(status)
            .ok_or_else(|| corrupt(format!("delivery status {status:?}")))?,
        attempts: u32::try_from(attempts).map_err(corrupt)?,
        next_attempt_at: Timestamp::from_unix_micros(col(row, "next_attempt_at")?),
        last_error: col(row, "last_error")?,
    })
}

impl<C: PgConnection> OutboxRepo for PgStore<C> {
    async fn events_after(&self, after: i64, limit: u32) -> Result<Vec<StoredEvent>, RepoError> {
        let mut conn = self.conn().await;
        let rows = conn
            .client()
            .query_typed(
                &format!(
                    "SELECT {} FROM outbox_events WHERE seq > $1 ORDER BY seq LIMIT $2",
                    event_columns()
                ),
                &[(&after, Type::INT8), (&i64::from(limit), Type::INT8)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter().map(event_from_row).collect()
    }

    async fn fan_out(
        &self,
        limit: u32,
        now: Timestamp,
        destinations: &(dyn Fn(&str) -> Vec<String> + Sync),
    ) -> Result<u32, RepoError> {
        let now = now.unix_micros();
        in_transaction!(self, |tx| {
            // SKIP LOCKED: two dispatchers split the work instead of waiting on each other.
            let claimed = tx
                .query_typed(
                    "SELECT seq, type FROM outbox_events WHERE fanned_out_at IS NULL
                     ORDER BY seq LIMIT $1 FOR UPDATE SKIP LOCKED",
                    &[(&i64::from(limit), Type::INT8)],
                )
                .await
                .map_err(repo_error)?;
            if claimed.is_empty() {
                return Ok(0);
            }
            let mut seqs = Vec::with_capacity(claimed.len());
            let (mut delivery_seqs, mut targets) = (Vec::new(), Vec::new());
            for row in &claimed {
                let seq: i64 = col(row, "seq")?;
                let event_type: &str = col(row, "type")?;
                seqs.push(seq);
                for destination in destinations(event_type) {
                    delivery_seqs.push(seq);
                    targets.push(destination);
                }
            }
            if !targets.is_empty() {
                tx.query_typed(
                    &format!(
                        "INSERT INTO event_deliveries (event_seq, destination, status,
                             next_attempt_at)
                         SELECT s, d, 'pending', {}
                         FROM unnest($1::bigint[], $2::text[]) AS t (s, d)
                         ON CONFLICT ON CONSTRAINT event_delivery_destination DO NOTHING",
                        ts(3)
                    ),
                    &[
                        (&delivery_seqs, Type::INT8_ARRAY),
                        (&targets, Type::TEXT_ARRAY),
                        (&now, Type::INT8),
                    ],
                )
                .await
                .map_err(repo_error)?;
            }
            tx.query_typed(
                &format!(
                    "UPDATE outbox_events SET fanned_out_at = {} WHERE seq = ANY($1)",
                    ts(2)
                ),
                &[(&seqs, Type::INT8_ARRAY), (&now, Type::INT8)],
            )
            .await
            .map_err(repo_error)?;
            u32::try_from(seqs.len()).map_err(corrupt)
        })
    }

    async fn lease_due(
        &self,
        now: Timestamp,
        lease_until: Timestamp,
        limit: u32,
    ) -> Result<Vec<Delivery>, RepoError> {
        let (now, until) = (now.unix_micros(), lease_until.unix_micros());
        in_transaction!(self, |tx| {
            let leased = tx
                .query_typed(
                    &format!(
                        "UPDATE event_deliveries d SET leased_until = {until}
                         FROM (
                             SELECT id FROM event_deliveries
                             WHERE status = 'pending' AND next_attempt_at <= {now}
                               AND (leased_until IS NULL OR leased_until <= {now})
                             ORDER BY next_attempt_at LIMIT $3 FOR UPDATE SKIP LOCKED
                         ) due
                         WHERE d.id = due.id
                         RETURNING d.id, d.event_seq, d.destination, d.attempts",
                        now = ts(1),
                        until = ts(2),
                    ),
                    &[
                        (&now, Type::INT8),
                        (&until, Type::INT8),
                        (&i64::from(limit), Type::INT8),
                    ],
                )
                .await
                .map_err(repo_error)?;
            if leased.is_empty() {
                return Ok(Vec::new());
            }
            let seqs: Vec<i64> = leased
                .iter()
                .map(|r| col(r, "event_seq"))
                .collect::<Result<_, _>>()?;
            let events = tx
                .query_typed(
                    &format!(
                        "SELECT {} FROM outbox_events WHERE seq = ANY($1)",
                        event_columns()
                    ),
                    &[(&seqs, Type::INT8_ARRAY)],
                )
                .await
                .map_err(repo_error)?
                .iter()
                .map(event_from_row)
                .collect::<Result<Vec<_>, _>>()?;
            leased
                .iter()
                .map(|row| {
                    let seq: i64 = col(row, "event_seq")?;
                    let attempts: i32 = col(row, "attempts")?;
                    let event = events
                        .iter()
                        .find(|e| e.seq == seq)
                        .cloned()
                        .ok_or_else(|| corrupt(format!("delivery of missing event {seq}")))?;
                    Ok(Delivery {
                        id: col(row, "id")?,
                        event,
                        destination: col(row, "destination")?,
                        attempts: u32::try_from(attempts).map_err(corrupt)?,
                    })
                })
                .collect()
        })
    }

    async fn record_outcomes(&self, outcomes: &[(Uuid, DeliveryOutcome)]) -> Result<(), RepoError> {
        if outcomes.is_empty() {
            return Ok(());
        }
        let ids: Vec<Uuid> = outcomes.iter().map(|(id, _)| *id).collect();
        let statuses: Vec<&str> = outcomes
            .iter()
            .map(|(_, o)| match o {
                DeliveryOutcome::Succeeded => "succeeded",
                DeliveryOutcome::Retry { .. } => "pending",
                DeliveryOutcome::Failed { .. } => "failed",
            })
            .collect();
        let errors: Vec<Option<&str>> = outcomes
            .iter()
            .map(|(_, o)| match o {
                DeliveryOutcome::Succeeded => None,
                DeliveryOutcome::Retry { error, .. } | DeliveryOutcome::Failed { error } => {
                    Some(error.as_str())
                }
            })
            .collect();
        let retry_at: Vec<Option<i64>> = outcomes
            .iter()
            .map(|(_, o)| match o {
                DeliveryOutcome::Retry { at, .. } => Some(at.unix_micros()),
                _ => None,
            })
            .collect();
        in_transaction!(self, |tx| {
            tx.query_typed(
                "UPDATE event_deliveries d
                 SET attempts = d.attempts + 1, leased_until = NULL, status = t.status,
                     last_error = t.error,
                     next_attempt_at = CASE WHEN t.at IS NULL THEN d.next_attempt_at
                         ELSE 'epoch'::timestamptz + t.at * interval '1 microsecond' END
                 FROM unnest($1::uuid[], $2::text[], $3::text[], $4::bigint[])
                      AS t (id, status, error, at)
                 WHERE d.id = t.id",
                &[
                    (&ids, Type::UUID_ARRAY),
                    (&statuses, Type::TEXT_ARRAY),
                    (&errors, Type::TEXT_ARRAY),
                    (&retry_at, Type::INT8_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?;
            Ok(())
        })
    }

    async fn prune_events(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError> {
        let before = before.unix_micros();
        in_transaction!(self, |tx| {
            let deleted = tx
                .execute_typed(
                    &format!(
                        "DELETE FROM outbox_events WHERE seq IN (
                             SELECT seq FROM outbox_events
                             WHERE occurred_at < {} AND fanned_out_at IS NOT NULL
                             ORDER BY seq LIMIT $2
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

    async fn get_event(&self, id: Uuid) -> Result<Option<StoredEvent>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_opt(
                &format!(
                    "SELECT {} FROM outbox_events WHERE id = $1",
                    event_columns()
                ),
                &[&id],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(event_from_row).transpose()
    }

    async fn deliveries_to(
        &self,
        destination: &str,
        status: Option<DeliveryStatus>,
        page: PageRequest<i64>,
    ) -> Result<Page<DeliveryRecord>, RepoError> {
        let status = status.map(DeliveryStatus::as_str);
        let mut conn = self.conn().await;
        // `event_delivery_destination` (event_seq, destination) is unique, so a
        // destination's deliveries are ordered by `event_seq` without ties.
        let rows = conn
            .client()
            .query_typed(
                &format!(
                    "SELECT {} FROM event_deliveries d
                     JOIN outbox_events e ON e.seq = d.event_seq
                     WHERE d.destination = $1
                       AND ($2::text IS NULL OR d.status = $2)
                       AND ($3::bigint IS NULL OR d.event_seq < $3)
                     ORDER BY d.event_seq DESC LIMIT $4",
                    delivery_columns()
                ),
                &[
                    (&destination, Type::TEXT),
                    (&status, Type::TEXT),
                    (&page.after, Type::INT8),
                    (&(i64::from(page.limit.get()) + 1), Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        let items = rows
            .iter()
            .map(delivery_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(items, page.limit))
    }

    async fn retry_delivery(
        &self,
        destination: &str,
        id: Uuid,
        now: Timestamp,
    ) -> Result<Option<DeliveryRecord>, RepoError> {
        let now = now.unix_micros();
        in_transaction!(self, |tx| {
            // A running delivery (leased past `now`) is left alone; the row is locked
            // either way, so a lease can't start between the check and the change.
            tx.execute_typed(
                &format!(
                    "UPDATE event_deliveries
                     SET status = 'pending', next_attempt_at = {now}, leased_until = NULL
                     WHERE id = $2 AND destination = $3
                       AND (leased_until IS NULL OR leased_until <= {now})",
                    now = ts(1)
                ),
                &[
                    (&now, Type::INT8),
                    (&id, Type::UUID),
                    (&destination, Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
            let row = tx
                .query_typed(
                    &format!(
                        "SELECT {} FROM event_deliveries d
                         JOIN outbox_events e ON e.seq = d.event_seq
                         WHERE d.id = $1 AND d.destination = $2",
                        delivery_columns()
                    ),
                    &[(&id, Type::UUID), (&destination, Type::TEXT)],
                )
                .await
                .map_err(repo_error)?;
            row.first().map(delivery_from_row).transpose()
        })
    }
}
