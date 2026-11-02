//! Transfers between locations, with their lines.

use std::collections::HashMap;

use futures_util::future::try_join;
use hypha_core::{
    LocationId, Page, PageRequest, Recorded, RepoError, TransferId, VariantId, Version, Versioned,
    stamp_insert, stamp_update,
};
use hypha_inventory::{
    IncomingTransfer, Transfer, TransferFilter, TransferLineSnapshot, TransferNumber, TransferRepo,
    TransferSnapshot, TransferStatus, TransferSummary,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        Param, col, micros, opt_micros, opt_ts, stamp_columns, stamps_from_row, ts,
        version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, Queries, consistent_read, in_transaction},
};

/// The transfer's own columns, for a SELECT on `t`.
fn columns() -> String {
    format!(
        "t.id, t.version, t.number, t.origin_id, t.destination_id, {}, t.reference, t.note,
         t.status, {}, {}, {}, t.line_count, t.units, {}",
        micros("t.expected_at"),
        micros("t.shipped_at"),
        micros("t.received_at"),
        micros("t.cancelled_at"),
        stamp_columns("t."),
    )
}

fn count_to_db(n: u32) -> Result<i32, RepoError> {
    i32::try_from(n).map_err(|_| corrupt(format!("count {n}")))
}

fn count_from_db(n: i32) -> Result<u32, RepoError> {
    u32::try_from(n).map_err(|_| corrupt(format!("count {n}")))
}

fn status_from_db(status: &str) -> Result<TransferStatus, RepoError> {
    TransferStatus::parse(status).ok_or_else(|| corrupt(format!("transfer status {status:?}")))
}

fn transfer_from_row(
    row: &Row,
    lines: &mut HashMap<Uuid, Vec<TransferLineSnapshot>>,
) -> Result<Transfer, RepoError> {
    let id: Uuid = col(row, "id")?;
    let status: String = col(row, "status")?;
    let snapshot = TransferSnapshot {
        id: TransferId::from_uuid(id),
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        number: TransferNumber::new(col(row, "number")?).map_err(corrupt)?,
        origin: LocationId::from_uuid(col(row, "origin_id")?),
        destination: LocationId::from_uuid(col(row, "destination_id")?),
        expected_at: opt_ts(col(row, "expected_at")?),
        reference: col(row, "reference")?,
        note: col(row, "note")?,
        lines: lines.remove(&id).unwrap_or_default(),
        status: status_from_db(&status)?,
        shipped_at: opt_ts(col(row, "shipped_at")?),
        received_at: opt_ts(col(row, "received_at")?),
        cancelled_at: opt_ts(col(row, "cancelled_at")?),
    };
    Transfer::restore(snapshot).map_err(|e| corrupt(format!("transfer {id}: {e}")))
}

fn line_from_row(row: &Row) -> Result<TransferLineSnapshot, RepoError> {
    Ok(TransferLineSnapshot {
        variant: VariantId::from_uuid(col(row, "variant_id")?),
        quantity: count_from_db(col(row, "quantity")?)?,
        shipped: count_from_db(col(row, "shipped")?)?,
        received: count_from_db(col(row, "received")?)?,
        rejected: count_from_db(col(row, "rejected")?)?,
    })
}

