//! Purchase orders with their lines and extra costs.

use std::collections::HashMap;

use futures_util::future::try_join3;
use hypha_core::Direction;
use hypha_core::{
    ExchangeRate, Keyset, LocationId, Money, Page, PageRequest, PurchaseOrderId, Recorded,
    RepoError, SortKey, SupplierId, UnitCost, VariantId, Version, Versioned, stamp_insert,
    stamp_update,
};
use hypha_purchasing::{
    CostAllocation, CostDraft, CostKind, IncomingStock, LineSnapshot, PurchaseOrder,
    PurchaseOrderFilter, PurchaseOrderLine, PurchaseOrderNumber, PurchaseOrderRepo,
    PurchaseOrderSnapshot, PurchaseOrderSort, PurchaseOrderStatus, PurchaseOrderSummary,
    ReceiptStatus,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        Param, col, currency_from_db, micros, opt_micros, opt_ts, stamp_columns, stamps_from_row,
        ts, version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, Queries, consistent_read, in_transaction},
    suppliers::{payment_terms_from_row, payment_terms_to_db},
};

/// The order's own columns, for a SELECT on `po`.
fn columns() -> String {
    format!(
        "po.id, po.version, po.number, po.supplier_id, po.supplier_name, po.destination_id,
         po.currency, po.exchange_rate, po.payment_terms_due, po.payment_terms_days,
         po.payment_terms_note, {}, po.supplier_reference, po.note_to_supplier, po.note,
         po.tax, po.status, po.receipt_status, {}, {}, {}, {}, po.revision, po.sent_revision,
         po.line_count,
         po.total, {}",
        micros("po.expected_at"),
        micros("po.ordered_at"),
        micros("po.sent_at"),
        micros("po.closed_at"),
        micros("po.cancelled_at"),
        stamp_columns("po."),
    )
}

fn count_to_db(n: u32) -> Result<i32, RepoError> {
    i32::try_from(n).map_err(|_| corrupt(format!("count {n}")))
}

fn count_from_db(n: i32) -> Result<u32, RepoError> {
    u32::try_from(n).map_err(|_| corrupt(format!("count {n}")))
}

/// What `columns` and its children load, as an order.
fn order_from_row(
    row: &Row,
    lines: &mut HashMap<Uuid, Vec<LineSnapshot>>,
    costs: &mut HashMap<Uuid, Vec<CostDraft>>,
) -> Result<PurchaseOrder, RepoError> {
    let id: Uuid = col(row, "id")?;
    let currency: String = col(row, "currency")?;
    let status: String = col(row, "status")?;
    let receipt_status: String = col(row, "receipt_status")?;
    let currency = currency_from_db(&currency)?;
    let snapshot = PurchaseOrderSnapshot {
        id: PurchaseOrderId::from_uuid(id),
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        number: PurchaseOrderNumber::new(col(row, "number")?).map_err(corrupt)?,
        supplier: SupplierId::from_uuid(col(row, "supplier_id")?),
        supplier_name: col(row, "supplier_name")?,
        destination: col::<Option<Uuid>>(row, "destination_id")?.map(LocationId::from_uuid),
        currency,
        exchange_rate: col::<Option<i64>>(row, "exchange_rate")?
            .map(ExchangeRate::new)
            .transpose()
            .map_err(corrupt)?,
        payment_terms: payment_terms_from_row(row)?,
        expected_at: opt_ts(col(row, "expected_at")?),
        supplier_reference: col(row, "supplier_reference")?,
        note_to_supplier: col(row, "note_to_supplier")?,
        note: col(row, "note")?,
        lines: lines.remove(&id).unwrap_or_default(),
        costs: costs.remove(&id).unwrap_or_default(),
        tax: col::<Option<i64>>(row, "tax")?
            .map(|amount| Money::new(amount, currency))
            .transpose()
            .map_err(corrupt)?,
        status: PurchaseOrderStatus::parse(&status)
            .ok_or_else(|| corrupt(format!("purchase order status {status:?}")))?,
        receipt_status: ReceiptStatus::parse(&receipt_status)
            .ok_or_else(|| corrupt(format!("receipt status {receipt_status:?}")))?,
        ordered_at: opt_ts(col(row, "ordered_at")?),
        sent_at: opt_ts(col(row, "sent_at")?),
        sent_revision: col::<Option<i32>>(row, "sent_revision")?
            .map(count_from_db)
            .transpose()?,
        closed_at: opt_ts(col(row, "closed_at")?),
        cancelled_at: opt_ts(col(row, "cancelled_at")?),
        revision: count_from_db(col(row, "revision")?)?,
    };
    PurchaseOrder::restore(snapshot).map_err(|e| corrupt(format!("purchase order {id}: {e}")))
}

