use hypha_core::{
    FulfillmentId, InventoryItemId, LocationId, OrderId, Page, PageRequest, PurchaseOrderId,
    ReceiptId, RepoError, TransferId, VariantId,
};
use hypha_inventory::{
    MovementFilter, StockMovement, StockMovementEntry, StockMovementRepo, StockReason, StockSource,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{actor_from_db, actor_to_db, col, micros, opt_micros},
    error::{corrupt, repo_error},
    store::{EventStamp, PgConnection, PgStore, Queries},
};

/// A source as its columns: kind, ID, parent ID.
fn source_to_db(source: StockSource) -> (&'static str, Option<Uuid>, Option<Uuid>) {
    match source {
        StockSource::Manual => ("manual", None, None),
        StockSource::Fulfillment { order, fulfillment } => (
            "fulfillment",
            Some(fulfillment.as_uuid()),
            Some(order.as_uuid()),
        ),
        StockSource::Receipt {
            purchase_order,
            receipt,
        } => (
            "receipt",
            Some(receipt.as_uuid()),
            Some(purchase_order.as_uuid()),
        ),
        StockSource::PurchaseOrder(id) => ("purchase_order", Some(id.as_uuid()), None),
        StockSource::Transfer(id) => ("transfer", Some(id.as_uuid()), None),
        StockSource::Migration => ("migration", None, None),
    }
}

fn source_from_db(
    kind: &str,
    id: Option<Uuid>,
    parent: Option<Uuid>,
) -> Result<StockSource, RepoError> {
    let missing = || corrupt(format!("a {kind:?} stock source without its IDs"));
    Ok(match kind {
        "manual" => StockSource::Manual,
        "fulfillment" => StockSource::Fulfillment {
            order: OrderId::from_uuid(parent.ok_or_else(missing)?),
            fulfillment: FulfillmentId::from_uuid(id.ok_or_else(missing)?),
        },
        "receipt" => StockSource::Receipt {
            purchase_order: PurchaseOrderId::from_uuid(parent.ok_or_else(missing)?),
            receipt: ReceiptId::from_uuid(id.ok_or_else(missing)?),
        },
        "purchase_order" => {
            StockSource::PurchaseOrder(PurchaseOrderId::from_uuid(id.ok_or_else(missing)?))
        }
        "transfer" => StockSource::Transfer(TransferId::from_uuid(id.ok_or_else(missing)?)),
        "migration" => StockSource::Migration,
        other => return Err(corrupt(format!("unknown stock source {other:?}"))),
    })
}