/// The transfers `filter` (on `t`) selects with `param`, each with its lines, both
/// queries sent together.
async fn load(
    client: &impl Queries,
    filter: &str,
    param: Uuid,
) -> Result<Vec<Transfer>, RepoError> {
    let param: [Param<'_>; 1] = [(&param, Type::UUID)];
    let transfers_sql = format!("SELECT {} FROM transfers t WHERE {filter}", columns());
    let lines_sql = format!(
        "SELECT l.* FROM transfer_lines l
         JOIN transfers t ON t.id = l.transfer_id
         WHERE {filter} ORDER BY l.transfer_id, l.position"
    );
    let (rows, line_rows) = try_join(
        client.query_typed(&transfers_sql, &param),
        client.query_typed(&lines_sql, &param),
    )
    .await
    .map_err(repo_error)?;
    let mut lines: HashMap<Uuid, Vec<TransferLineSnapshot>> = HashMap::new();
    for row in &line_rows {
        lines
            .entry(col(row, "transfer_id")?)
            .or_default()
            .push(line_from_row(row)?);
    }
    rows.iter()
        .map(|row| transfer_from_row(row, &mut lines))
        .collect()
}

/// A transfer's own column values, owned so the parameters can borrow them.
struct Values<'a> {
    s: &'a TransferSnapshot,
    id: Uuid,
    number: i64,
    origin: Uuid,
    destination: Uuid,
    expected_at: Option<i64>,
    status: &'static str,
    shipped_at: Option<i64>,
    received_at: Option<i64>,
    cancelled_at: Option<i64>,
    line_count: i32,
    units: i64,
}

impl<'a> Values<'a> {
    fn of(s: &'a TransferSnapshot) -> Result<Self, RepoError> {
        let units: u64 = s.lines.iter().map(|l| u64::from(l.quantity)).sum();
        Ok(Self {
            s,
            id: s.id.as_uuid(),
            number: s.number.value(),
            origin: s.origin.as_uuid(),
            destination: s.destination.as_uuid(),
            expected_at: opt_micros(s.expected_at),
            status: s.status.as_str(),
            shipped_at: opt_micros(s.shipped_at),
            received_at: opt_micros(s.received_at),
            cancelled_at: opt_micros(s.cancelled_at),
            line_count: i32::try_from(s.lines.len())
                .map_err(|_| corrupt(format!("{} lines", s.lines.len())))?,
            units: i64::try_from(units).map_err(|_| corrupt(format!("{units} units")))?,
        })
    }