fn line_from_row(row: &Row) -> Result<LineSnapshot, RepoError> {
    let currency: String = col(row, "unit_cost_currency")?;
    Ok(LineSnapshot {
        variant: VariantId::from_uuid(col(row, "variant_id")?),
        product_title: col(row, "product_title")?,
        variant_title: col(row, "variant_title")?,
        sku: col(row, "sku")?,
        supplier_sku: col(row, "supplier_sku")?,
        quantity: count_from_db(col(row, "quantity")?)?,
        unit_cost: UnitCost::new(col(row, "unit_cost")?, currency_from_db(&currency)?)
            .map_err(corrupt)?,
        received: count_from_db(col(row, "received")?)?,
        rejected: count_from_db(col(row, "rejected")?)?,
        cancelled: count_from_db(col(row, "cancelled")?)?,
        extra_applied: col(row, "extra_applied")?,
    })
}

fn cost_from_row(row: &Row) -> Result<CostDraft, RepoError> {
    let kind: String = col(row, "kind")?;
    let allocation: String = col(row, "allocation")?;
    let currency: String = col(row, "currency")?;
    Ok(CostDraft {
        kind: CostKind::parse(&kind).ok_or_else(|| corrupt(format!("cost kind {kind:?}")))?,
        description: col(row, "description")?,
        amount: Money::new(col(row, "amount")?, currency_from_db(&currency)?).map_err(corrupt)?,
        allocation: CostAllocation::parse(&allocation)
            .ok_or_else(|| corrupt(format!("cost allocation {allocation:?}")))?,
    })
}

/// The orders matching `filter` (a condition on `po.` with one uuid parameter), with
/// their lines and costs, the three queries sent together.
async fn load(
    client: &impl Queries,
    filter: &str,
    param: Uuid,
) -> Result<Vec<PurchaseOrder>, RepoError> {
    let param: [Param<'_>; 1] = [(&param, Type::UUID)];
    let orders_sql = format!(
        "SELECT {} FROM purchase_orders po WHERE {filter}",
        columns()
    );
    let lines_sql = format!(
        "SELECT l.* FROM purchase_order_lines l
         JOIN purchase_orders po ON po.id = l.purchase_order_id
         WHERE {filter} ORDER BY l.purchase_order_id, l.position"
    );
    let costs_sql = format!(
        "SELECT c.* FROM purchase_order_costs c
         JOIN purchase_orders po ON po.id = c.purchase_order_id
         WHERE {filter} ORDER BY c.purchase_order_id, c.position"
    );
    let (orders, line_rows, cost_rows) = try_join3(
        client.query_typed(&orders_sql, &param),
        client.query_typed(&lines_sql, &param),
        client.query_typed(&costs_sql, &param),
    )
    .await
    .map_err(repo_error)?;
    let mut lines: HashMap<Uuid, Vec<LineSnapshot>> = HashMap::new();
    for row in &line_rows {
        lines
            .entry(col(row, "purchase_order_id")?)
            .or_default()
            .push(line_from_row(row)?);
    }
    let mut costs: HashMap<Uuid, Vec<CostDraft>> = HashMap::new();
    for row in &cost_rows {
        costs
            .entry(col(row, "purchase_order_id")?)
            .or_default()
            .push(cost_from_row(row)?);
    }
    orders
        .iter()
        .map(|row| order_from_row(row, &mut lines, &mut costs))
        .collect()
}

/// An order's own column values, owned so the parameters can borrow them.
struct Values<'a> {
    s: &'a PurchaseOrderSnapshot,
    id: Uuid,
    number: i64,
    supplier: Uuid,
    destination: Option<Uuid>,
    currency: &'static str,
    rate: Option<i64>,
    due: Option<&'static str>,
    days: Option<i32>,
    terms_note: Option<&'a str>,
    expected_at: Option<i64>,
    tax: Option<i64>,
    status: &'static str,
    receipt_status: &'static str,
    ordered_at: Option<i64>,
    sent_at: Option<i64>,
    closed_at: Option<i64>,
    cancelled_at: Option<i64>,
    revision: i32,
    sent_revision: Option<i32>,
    line_count: i32,
    total: Option<i64>,
}

