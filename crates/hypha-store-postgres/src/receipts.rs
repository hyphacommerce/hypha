//! Receipts with their lines, written only in a batch with their order.

use std::collections::HashMap;

use futures_util::future::try_join;
use hypha_core::{
    ExchangeRate, LocationId, PurchaseOrderId, ReceiptId, RepoError, Timestamp, UnitCost,
    VariantId, Version,
};
use hypha_purchasing::{Receipt, ReceiptLine, ReceiptRepo, ReceiptSnapshot, ReceiptState};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        Param, col, currency_from_db, micros, opt_micros, stamp_columns, stamps_from_row,
        version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, Queries, consistent_read},
};

fn count_to_db(n: u32) -> Result<i32, RepoError> {
    i32::try_from(n).map_err(|_| corrupt(format!("count {n}")))
}

fn count_from_db(n: i32) -> Result<u32, RepoError> {
    u32::try_from(n).map_err(|_| corrupt(format!("count {n}")))
}

fn line_from_row(row: &Row) -> Result<ReceiptLine, RepoError> {
    let cost: Option<i64> = col(row, "landed_unit_cost")?;
    let currency: Option<String> = col(row, "landed_unit_cost_currency")?;
    let landed_unit_cost = match (cost, currency) {
        (Some(cost), Some(currency)) => {
            Some(UnitCost::new(cost, currency_from_db(&currency)?).map_err(corrupt)?)
        }
        _ => None,
    };
    Ok(ReceiptLine {
        variant: VariantId::from_uuid(col(row, "variant_id")?),
        accepted: count_from_db(col(row, "accepted")?)?,
        rejected: count_from_db(col(row, "rejected")?)?,
        landed_unit_cost,
    })
}