/// Writes `movements` inside the caller's transaction, in their order, as the stamp's
/// actor. Call it after the items' rows are locked (written), so an item's rows go in
/// the order its writes commit.
pub(crate) async fn insert_movements(
    tx: &impl Queries,
    movements: &[StockMovement],
    stamp: &EventStamp<'_>,
) -> Result<(), RepoError> {
    if movements.is_empty() {
        return Ok(());
    }
    let (actor_kind, actor_id) = actor_to_db(stamp.actor);
    let items: Vec<Uuid> = movements.iter().map(|m| m.item.as_uuid()).collect();
    let variants: Vec<Uuid> = movements.iter().map(|m| m.variant.as_uuid()).collect();
    let locations: Vec<Uuid> = movements.iter().map(|m| m.location.as_uuid()).collect();
    let deltas: Vec<i64> = movements.iter().map(|m| m.delta).collect();
    let afters: Vec<i64> = movements.iter().map(|m| m.after).collect();
    let reasons: Vec<&str> = movements.iter().map(|m| m.reason.as_str()).collect();
    let sources: Vec<_> = movements.iter().map(|m| source_to_db(m.source)).collect();
    let kinds: Vec<&str> = sources.iter().map(|s| s.0).collect();
    let ids: Vec<Option<Uuid>> = sources.iter().map(|s| s.1).collect();
    let parents: Vec<Option<Uuid>> = sources.iter().map(|s| s.2).collect();
    let notes: Vec<Option<&str>> = movements
        .iter()
        .map(|m| m.note.as_ref().map(|n| n.as_str()))
        .collect();
    tx.execute_typed(
        "INSERT INTO stock_movements (item_id, variant_id, location_id, delta, after, reason,
             source_kind, source_id, source_parent_id, actor_kind, actor_id, note)
         SELECT i, v, l, d, a, r, k, s, p, $11, $12, n
         FROM unnest($1::uuid[], $2::uuid[], $3::uuid[], $4::bigint[], $5::bigint[],
                     $6::text[], $7::text[], $8::uuid[], $9::uuid[], $10::text[])
              WITH ORDINALITY AS m (i, v, l, d, a, r, k, s, p, n, o)
         ORDER BY o",
        &[
            (&items, Type::UUID_ARRAY),
            (&variants, Type::UUID_ARRAY),
            (&locations, Type::UUID_ARRAY),
            (&deltas, Type::INT8_ARRAY),
            (&afters, Type::INT8_ARRAY),
            (&reasons, Type::TEXT_ARRAY),
            (&kinds, Type::TEXT_ARRAY),
            (&ids, Type::UUID_ARRAY),
            (&parents, Type::UUID_ARRAY),
            (&notes, Type::TEXT_ARRAY),
            (&actor_kind, Type::TEXT),
            (&actor_id, Type::UUID),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

fn movement_columns() -> String {
    format!(
        "position, item_id, variant_id, location_id, delta, after, reason, source_kind, \
         source_id, source_parent_id, actor_kind, actor_id, note, {}",
        micros("at")
    )
}

fn movement_from_row(row: &Row) -> Result<StockMovementEntry, RepoError> {
    let reason: String = col(row, "reason")?;
    let kind: String = col(row, "source_kind")?;
    let actor_kind: String = col(row, "actor_kind")?;
    Ok(StockMovementEntry {
        position: col(row, "position")?,
        item: InventoryItemId::from_uuid(col(row, "item_id")?),
        variant: VariantId::from_uuid(col(row, "variant_id")?),
        location: col::<Option<Uuid>>(row, "location_id")?.map(LocationId::from_uuid),
        delta: col(row, "delta")?,
        after: col(row, "after")?,
        reason: StockReason::parse(&reason)
            .ok_or_else(|| corrupt(format!("unknown stock reason {reason:?}")))?,
        source: source_from_db(&kind, col(row, "source_id")?, col(row, "source_parent_id")?)?,
        note: col(row, "note")?,
        actor: actor_from_db(&actor_kind, col(row, "actor_id")?)?,
        at: hypha_core::Timestamp::from_unix_micros(col(row, "at")?),
    })
}

impl<C: PgConnection> StockMovementRepo for PgStore<C> {
    async fn list(
        &self,
        item: InventoryItemId,
        filter: &MovementFilter,
        page: PageRequest<i64>,
    ) -> Result<Page<StockMovementEntry>, RepoError> {
        let location = filter.location.map(|l| l.as_uuid());
        let reason = filter.reason.map(StockReason::as_str);
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM stock_movements
                     WHERE item_id = $1
                       AND ($2::uuid IS NULL OR location_id = $2)
                       AND ($3::text IS NULL OR reason = $3)
                       AND ($4::bigint IS NULL
                            OR at >= 'epoch'::timestamptz + $4 * interval '1 microsecond')
                       AND ($5::bigint IS NULL
                            OR at < 'epoch'::timestamptz + $5 * interval '1 microsecond')
                       AND ($6::bigint IS NULL OR position < $6)
                     ORDER BY position DESC LIMIT $7",
                    movement_columns()
                ),
                &[
                    (&item.as_uuid(), Type::UUID),
                    (&location, Type::UUID),
                    (&reason, Type::TEXT),
                    (&opt_micros(filter.from), Type::INT8),
                    (&opt_micros(filter.to), Type::INT8),
                    (&page.after, Type::INT8),
                    (&(i64::from(page.limit.get()) + 1), Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        let items = rows
            .iter()
            .map(movement_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(items, page.limit))
    }
}
