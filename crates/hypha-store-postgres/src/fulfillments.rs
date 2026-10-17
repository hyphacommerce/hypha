use std::collections::HashMap;

use hypha_core::{
    FulfillmentId, LocationId, OrderId, OrderLineId, Quantity, Recorded, RepoError, Version,
    Versioned, stamp_insert, stamp_update,
};
use hypha_fulfillment::{
    Fulfillment, FulfillmentLine, FulfillmentRepo, FulfillmentSnapshot, FulfillmentState, Tracking,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        col, micros, opt_micros, opt_ts, stamp_columns, stamps_from_row, ts, version_from_db,
        version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::Queries,
    store::{PgConnection, PgStore, consistent_read, in_transaction},
};

impl<C: PgConnection> FulfillmentRepo for PgStore<C> {
    async fn get(&self, id: FulfillmentId) -> Result<Option<Fulfillment>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            Ok(load(tx, "f.id = $1", id.as_uuid()).await?.pop())
        })
    }

    async fn list_for_order(&self, order: OrderId) -> Result<Vec<Fulfillment>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            load(tx, "f.order_id = $1", order.as_uuid()).await
        })
    }

    async fn insert(&self, fulfillment: &mut Fulfillment) -> Result<(), RepoError> {
        stamp_insert(fulfillment, self.now());
        let s = fulfillment.snapshot();
        let events = fulfillment.take_events();
        let actor = self.actor();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_fulfillment(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        fulfillment.set_version(version);
        Ok(())
    }

    async fn update(&self, fulfillment: &mut Fulfillment) -> Result<(), RepoError> {
        stamp_update(fulfillment, self.now());
        let s = fulfillment.snapshot();
        let events = fulfillment.take_events();
        let actor = self.actor();
        let version = in_transaction!(self, |tx| {
            let version = update_fulfillment(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await?;
            Ok(version)
        })?;
        fulfillment.set_version(version);
        Ok(())
    }
}

fn cancelled_at(s: &FulfillmentSnapshot) -> Option<i64> {
    opt_micros(match s.state {
        FulfillmentState::Cancelled { at } => Some(at),
        FulfillmentState::Shipped => None,
    })
}

/// `FulfillmentRepo::insert` inside the caller's transaction (also used by `PgBatch`):
/// the row, its lines and its tracking.
pub(crate) async fn insert_fulfillment(
    tx: &impl Queries,
    s: &FulfillmentSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        &format!(
            "INSERT INTO fulfillments (id, version, order_id, location_id, cancelled_at)
             VALUES ($1, 1, $2, $3, {})",
            ts(4)
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.order.as_uuid(), Type::UUID),
            (&s.location.as_uuid(), Type::UUID),
            (&cancelled_at(s), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    let lines: Vec<Uuid> = s.lines.iter().map(|l| l.line.as_uuid()).collect();
    let quantities: Vec<i64> = s
        .lines
        .iter()
        .map(|l| i64::from(l.quantity.value()))
        .collect();
    let taken: Vec<bool> = s.lines.iter().map(|l| l.stock_taken).collect();
    tx.query_typed(
        "INSERT INTO fulfillment_lines (fulfillment_id, position, order_line_id, quantity,
             stock_taken)
         SELECT $1, (n - 1)::integer, line, quantity, stock_taken
         FROM unnest($2::uuid[], $3::bigint[], $4::boolean[])
              WITH ORDINALITY AS t (line, quantity, stock_taken, n)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&lines, Type::UUID_ARRAY),
            (&quantities, Type::INT8_ARRAY),
            (&taken, Type::BOOL_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    insert_tracking(tx, s).await?;
    write_stamps(
        tx,
        "fulfillments",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await
}

/// `FulfillmentRepo::update` inside the caller's transaction (also used by `PgBatch`);
/// the new version. Lines never change after insert; tracking is replaced.
pub(crate) async fn update_fulfillment(
    tx: &impl Queries,
    s: &FulfillmentSnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            &format!(
                "UPDATE fulfillments SET version = version + 1, order_id = $3,
                     location_id = $4, cancelled_at = {}
                 WHERE id = $1 AND version = $2 RETURNING version",
                ts(5)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.order.as_uuid(), Type::UUID),
                (&s.location.as_uuid(), Type::UUID),
                (&cancelled_at(s), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    tx.query_typed(
        "DELETE FROM fulfillment_tracking WHERE fulfillment_id = $1",
        &[(&s.id.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    insert_tracking(tx, s).await?;
    write_stamps(
        tx,
        "fulfillments",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await?;
    version_from_db(col(&row, "version")?)
}

async fn insert_tracking(tx: &impl Queries, s: &FulfillmentSnapshot) -> Result<(), RepoError> {
    if s.tracking.is_empty() {
        return Ok(());
    }
    let carriers: Vec<Option<&str>> = s.tracking.iter().map(Tracking::carrier).collect();
    let numbers: Vec<&str> = s.tracking.iter().map(Tracking::number).collect();
    let urls: Vec<Option<&str>> = s.tracking.iter().map(Tracking::url).collect();
    tx.query_typed(
        "INSERT INTO fulfillment_tracking (fulfillment_id, position, carrier, number, url)
         SELECT $1, (n - 1)::integer, carrier, number, url
         FROM unnest($2::text[], $3::text[], $4::text[])
              WITH ORDINALITY AS t (carrier, number, url, n)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&carriers, Type::TEXT_ARRAY),
            (&numbers, Type::TEXT_ARRAY),
            (&urls, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// The fulfilments matching `filter` (a condition on `f.` with one uuid parameter),
/// newest first, with their lines and tracking, in 3 queries (run inside
/// `consistent_read`).
async fn load(
    client: &impl Queries,
    filter: &str,
    param: Uuid,
) -> Result<Vec<Fulfillment>, RepoError> {
    let rows = client
        .query_typed(
            &format!(
                "SELECT f.id, f.version, f.order_id, f.location_id, {}, {}
                 FROM fulfillments f WHERE {filter} ORDER BY f.id DESC",
                micros("f.cancelled_at"),
                stamp_columns("f."),
            ),
            &[(&param, Type::UUID)],
        )
        .await
        .map_err(repo_error)?;
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<Uuid> = rows
        .iter()
        .map(|r| col(r, "id"))
        .collect::<Result<_, _>>()?;

    let mut lines: HashMap<Uuid, Vec<FulfillmentLine>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT fulfillment_id, order_line_id, quantity, stock_taken FROM fulfillment_lines
             WHERE fulfillment_id = ANY($1) ORDER BY fulfillment_id, position",
            &[(&ids, Type::UUID_ARRAY)],
        )
        .await
        .map_err(repo_error)?
    {
        let quantity: i64 = col(&row, "quantity")?;
        let quantity = u32::try_from(quantity)
            .ok()
            .and_then(|q| Quantity::new(q).ok())
            .ok_or_else(|| corrupt(format!("fulfilment line quantity {quantity}")))?;
        lines
            .entry(col(&row, "fulfillment_id")?)
            .or_default()
            .push(FulfillmentLine {
                line: OrderLineId::from_uuid(col(&row, "order_line_id")?),
                quantity,
                stock_taken: col(&row, "stock_taken")?,
            });
    }

    let mut tracking: HashMap<Uuid, Vec<Tracking>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT fulfillment_id, carrier, number, url FROM fulfillment_tracking
             WHERE fulfillment_id = ANY($1) ORDER BY fulfillment_id, position",
            &[(&ids, Type::UUID_ARRAY)],
        )
        .await
        .map_err(repo_error)?
    {
        let carrier: Option<String> = col(&row, "carrier")?;
        let number: String = col(&row, "number")?;
        let url: Option<String> = col(&row, "url")?;
        let t = Tracking::new(carrier.as_deref(), &number, url.as_deref()).map_err(corrupt)?;
        tracking
            .entry(col(&row, "fulfillment_id")?)
            .or_default()
            .push(t);
    }

    rows.iter()
        .map(|row| fulfillment_from_db(row, &mut lines, &mut tracking))
        .collect()
}

fn fulfillment_from_db(
    row: &Row,
    lines: &mut HashMap<Uuid, Vec<FulfillmentLine>>,
    tracking: &mut HashMap<Uuid, Vec<Tracking>>,
) -> Result<Fulfillment, RepoError> {
    let id: Uuid = col(row, "id")?;
    let state = match opt_ts(col(row, "cancelled_at")?) {
        Some(at) => FulfillmentState::Cancelled { at },
        None => FulfillmentState::Shipped,
    };
    let snapshot = FulfillmentSnapshot {
        id: FulfillmentId::from_uuid(id),
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        order: OrderId::from_uuid(col(row, "order_id")?),
        location: LocationId::from_uuid(col(row, "location_id")?),
        lines: lines.remove(&id).unwrap_or_default(),
        tracking: tracking.remove(&id).unwrap_or_default(),
        state,
    };
    Fulfillment::restore(snapshot).map_err(|e| corrupt(format!("fulfilment {id}: {e}")))
}