/// The receipts matching `filter` (a condition on `r.` with one uuid parameter), oldest
/// first, with their lines, both queries sent together.
async fn load(client: &impl Queries, filter: &str, param: Uuid) -> Result<Vec<Receipt>, RepoError> {
    let param: [Param<'_>; 1] = [(&param, Type::UUID)];
    let receipts_sql = format!(
        "SELECT r.id, r.version, r.purchase_order_id, r.location_id, r.exchange_rate, r.note,
                {}, {}
         FROM receipts r WHERE {filter} ORDER BY r.id",
        micros("r.cancelled_at"),
        stamp_columns("r."),
    );
    let lines_sql = format!(
        "SELECT l.* FROM receipt_lines l JOIN receipts r ON r.id = l.receipt_id
         WHERE {filter} ORDER BY l.receipt_id, l.position"
    );
    let (receipts, line_rows) = try_join(
        client.query_typed(&receipts_sql, &param),
        client.query_typed(&lines_sql, &param),
    )
    .await
    .map_err(repo_error)?;
    let mut lines: HashMap<Uuid, Vec<ReceiptLine>> = HashMap::new();
    for row in &line_rows {
        lines
            .entry(col(row, "receipt_id")?)
            .or_default()
            .push(line_from_row(row)?);
    }
    receipts
        .iter()
        .map(|row| {
            let id: Uuid = col(row, "id")?;
            let cancelled: Option<i64> = col(row, "cancelled_at")?;
            let snapshot = ReceiptSnapshot {
                id: ReceiptId::from_uuid(id),
                version: version_from_db(col(row, "version")?)?,
                stamps: stamps_from_row(row)?,
                purchase_order: PurchaseOrderId::from_uuid(col(row, "purchase_order_id")?),
                location: LocationId::from_uuid(col(row, "location_id")?),
                lines: lines.remove(&id).unwrap_or_default(),
                exchange_rate: col::<Option<i64>>(row, "exchange_rate")?
                    .map(ExchangeRate::new)
                    .transpose()
                    .map_err(corrupt)?,
                note: col(row, "note")?,
                state: match cancelled {
                    None => ReceiptState::Recorded,
                    Some(at) => ReceiptState::Cancelled {
                        at: Timestamp::from_unix_micros(at),
                    },
                },
            };
            Receipt::restore(snapshot).map_err(|e| corrupt(format!("receipt {id}: {e}")))
        })
        .collect()
}

fn cancelled_at(s: &ReceiptSnapshot) -> Option<i64> {
    match s.state {
        ReceiptState::Recorded => None,
        ReceiptState::Cancelled { at } => opt_micros(Some(at)),
    }
}

/// `StageReceipts::insert_receipt` inside the batch's transaction: the row, then its
/// lines.
pub(crate) async fn insert_receipt(
    tx: &impl Queries,
    s: &ReceiptSnapshot,
) -> Result<(), RepoError> {
    let id = s.id.as_uuid();
    let rate = s.exchange_rate.map(ExchangeRate::scaled);
    let cancelled = cancelled_at(s);
    tx.query_typed(
        "INSERT INTO receipts (id, version, purchase_order_id, location_id, exchange_rate,
             note, cancelled_at)
         VALUES ($1, 1, $2, $3, $4, $5,
                 'epoch'::timestamptz + $6::bigint * interval '1 microsecond')",
        &[
            (&id, Type::UUID),
            (&s.purchase_order.as_uuid(), Type::UUID),
            (&s.location.as_uuid(), Type::UUID),
            (&rate, Type::INT8),
            (&s.note, Type::TEXT),
            (&cancelled, Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    let variants: Vec<Uuid> = s.lines.iter().map(|l| l.variant.as_uuid()).collect();
    let accepted = s
        .lines
        .iter()
        .map(|l| count_to_db(l.accepted))
        .collect::<Result<Vec<_>, _>>()?;
    let rejected = s
        .lines
        .iter()
        .map(|l| count_to_db(l.rejected))
        .collect::<Result<Vec<_>, _>>()?;
    let costs: Vec<Option<i64>> = s
        .lines
        .iter()
        .map(|l| l.landed_unit_cost.map(|c| c.scaled()))
        .collect();
    let currencies: Vec<Option<&str>> = s
        .lines
        .iter()
        .map(|l| l.landed_unit_cost.map(|c| c.currency().as_str()))
        .collect();
    tx.query_typed(
        "INSERT INTO receipt_lines (receipt_id, position, variant_id, accepted, rejected,
             landed_unit_cost, landed_unit_cost_currency)
         SELECT $1, (n - 1)::integer, v, a, r, c, cc
         FROM unnest($2::uuid[], $3::integer[], $4::integer[], $5::bigint[], $6::text[])
              WITH ORDINALITY AS l (v, a, r, c, cc, n)",
        &[
            (&id, Type::UUID),
            (&variants, Type::UUID_ARRAY),
            (&accepted, Type::INT4_ARRAY),
            (&rejected, Type::INT4_ARRAY),
            (&costs, Type::INT8_ARRAY),
            (&currencies, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_stamps(tx, "receipts", "id", &id, Type::UUID, s.stamps).await
}

/// `StageReceipts::update_receipt` inside the batch's transaction: only whether it was
/// taken back changes. `StaleVersion` unless still at the version loaded.
pub(crate) async fn update_receipt(
    tx: &impl Queries,
    s: &ReceiptSnapshot,
) -> Result<Version, RepoError> {
    let id = s.id.as_uuid();
    let old = version_to_db(s.version)?;
    let cancelled = cancelled_at(s);
    let row = tx
        .query_typed_opt(
            "UPDATE receipts SET version = version + 1,
                 cancelled_at = 'epoch'::timestamptz + $3::bigint * interval '1 microsecond'
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&id, Type::UUID),
                (&old, Type::INT8),
                (&cancelled, Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    write_stamps(tx, "receipts", "id", &id, Type::UUID, s.stamps).await?;
    version_from_db(col(&row, "version")?)
}

impl<C: PgConnection> ReceiptRepo for PgStore<C> {
    async fn get(&self, id: ReceiptId) -> Result<Option<Receipt>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            Ok(load(tx, "r.id = $1", id.as_uuid()).await?.pop())
        })
    }

    async fn for_purchase_order(&self, po: PurchaseOrderId) -> Result<Vec<Receipt>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            load(tx, "r.purchase_order_id = $1", po.as_uuid()).await
        })
    }
}