    /// `$1` the ID, then every column in `COLUMNS` order from `$2`.
    fn params(&'a self) -> Vec<Param<'a>> {
        vec![
            (&self.id, Type::UUID),
            (&self.number, Type::INT8),
            (&self.origin, Type::UUID),
            (&self.destination, Type::UUID),
            (&self.expected_at, Type::INT8),
            (&self.s.reference, Type::TEXT),
            (&self.s.note, Type::TEXT),
            (&self.status, Type::TEXT),
            (&self.shipped_at, Type::INT8),
            (&self.received_at, Type::INT8),
            (&self.cancelled_at, Type::INT8),
            (&self.line_count, Type::INT4),
            (&self.units, Type::INT8),
        ]
    }
}

/// The written columns, in `Values::params` order after the ID; the times as microseconds.
const COLUMNS: [&str; 12] = [
    "number",
    "origin_id",
    "destination_id",
    "expected_at",
    "reference",
    "note",
    "status",
    "shipped_at",
    "received_at",
    "cancelled_at",
    "line_count",
    "units",
];

/// The value placeholder for `column` at `$n`.
fn placeholder(column: &str, n: usize) -> String {
    if column.ends_with("_at") {
        ts(n)
    } else {
        format!("${n}")
    }
}

async fn insert_transfer(tx: &impl Queries, s: &TransferSnapshot) -> Result<(), RepoError> {
    let values = Values::of(s)?;
    let placeholders: Vec<String> = COLUMNS
        .iter()
        .zip(2..)
        .map(|(c, n)| placeholder(c, n))
        .collect();
    tx.query_typed(
        &format!(
            "INSERT INTO transfers (id, version, {}) VALUES ($1, 1, {})",
            COLUMNS.join(", "),
            placeholders.join(", ")
        ),
        &values.params(),
    )
    .await
    .map_err(repo_error)?;
    insert_lines(tx, s).await?;
    write_stamps(tx, "transfers", "id", &values.id, Type::UUID, s.stamps).await
}

/// `TransferRepo::update` inside the caller's transaction (also used by `PgBatch`); the
/// new version. Lines are written whole again.
pub(crate) async fn update_transfer(
    tx: &impl Queries,
    s: &TransferSnapshot,
) -> Result<Version, RepoError> {
    let values = Values::of(s)?;
    let old = version_to_db(s.version)?;
    let assignments: Vec<String> = COLUMNS
        .iter()
        .zip(3..)
        .map(|(c, n)| format!("{c} = {}", placeholder(c, n)))
        .collect();
    let mut params = values.params();
    params.insert(1, (&old, Type::INT8));
    let row = tx
        .query_typed_opt(
            &format!(
                "UPDATE transfers SET version = version + 1, {}
                 WHERE id = $1 AND version = $2 RETURNING version",
                assignments.join(", ")
            ),
            &params,
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    let id = values.id;
    tx.query_typed(
        "DELETE FROM transfer_lines WHERE transfer_id = $1",
        &[(&id, Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    insert_lines(tx, s).await?;
    write_stamps(tx, "transfers", "id", &id, Type::UUID, s.stamps).await?;
    version_from_db(col(&row, "version")?)
}

async fn insert_lines(tx: &impl Queries, s: &TransferSnapshot) -> Result<(), RepoError> {
    if s.lines.is_empty() {
        return Ok(());
    }
    let counts = |f: fn(&TransferLineSnapshot) -> u32| -> Result<Vec<i32>, RepoError> {
        s.lines.iter().map(|l| count_to_db(f(l))).collect()
    };
    let variants: Vec<Uuid> = s.lines.iter().map(|l| l.variant.as_uuid()).collect();
    let quantities = counts(|l| l.quantity)?;
    let shipped = counts(|l| l.shipped)?;
    let received = counts(|l| l.received)?;
    let rejected = counts(|l| l.rejected)?;
    tx.query_typed(
        "INSERT INTO transfer_lines (transfer_id, position, variant_id, quantity, shipped,
             received, rejected)
         SELECT $1, (n - 1)::integer, v, q, s, r, rj
         FROM unnest($2::uuid[], $3::integer[], $4::integer[], $5::integer[], $6::integer[])
              WITH ORDINALITY AS l (v, q, s, r, rj, n)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&variants, Type::UUID_ARRAY),
            (&quantities, Type::INT4_ARRAY),
            (&shipped, Type::INT4_ARRAY),
            (&received, Type::INT4_ARRAY),
            (&rejected, Type::INT4_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// A summary straight from the transfer's row.
fn summary_from_row(row: &Row) -> Result<TransferSummary, RepoError> {
    let status: String = col(row, "status")?;
    let line_count: i32 = col(row, "line_count")?;
    let units: i64 = col(row, "units")?;
    Ok(TransferSummary {
        id: TransferId::from_uuid(col(row, "id")?),
        number: TransferNumber::new(col(row, "number")?).map_err(corrupt)?,
        origin: LocationId::from_uuid(col(row, "origin_id")?),
        destination: LocationId::from_uuid(col(row, "destination_id")?),
        status: status_from_db(&status)?,
        line_count: usize::try_from(line_count)
            .map_err(|_| corrupt(format!("{line_count} lines")))?,
        units: u64::try_from(units).map_err(|_| corrupt(format!("{units} units")))?,
        expected_at: opt_ts(col(row, "expected_at")?),
        shipped_at: opt_ts(col(row, "shipped_at")?),
        received_at: opt_ts(col(row, "received_at")?),
        created_at: stamps_from_row(row)?.map(|s| s.created_at),
    })
}

impl<C: PgConnection> TransferRepo for PgStore<C> {
    async fn get(&self, id: TransferId) -> Result<Option<Transfer>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            Ok(load(tx, "t.id = $1", id.as_uuid()).await?.pop())
        })
    }

    async fn list(
        &self,
        filter: &TransferFilter,
        page: PageRequest<TransferId>,
    ) -> Result<Page<TransferSummary>, RepoError> {
        let status = filter.status.map(TransferStatus::as_str);
        let origin = filter.origin.map(|l| l.as_uuid());
        let destination = filter.destination.map(|l| l.as_uuid());
        let variant = filter.variant.map(|v| v.as_uuid());
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM transfers t
                     WHERE ($1::text IS NULL OR t.status = $1)
                       AND ($2::uuid IS NULL OR t.origin_id = $2)
                       AND ($3::uuid IS NULL OR t.destination_id = $3)
                       AND ($4::uuid IS NULL OR EXISTS (
                            SELECT 1 FROM transfer_lines l
                            WHERE l.transfer_id = t.id AND l.variant_id = $4))
                       AND ($5::uuid IS NULL OR t.id < $5)
                     ORDER BY t.id DESC LIMIT $6",
                    columns()
                ),
                &[
                    (&status, Type::TEXT),
                    (&origin, Type::UUID),
                    (&destination, Type::UUID),
                    (&variant, Type::UUID),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        let items = rows
            .iter()
            .map(summary_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(items, page.limit))
    }

    async fn next_number(&self) -> Result<TransferNumber, RepoError> {
        let mut conn = self.conn().await;
        // `nextval` is atomic across sessions and never rolled back: no two transfers get
        // the same number.
        let row = conn
            .db()
            .query_typed_one("SELECT nextval('transfer_numbers') AS number", &[])
            .await
            .map_err(repo_error)?;
        TransferNumber::new(col(&row, "number")?).map_err(corrupt)
    }

    async fn insert(&self, transfer: &mut Transfer) -> Result<(), RepoError> {
        stamp_insert(transfer, self.now());
        let s = transfer.snapshot();
        let events = transfer.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_transfer(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        transfer.set_version(version);
        Ok(())
    }

    async fn update(&self, transfer: &mut Transfer) -> Result<(), RepoError> {
        stamp_update(transfer, self.now());
        let s = transfer.snapshot();
        let events = transfer.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_transfer(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        transfer.set_version(version);
        Ok(())
    }

    async fn delete(&self, transfer: &mut Transfer) -> Result<(), RepoError> {
        let id = transfer.id().as_uuid();
        let version = version_to_db(transfer.version())?;
        let events = transfer.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            let deleted = tx
                .query_typed_opt(
                    "DELETE FROM transfers WHERE id = $1 AND version = $2 RETURNING id",
                    &[(&id, Type::UUID), (&version, Type::INT8)],
                )
                .await
                .map_err(repo_error)?;
            if deleted.is_none() {
                return Err(RepoError::StaleVersion);
            }
            insert_events(tx, &events, None, stamp).await
        })
    }

    async fn incoming(&self, variants: &[VariantId]) -> Result<Vec<IncomingTransfer>, RepoError> {
        if variants.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = variants.iter().map(|v| v.as_uuid()).collect();
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT l.variant_id, t.destination_id,
                        sum(l.shipped - l.received - l.rejected)::bigint AS quantity,
                        (extract(epoch FROM min(t.expected_at)) * 1000000)::bigint
                            AS next_expected_at
                 FROM transfer_lines l
                 JOIN transfers t ON t.id = l.transfer_id
                 WHERE t.status = 'in_transit' AND l.variant_id = ANY($1)
                   AND l.shipped - l.received - l.rejected > 0
                 GROUP BY l.variant_id, t.destination_id
                 ORDER BY l.variant_id, t.destination_id",
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter()
            .map(|row| {
                let quantity: i64 = col(row, "quantity")?;
                Ok(IncomingTransfer {
                    variant: VariantId::from_uuid(col(row, "variant_id")?),
                    location: LocationId::from_uuid(col(row, "destination_id")?),
                    quantity: u64::try_from(quantity).map_err(corrupt)?,
                    next_expected_at: opt_ts(col(row, "next_expected_at")?),
                })
            })
            .collect()
    }
}