impl<'a> Values<'a> {
    fn of(s: &'a PurchaseOrderSnapshot, summary: &PurchaseOrderSummary) -> Result<Self, RepoError> {
        let (due, days, terms_note) = payment_terms_to_db(s.payment_terms.as_ref())?;
        Ok(Self {
            s,
            id: s.id.as_uuid(),
            number: s.number.value(),
            supplier: s.supplier.as_uuid(),
            destination: s.destination.map(|d| d.as_uuid()),
            currency: s.currency.as_str(),
            rate: s.exchange_rate.map(ExchangeRate::scaled),
            due,
            days,
            terms_note,
            expected_at: opt_micros(s.expected_at),
            tax: s.tax.map(|t| t.amount()),
            status: s.status.as_str(),
            receipt_status: s.receipt_status.as_str(),
            ordered_at: opt_micros(s.ordered_at),
            sent_at: opt_micros(s.sent_at),
            closed_at: opt_micros(s.closed_at),
            cancelled_at: opt_micros(s.cancelled_at),
            revision: count_to_db(s.revision)?,
            sent_revision: s.sent_revision.map(count_to_db).transpose()?,
            line_count: i32::try_from(summary.line_count)
                .map_err(|_| corrupt(format!("{} lines", summary.line_count)))?,
            total: summary.total.map(|t| t.amount()),
        })
    }

    /// `$1` the ID, then every column in `COLUMNS` order from `$2`.
    fn params(&'a self) -> Vec<Param<'a>> {
        vec![
            (&self.id, Type::UUID),
            (&self.number, Type::INT8),
            (&self.supplier, Type::UUID),
            (&self.s.supplier_name, Type::TEXT),
            (&self.destination, Type::UUID),
            (&self.currency, Type::TEXT),
            (&self.rate, Type::INT8),
            (&self.due, Type::TEXT),
            (&self.days, Type::INT4),
            (&self.terms_note, Type::TEXT),
            (&self.expected_at, Type::INT8),
            (&self.s.supplier_reference, Type::TEXT),
            (&self.s.note_to_supplier, Type::TEXT),
            (&self.s.note, Type::TEXT),
            (&self.tax, Type::INT8),
            (&self.status, Type::TEXT),
            (&self.receipt_status, Type::TEXT),
            (&self.ordered_at, Type::INT8),
            (&self.sent_at, Type::INT8),
            (&self.closed_at, Type::INT8),
            (&self.cancelled_at, Type::INT8),
            (&self.revision, Type::INT4),
            (&self.sent_revision, Type::INT4),
            (&self.line_count, Type::INT4),
            (&self.total, Type::INT8),
        ]
    }
}

/// The written columns, in `Values::params` order after the ID; the times as microseconds.
const COLUMNS: [&str; 24] = [
    "number",
    "supplier_id",
    "supplier_name",
    "destination_id",
    "currency",
    "exchange_rate",
    "payment_terms_due",
    "payment_terms_days",
    "payment_terms_note",
    "expected_at",
    "supplier_reference",
    "note_to_supplier",
    "note",
    "tax",
    "status",
    "receipt_status",
    "ordered_at",
    "sent_at",
    "closed_at",
    "cancelled_at",
    "revision",
    "sent_revision",
    "line_count",
    "total",
];

/// Whether a column holds a time, written from microseconds.
fn is_time(column: &str) -> bool {
    column.ends_with("_at")
}

/// The value placeholder for `column` at `$n`.
fn placeholder(column: &str, n: usize) -> String {
    if is_time(column) {
        ts(n)
    } else {
        format!("${n}")
    }
}

/// `PurchaseOrderRepo::insert` inside the caller's transaction: the order, its lines and
/// costs.
pub(crate) async fn insert_purchase_order(
    tx: &impl Queries,
    s: &PurchaseOrderSnapshot,
    summary: &PurchaseOrderSummary,
) -> Result<(), RepoError> {
    let values = Values::of(s, summary)?;
    let placeholders: Vec<String> = COLUMNS
        .iter()
        .zip(2..)
        .map(|(c, n)| placeholder(c, n))
        .collect();
    tx.query_typed(
        &format!(
            "INSERT INTO purchase_orders (id, version, {}) VALUES ($1, 1, {})",
            COLUMNS.join(", "),
            placeholders.join(", ")
        ),
        &values.params(),
    )
    .await
    .map_err(repo_error)?;
    insert_children(tx, s).await?;
    write_stamps(
        tx,
        "purchase_orders",
        "id",
        &values.id,
        Type::UUID,
        s.stamps,
    )
    .await
}

/// `PurchaseOrderRepo::update` inside the caller's transaction (also used by `PgBatch`);
/// the new version. Lines and costs are written whole again.
pub(crate) async fn update_purchase_order(
    tx: &impl Queries,
    s: &PurchaseOrderSnapshot,
    summary: &PurchaseOrderSummary,
) -> Result<Version, RepoError> {
    let values = Values::of(s, summary)?;
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
                "UPDATE purchase_orders SET version = version + 1, {}
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
        "DELETE FROM purchase_order_lines WHERE purchase_order_id = $1",
        &[(&id, Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    tx.query_typed(
        "DELETE FROM purchase_order_costs WHERE purchase_order_id = $1",
        &[(&id, Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    insert_children(tx, s).await?;
    write_stamps(tx, "purchase_orders", "id", &id, Type::UUID, s.stamps).await?;
    version_from_db(col(&row, "version")?)
}

async fn insert_children(tx: &impl Queries, s: &PurchaseOrderSnapshot) -> Result<(), RepoError> {
    let id = s.id.as_uuid();
    if !s.lines.is_empty() {
        let variants: Vec<Uuid> = s.lines.iter().map(|l| l.variant.as_uuid()).collect();
        let products: Vec<&str> = s.lines.iter().map(|l| l.product_title.as_str()).collect();
        let titles: Vec<Option<&str>> =
            s.lines.iter().map(|l| l.variant_title.as_deref()).collect();
        let skus: Vec<Option<&str>> = s.lines.iter().map(|l| l.sku.as_deref()).collect();
        let supplier_skus: Vec<Option<&str>> =
            s.lines.iter().map(|l| l.supplier_sku.as_deref()).collect();
        let quantities = counts(s.lines.iter().map(|l| l.quantity))?;
        let costs: Vec<i64> = s.lines.iter().map(|l| l.unit_cost.scaled()).collect();
        let currencies: Vec<&str> = s
            .lines
            .iter()
            .map(|l| l.unit_cost.currency().as_str())
            .collect();
        let received = counts(s.lines.iter().map(|l| l.received))?;
        let rejected = counts(s.lines.iter().map(|l| l.rejected))?;
        let cancelled = counts(s.lines.iter().map(|l| l.cancelled))?;
        let extra_applied: Vec<Option<i64>> = s.lines.iter().map(|l| l.extra_applied).collect();
        tx.query_typed(
            "INSERT INTO purchase_order_lines (purchase_order_id, position, variant_id,
                 product_title, variant_title, sku, supplier_sku, quantity, unit_cost,
                 unit_cost_currency, received, rejected, cancelled, extra_applied)
             SELECT $1, (n - 1)::integer, v, p, t, s, ss, q, c, cc, r, rj, x, e
             FROM unnest($2::uuid[], $3::text[], $4::text[], $5::text[], $6::text[],
                         $7::integer[], $8::bigint[], $9::text[], $10::integer[],
                         $11::integer[], $12::integer[], $13::bigint[])
                  WITH ORDINALITY AS l (v, p, t, s, ss, q, c, cc, r, rj, x, e, n)",
            &[
                (&id, Type::UUID),
                (&variants, Type::UUID_ARRAY),
                (&products, Type::TEXT_ARRAY),
                (&titles, Type::TEXT_ARRAY),
                (&skus, Type::TEXT_ARRAY),
                (&supplier_skus, Type::TEXT_ARRAY),
                (&quantities, Type::INT4_ARRAY),
                (&costs, Type::INT8_ARRAY),
                (&currencies, Type::TEXT_ARRAY),
                (&received, Type::INT4_ARRAY),
                (&rejected, Type::INT4_ARRAY),
                (&cancelled, Type::INT4_ARRAY),
                (&extra_applied, Type::INT8_ARRAY),
            ],
        )
        .await
        .map_err(repo_error)?;
    }
    if !s.costs.is_empty() {
        let kinds: Vec<&str> = s.costs.iter().map(|c| c.kind.as_str()).collect();
        let descriptions: Vec<Option<&str>> =
            s.costs.iter().map(|c| c.description.as_deref()).collect();
        let amounts: Vec<i64> = s.costs.iter().map(|c| c.amount.amount()).collect();
        let currencies: Vec<&str> = s
            .costs
            .iter()
            .map(|c| c.amount.currency().as_str())
            .collect();
        let allocations: Vec<&str> = s.costs.iter().map(|c| c.allocation.as_str()).collect();
        tx.query_typed(
            "INSERT INTO purchase_order_costs (purchase_order_id, position, kind, description,
                 amount, currency, allocation)
             SELECT $1, (n - 1)::integer, k, d, a, c, al
             FROM unnest($2::text[], $3::text[], $4::bigint[], $5::text[], $6::text[])
                  WITH ORDINALITY AS c (k, d, a, c, al, n)",
            &[
                (&id, Type::UUID),
                (&kinds, Type::TEXT_ARRAY),
                (&descriptions, Type::TEXT_ARRAY),
                (&amounts, Type::INT8_ARRAY),
                (&currencies, Type::TEXT_ARRAY),
                (&allocations, Type::TEXT_ARRAY),
            ],
        )
        .await
        .map_err(repo_error)?;
    }
    Ok(())
}

fn counts(values: impl Iterator<Item = u32>) -> Result<Vec<i32>, RepoError> {
    values.map(count_to_db).collect()
}

/// A summary straight from the order's row.
fn summary_from_row(row: &Row) -> Result<PurchaseOrderSummary, RepoError> {
    let currency: String = col(row, "currency")?;
    let status: String = col(row, "status")?;
    let receipt_status: String = col(row, "receipt_status")?;
    let currency = currency_from_db(&currency)?;
    let line_count: i32 = col(row, "line_count")?;
    Ok(PurchaseOrderSummary {
        id: PurchaseOrderId::from_uuid(col(row, "id")?),
        number: PurchaseOrderNumber::new(col(row, "number")?).map_err(corrupt)?,
        supplier: SupplierId::from_uuid(col(row, "supplier_id")?),
        supplier_name: col(row, "supplier_name")?,
        destination: col::<Option<Uuid>>(row, "destination_id")?.map(LocationId::from_uuid),
        currency,
        status: PurchaseOrderStatus::parse(&status)
            .ok_or_else(|| corrupt(format!("purchase order status {status:?}")))?,
        receipt_status: ReceiptStatus::parse(&receipt_status)
            .ok_or_else(|| corrupt(format!("receipt status {receipt_status:?}")))?,
        expected_at: opt_ts(col(row, "expected_at")?),
        ordered_at: opt_ts(col(row, "ordered_at")?),
        line_count: usize::try_from(line_count)
            .map_err(|_| corrupt(format!("{line_count} lines")))?,
        total: col::<Option<i64>>(row, "total")?
            .map(|amount| Money::new(amount, currency))
            .transpose()
            .map_err(corrupt)?,
        created_at: stamps_from_row(row)?.map(|s| s.created_at),
    })
}

impl<C: PgConnection> PurchaseOrderRepo for PgStore<C> {
    async fn get(&self, id: PurchaseOrderId) -> Result<Option<PurchaseOrder>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            Ok(load(tx, "po.id = $1", id.as_uuid()).await?.pop())
        })
    }

    async fn list(
        &self,
        filter: &PurchaseOrderFilter,
        sort: PurchaseOrderSort,
        page: PageRequest<Keyset<PurchaseOrderId>>,
    ) -> Result<Page<PurchaseOrderSummary>, RepoError> {
        let after_key = match &page.after {
            None => None,
            Some(Keyset {
                key: SortKey::Int(key),
                ..
            }) => Some(*key),
            Some(other) => {
                return Err(RepoError::Backend(format!(
                    "a {:?} position for a {sort:?} list",
                    other.key
                )));
            }
        };
        let after_id = page.after.as_ref().map(|k| k.id.as_uuid());
        // The sort key as SQL: the number, or the expected time with none after every time.
        let key = match sort {
            PurchaseOrderSort::NumberDesc | PurchaseOrderSort::NumberAsc => "po.number".to_string(),
            PurchaseOrderSort::ExpectedAt => format!(
                "COALESCE((extract(epoch FROM po.expected_at) * 1000000)::bigint, {})",
                PurchaseOrderSort::NOT_EXPECTED
            ),
        };
        let (cmp, order) = match sort.direction() {
            Direction::Ascending => (">", "ASC"),
            Direction::Descending => ("<", "DESC"),
        };
        let supplier = filter.supplier.map(|s| s.as_uuid());
        let status = filter.status.map(PurchaseOrderStatus::as_str);
        let receipt_status = filter.receipt_status.map(ReceiptStatus::as_str);
        let destination = filter.destination.map(|d| d.as_uuid());
        let variant = filter.variant.map(|v| v.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM purchase_orders po
                     WHERE ($1::uuid IS NULL OR po.supplier_id = $1)
                       AND ($2::text IS NULL OR po.status = $2)
                       AND ($3::text IS NULL OR po.receipt_status = $3)
                       AND ($4::uuid IS NULL OR po.destination_id = $4)
                       AND ($5::uuid IS NULL OR EXISTS (
                            SELECT 1 FROM purchase_order_lines l
                            WHERE l.purchase_order_id = po.id AND l.variant_id = $5))
                       AND ($6::bigint IS NULL OR ({key}, po.id) {cmp} ($6, $7::uuid))
                     ORDER BY {key} {order}, po.id {order} LIMIT $8",
                    columns()
                ),
                &[
                    (&supplier, Type::UUID),
                    (&status, Type::TEXT),
                    (&receipt_status, Type::TEXT),
                    (&destination, Type::UUID),
                    (&variant, Type::UUID),
                    (&after_key, Type::INT8),
                    (&after_id, Type::UUID),
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

    async fn next_number(&self) -> Result<PurchaseOrderNumber, RepoError> {
        let mut conn = self.conn().await;
        // `nextval` is atomic across sessions and never rolled back, so it needs no
        // transaction: two orders can't get the same number.
        let row = conn
            .db()
            .query_typed_one("SELECT nextval('purchase_order_numbers') AS number", &[])
            .await
            .map_err(repo_error)?;
        PurchaseOrderNumber::new(col(&row, "number")?).map_err(corrupt)
    }

    async fn insert(&self, po: &mut PurchaseOrder) -> Result<(), RepoError> {
        stamp_insert(po, self.now());
        let s = po.snapshot();
        let summary = po.summary();
        let events = po.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_purchase_order(tx, &s, &summary).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        po.set_version(version);
        Ok(())
    }

    async fn update(&self, po: &mut PurchaseOrder) -> Result<(), RepoError> {
        stamp_update(po, self.now());
        let s = po.snapshot();
        let summary = po.summary();
        let events = po.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_purchase_order(tx, &s, &summary).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        po.set_version(version);
        Ok(())
    }

    async fn delete(&self, po: &mut PurchaseOrder) -> Result<(), RepoError> {
        let id = po.id().as_uuid();
        let version = version_to_db(po.version())?;
        let events = po.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            let deleted = tx
                .query_typed_opt(
                    "DELETE FROM purchase_orders WHERE id = $1 AND version = $2 RETURNING id",
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

    async fn last_line(
        &self,
        supplier: SupplierId,
        variant: VariantId,
    ) -> Result<Option<PurchaseOrderLine>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            let found = tx
                .query_typed_opt(
                    "SELECT po.id FROM purchase_orders po
                     JOIN purchase_order_lines l ON l.purchase_order_id = po.id
                     WHERE po.supplier_id = $1 AND l.variant_id = $2
                       AND po.status <> 'cancelled'
                     ORDER BY po.number DESC LIMIT 1",
                    &[
                        (&supplier.as_uuid(), Type::UUID),
                        (&variant.as_uuid(), Type::UUID),
                    ],
                )
                .await
                .map_err(repo_error)?;
            let Some(row) = found else { return Ok(None) };
            let orders = load(tx, "po.id = $1", col(&row, "id")?).await?;
            Ok(orders.first().and_then(|po| po.line(variant).cloned()))
        })
    }

    async fn incoming(&self, variants: &[VariantId]) -> Result<Vec<IncomingStock>, RepoError> {
        if variants.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = variants.iter().map(|v| v.as_uuid()).collect();
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT l.variant_id, po.destination_id,
                        sum(l.quantity - l.received - l.rejected - l.cancelled)::bigint
                            AS quantity,
                        (extract(epoch FROM min(po.expected_at)) * 1000000)::bigint
                            AS next_expected_at
                 FROM purchase_order_lines l
                 JOIN purchase_orders po ON po.id = l.purchase_order_id
                 WHERE po.status = 'ordered' AND po.destination_id IS NOT NULL
                   AND l.variant_id = ANY($1)
                   AND l.quantity - l.received - l.rejected - l.cancelled > 0
                 GROUP BY l.variant_id, po.destination_id
                 ORDER BY l.variant_id, po.destination_id",
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter()
            .map(|row| {
                let quantity: i64 = col(row, "quantity")?;
                Ok(IncomingStock {
                    variant: VariantId::from_uuid(col(row, "variant_id")?),
                    location: LocationId::from_uuid(col(row, "destination_id")?),
                    quantity: u64::try_from(quantity).map_err(corrupt)?,
                    next_expected_at: opt_ts(col(row, "next_expected_at")?),
                })
            })
            .collect()
    }

    async fn store_currency_named(&self) -> Result<bool, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_one(
                "SELECT EXISTS (
                     SELECT 1 FROM purchase_orders
                     WHERE status IN ('ordered', 'closed')
                        OR (status = 'draft' AND exchange_rate IS NOT NULL)
                 ) AS named",
                &[],
            )
            .await
            .map_err(repo_error)?;
        col(&row, "named")
    }
}
