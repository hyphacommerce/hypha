use std::collections::HashMap;

use futures_util::future::{try_join, try_join3};
use hypha_core::{
    InventoryItemId, LocationId, MarketId, PendingEvent, Quantity, Recorded, RepoError, VariantId,
    Version, Versioned, stamp_insert, stamp_update,
};
use hypha_inventory::{
    InventoryItem, InventoryItemRepo, InventoryItemSnapshot, Level, Location, LocationRepo,
    LocationSnapshot, Reservation, ReservationRef,
};
use tokio_postgres::types::{ToSql, Type};
use uuid::Uuid;

use crate::{
    columns::{
        address_assignments, address_columns, address_from_row, address_values, col,
        id_and_address, stamp_columns, stamps_from_row, version_from_db, version_to_db,
        write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::Queries,
    store::{PgConnection, PgStore, consistent_read, in_transaction},
};

// ---------- locations ----------

impl<C: PgConnection> LocationRepo for PgStore<C> {
    async fn get(&self, id: LocationId) -> Result<Option<Location>, RepoError> {
        let mut conn = self.conn().await;
        let ids = [id.as_uuid()];
        consistent_read!(conn, |tx| {
            Ok(load_locations(tx, Locations::Ids(&ids)).await?.pop())
        })
    }

    async fn serving(&self, market: MarketId) -> Result<Vec<Location>, RepoError> {
        let mut conn = self.conn().await;
        let market = market.as_uuid();
        consistent_read!(conn, |tx| {
            load_locations(tx, Locations::Serving(market)).await
        })
    }

    async fn list(&self) -> Result<Vec<Location>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| load_locations(tx, Locations::All).await)
    }

    async fn insert(&self, location: &mut Location) -> Result<(), RepoError> {
        stamp_insert(location, self.now());
        let s = location.snapshot();
        let events = location.take_events();
        let actor = self.actor();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_location(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        location.set_version(version);
        Ok(())
    }

    async fn update(&self, location: &mut Location) -> Result<(), RepoError> {
        stamp_update(location, self.now());
        let s = location.snapshot();
        let events = location.take_events();
        let actor = self.actor();
        let version = in_transaction!(self, |tx| {
            let version = update_location(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await?;
            Ok(version)
        })?;
        location.set_version(version);
        Ok(())
    }
}

/// `LocationRepo::insert` inside the caller's transaction: the row, its markets and
/// address.
pub(crate) async fn insert_location(
    tx: &impl Queries,
    s: &LocationSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO locations (id, version, name, priority) VALUES ($1, 1, $2, $3)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.name, Type::TEXT),
            (&i64::from(s.priority), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_location_markets(tx, s.id, &s.markets).await?;
    write_location_address(tx, s).await?;
    write_stamps(tx, "locations", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(())
}

/// `LocationRepo::update` inside the caller's transaction; the new version. The version
/// check comes first; the markets are only replaced if it passes.
pub(crate) async fn update_location(
    tx: &impl Queries,
    s: &LocationSnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE locations SET version = version + 1, name = $3, priority = $4
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&i64::from(s.priority), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    tx.query_typed(
        "DELETE FROM location_markets WHERE location_id = $1",
        &[(&s.id.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    write_location_markets(tx, s.id, &s.markets).await?;
    write_location_address(tx, s).await?;
    let written = version_from_db(col(&row, "version")?)?;
    write_stamps(tx, "locations", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(written)
}

/// All of a location's markets in one statement; `WITH ORDINALITY` keeps their order.
async fn write_location_markets(
    tx: &impl Queries,
    location: LocationId,
    markets: &[MarketId],
) -> Result<(), RepoError> {
    let markets: Vec<Uuid> = markets.iter().map(|m| m.as_uuid()).collect();
    tx.query_typed(
        "INSERT INTO location_markets (location_id, position, market_id)
         SELECT $1, (n - 1)::integer, m FROM unnest($2::uuid[]) WITH ORDINALITY AS t (m, n)",
        &[
            (&location.as_uuid(), Type::UUID),
            (&markets, Type::UUID_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// Sets the address columns to `s.address` (all NULL for none).
async fn write_location_address(tx: &impl Queries, s: &LocationSnapshot) -> Result<(), RepoError> {
    let id = s.id.as_uuid();
    let address = address_values(s.address.as_ref());
    tx.query_typed(
        &format!(
            "UPDATE locations SET {} WHERE id = $1",
            address_assignments("address_", 2),
        ),
        &id_and_address(&id, &address),
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// The locations with these IDs and their markets, by priority then ID, in 2 queries.
/// Which locations a load is for: the filter on `locations l` both of its queries
/// share, so they go out together (pipelined) and read the same snapshot.
enum Locations<'a> {
    Ids(&'a [Uuid]),
    Serving(Uuid),
    All,
}

async fn load_locations(
    client: &impl Queries,
    which: Locations<'_>,
) -> Result<Vec<Location>, RepoError> {
    let (filter, param): (&str, [(&(dyn ToSql + Sync), Type); 1]) = match &which {
        Locations::Ids(ids) => ("l.id = ANY($1)", [(ids, Type::UUID_ARRAY)]),
        Locations::Serving(market) => (
            "l.id IN (SELECT location_id FROM location_markets WHERE market_id = $1)",
            [(market, Type::UUID)],
        ),
        Locations::All => ("$1::uuid IS NULL", [(&None::<Uuid>, Type::UUID)]),
    };
    let locations_sql = format!(
        "SELECT l.id, l.version, l.name, l.priority, {}, {}
         FROM locations l WHERE {filter}
         ORDER BY l.priority, l.id",
        address_columns("l.address_"),
        stamp_columns("l."),
    );
    let markets_sql = format!(
        "SELECT lm.location_id, lm.market_id FROM location_markets lm
         JOIN locations l ON l.id = lm.location_id
         WHERE {filter} ORDER BY lm.location_id, lm.position"
    );
    let (rows, market_rows) = try_join(
        client.query_typed(&locations_sql, &param),
        client.query_typed(&markets_sql, &param),
    )
    .await
    .map_err(repo_error)?;
    let mut markets: HashMap<Uuid, Vec<MarketId>> = HashMap::new();
    for r in market_rows {
        markets
            .entry(col(&r, "location_id")?)
            .or_default()
            .push(MarketId::from_uuid(col(&r, "market_id")?));
    }

    rows.iter()
        .map(|r| {
            let id: Uuid = col(r, "id")?;
            let priority: i64 = col(r, "priority")?;
            let snapshot = LocationSnapshot {
                id: LocationId::from_uuid(id),
                version: version_from_db(col(r, "version")?)?,
                stamps: stamps_from_row(r)?,
                name: col(r, "name")?,
                markets: markets.remove(&id).unwrap_or_default(),
                priority: u32::try_from(priority)
                    .map_err(|_| corrupt(format!("location priority {priority}")))?,
                address: address_from_row(r, "address_")?,
            };
            Location::restore(snapshot).map_err(|e| corrupt(format!("location {id}: {e}")))
        })
        .collect()
}

// ---------- items ----------

impl<C: PgConnection> InventoryItemRepo for PgStore<C> {
    async fn get(&self, id: InventoryItemId) -> Result<Option<InventoryItem>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            Ok(load_items(tx, "id", &[id.as_uuid()]).await?.pop())
        })
    }

    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<InventoryItem>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            Ok(load_items(tx, "variant_id", &[variant.as_uuid()])
                .await?
                .pop())
        })
    }

    async fn for_variants(&self, variants: &[VariantId]) -> Result<Vec<InventoryItem>, RepoError> {
        if variants.is_empty() {
            return Ok(vec![]);
        }
        let ids: Vec<Uuid> = variants.iter().map(|v| v.as_uuid()).collect();
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| load_items(tx, "variant_id", &ids).await)
    }

    async fn insert(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        stamp_insert(item, self.now());
        let s = item.snapshot();
        let events = item.take_events();
        let actor = self.actor();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_item(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        item.set_version(version);
        Ok(())
    }

    async fn delete(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        let id = item.id().as_uuid();
        let events = item.take_events();
        let actor = self.actor();
        in_transaction!(self, |tx| {
            // Levels and reservations go with it (ON DELETE CASCADE).
            tx.query_typed(
                "DELETE FROM inventory_items WHERE id = $1",
                &[(&id, Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
            insert_events(tx, &events, None, actor).await
        })
    }

    async fn update(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        self.update_many(std::slice::from_mut(item)).await
    }

    /// One transaction of 5 statements however many items: bump every version at once
    /// (stale if fewer rows match than items were given), then replace the children.
    async fn update_many(&self, items: &mut [InventoryItem]) -> Result<(), RepoError> {
        if items.is_empty() {
            return Ok(());
        }
        let now = self.now();
        for item in items.iter_mut() {
            stamp_update(item, now);
        }
        let snapshots: Vec<InventoryItemSnapshot> = items.iter().map(|i| i.snapshot()).collect();
        let events: Vec<(Uuid, Vec<PendingEvent>)> = items
            .iter_mut()
            .map(|i| (i.id().as_uuid(), i.take_events()))
            .collect();
        let actor = self.actor();
        let new_versions = in_transaction!(self, |tx| {
            let versions = update_items(tx, snapshots).await?;
            for (id, events) in &events {
                insert_events(tx, events, versions.get(id).copied(), actor).await?;
            }
            Ok(versions)
        })?;
        for item in items.iter_mut() {
            if let Some(version) = new_versions.get(&item.id().as_uuid()) {
                item.set_version(*version);
            }
        }
        Ok(())
    }
}

/// `InventoryItemRepo::insert` inside the caller's transaction: the row, then its levels
/// and reservations.
pub(crate) async fn insert_item(
    tx: &impl Queries,
    s: &InventoryItemSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO inventory_items (id, version, variant_id, tracked, continue_selling,
             retired)
         VALUES ($1, 1, $2, $3, $4, $5)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.variant.as_uuid(), Type::UUID),
            (&s.tracked, Type::BOOL),
            (&s.continue_selling, Type::BOOL),
            (&s.retired, Type::BOOL),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_children(tx, std::slice::from_ref(s)).await?;
    write_stamps(
        tx,
        "inventory_items",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await?;
    Ok(())
}

/// Locks the rows of `ids` in ID order (`FOR UPDATE`), so every writer of several items
/// takes their locks in one order and queues instead of deadlocking. A batch that
/// updates some items and deletes others locks them all at once, before either.
pub(crate) async fn lock_items(tx: &impl Queries, ids: &[Uuid]) -> Result<(), RepoError> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut sorted = ids.to_vec();
    sorted.sort();
    tx.query_typed(
        "SELECT id FROM inventory_items WHERE id = ANY($1) ORDER BY id FOR UPDATE",
        &[(&sorted, Type::UUID_ARRAY)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// `StageInventoryItems::delete_inventory_item`'s writes inside the caller's
/// transaction (`PgBatch`): every item at the version it was loaded at, or
/// `StaleVersion` and the caller drops the transaction. Levels and reservations go with
/// them (ON DELETE CASCADE).
pub(crate) async fn delete_items(
    tx: &impl Queries,
    snapshots: &[InventoryItemSnapshot],
) -> Result<(), RepoError> {
    if snapshots.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = snapshots.iter().map(|s| s.id.as_uuid()).collect();
    let versions = snapshots
        .iter()
        .map(|s| version_to_db(s.version))
        .collect::<Result<Vec<i64>, _>>()?;
    let removed = tx
        .query_typed(
            "DELETE FROM inventory_items AS i USING unnest($1::uuid[], $2::bigint[])
                 AS t (id, version)
             WHERE i.id = t.id AND i.version = t.version RETURNING i.id",
            &[(&ids, Type::UUID_ARRAY), (&versions, Type::INT8_ARRAY)],
        )
        .await
        .map_err(repo_error)?;
    if removed.len() != snapshots.len() {
        return Err(RepoError::StaleVersion);
    }
    Ok(())
}

/// `InventoryItemRepo::update_many` inside the caller's transaction (also used by
/// `PgBatch`): every item or none. Returns each item's new version; on `StaleVersion`
/// the caller drops the transaction, rolling it back.
pub(crate) async fn update_items(
    tx: &impl Queries,
    mut snapshots: Vec<InventoryItemSnapshot>,
) -> Result<HashMap<Uuid, Version>, RepoError> {
    if snapshots.is_empty() {
        return Ok(HashMap::new());
    }
    // Sorted by ID, so every writer takes its row locks in the same order (below).
    snapshots.sort_by_key(|s| s.id.as_uuid());
    let ids: Vec<Uuid> = snapshots.iter().map(|s| s.id.as_uuid()).collect();
    let versions = snapshots
        .iter()
        .map(|s| version_to_db(s.version))
        .collect::<Result<Vec<i64>, _>>()?;
    let variants: Vec<Uuid> = snapshots.iter().map(|s| s.variant.as_uuid()).collect();
    let tracked: Vec<bool> = snapshots.iter().map(|s| s.tracked).collect();
    let continue_selling: Vec<bool> = snapshots.iter().map(|s| s.continue_selling).collect();
    let retired: Vec<bool> = snapshots.iter().map(|s| s.retired).collect();

    // Lock the rows in ID order first. Two checkouts touching overlapping items then
    // queue instead of deadlocking, whatever order their callers listed the items in;
    // the UPDATE below joins through `unnest`, whose lock order the planner picks.
    lock_items(tx, &ids).await?;
    let bumped = tx
        .query_typed(
            "UPDATE inventory_items AS i
             SET version = i.version + 1, variant_id = t.variant_id, tracked = t.tracked,
                 continue_selling = t.continue_selling, retired = t.retired
             FROM unnest($1::uuid[], $2::bigint[], $3::uuid[], $4::boolean[], $5::boolean[],
                         $6::boolean[])
                 AS t (id, version, variant_id, tracked, continue_selling, retired)
             WHERE i.id = t.id AND i.version = t.version
             RETURNING i.id, i.version",
            &[
                (&ids, Type::UUID_ARRAY),
                (&versions, Type::INT8_ARRAY),
                (&variants, Type::UUID_ARRAY),
                (&tracked, Type::BOOL_ARRAY),
                (&continue_selling, Type::BOOL_ARRAY),
                (&retired, Type::BOOL_ARRAY),
            ],
        )
        .await
        .map_err(repo_error)?;
    if bumped.len() != snapshots.len() {
        // Some item changed or is gone: write none of them.
        return Err(RepoError::StaleVersion);
    }
    let new_versions = bumped
        .iter()
        .map(|r| Ok((col::<Uuid>(r, "id")?, version_from_db(col(r, "version")?)?)))
        .collect::<Result<HashMap<Uuid, Version>, RepoError>>()?;

    for table in ["inventory_levels", "inventory_reservations"] {
        tx.query_typed(
            &format!("DELETE FROM {table} WHERE item_id = ANY($1)"),
            &[(&ids, Type::UUID_ARRAY)],
        )
        .await
        .map_err(repo_error)?;
    }
    write_children(tx, &snapshots).await?;
    // Their stamps, one statement for all.
    let stamped: Vec<&InventoryItemSnapshot> =
        snapshots.iter().filter(|s| s.stamps.is_some()).collect();
    if !stamped.is_empty() {
        let ids: Vec<Uuid> = stamped.iter().map(|s| s.id.as_uuid()).collect();
        let created: Vec<i64> = stamped
            .iter()
            .filter_map(|s| s.stamps.map(|t| t.created_at.unix_micros()))
            .collect();
        let updated: Vec<i64> = stamped
            .iter()
            .filter_map(|s| s.stamps.map(|t| t.updated_at.unix_micros()))
            .collect();
        tx.query_typed(
            "UPDATE inventory_items i
             SET created_at = 'epoch'::timestamptz + t.c * interval '1 microsecond',
                 updated_at = 'epoch'::timestamptz + t.u * interval '1 microsecond'
             FROM unnest($1::uuid[], $2::bigint[], $3::bigint[]) AS t (id, c, u)
             WHERE i.id = t.id",
            &[
                (&ids, Type::UUID_ARRAY),
                (&created, Type::INT8_ARRAY),
                (&updated, Type::INT8_ARRAY),
            ],
        )
        .await
        .map_err(repo_error)?;
    }
    Ok(new_versions)
}

/// The levels and reservations of `items`, one statement each. Positions are counted
/// per item, so each item's order survives.
async fn write_children(
    tx: &impl Queries,
    items: &[InventoryItemSnapshot],
) -> Result<(), RepoError> {
    let mut level_items = Vec::new();
    let mut level_positions: Vec<i32> = Vec::new();
    let mut level_locations = Vec::new();
    let mut on_hand = Vec::new();
    let mut held_items = Vec::new();
    let mut held_positions: Vec<i32> = Vec::new();
    let mut references = Vec::new();
    let mut held_locations = Vec::new();
    let mut quantities = Vec::new();
    for item in items {
        for (position, level) in (0..).zip(&item.levels) {
            level_items.push(item.id.as_uuid());
            level_positions.push(position);
            level_locations.push(level.location.as_uuid());
            on_hand.push(level.on_hand);
        }
        for (position, r) in (0..).zip(&item.reservations) {
            held_items.push(item.id.as_uuid());
            held_positions.push(position);
            references.push(r.reference.as_str());
            held_locations.push(r.location.as_uuid());
            quantities.push(i64::from(r.quantity.value()));
        }
    }
    tx.query_typed(
        "INSERT INTO inventory_levels (item_id, position, location_id, on_hand)
         SELECT * FROM unnest($1::uuid[], $2::integer[], $3::uuid[], $4::bigint[])",
        &[
            (&level_items, Type::UUID_ARRAY),
            (&level_positions, Type::INT4_ARRAY),
            (&level_locations, Type::UUID_ARRAY),
            (&on_hand, Type::INT8_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    tx.query_typed(
        "INSERT INTO inventory_reservations (item_id, position, reference, location_id, quantity)
         SELECT * FROM unnest($1::uuid[], $2::integer[], $3::text[], $4::uuid[], $5::bigint[])",
        &[
            (&held_items, Type::UUID_ARRAY),
            (&held_positions, Type::INT4_ARRAY),
            (&references, Type::TEXT_ARRAY),
            (&held_locations, Type::UUID_ARRAY),
            (&quantities, Type::INT8_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// The items whose `column` (`id` or `variant_id`) is in `values`, with their levels
/// and reservations, in 3 queries.
/// Loads the inventory items whose `column` is one of `values`, in three queries sent at
/// once: the levels and reservations select their rows through the same filter (a join),
/// so they don't wait for the items' IDs, and all three read the same snapshot.
async fn load_items(
    client: &impl Queries,
    column: &'static str,
    values: &[Uuid],
) -> Result<Vec<InventoryItem>, RepoError> {
    if values.is_empty() {
        return Ok(vec![]);
    }
    let param: [(&(dyn ToSql + Sync), Type); 1] = [(&values, Type::UUID_ARRAY)];
    let items_sql = format!(
        "SELECT i.id, i.version, i.variant_id, i.tracked, i.continue_selling, i.retired, {}
         FROM inventory_items i WHERE i.{column} = ANY($1) ORDER BY i.id",
        stamp_columns("i."),
    );
    let levels_sql = format!(
        "SELECT l.item_id, l.location_id, l.on_hand FROM inventory_levels l
         JOIN inventory_items i ON i.id = l.item_id
         WHERE i.{column} = ANY($1) ORDER BY l.item_id, l.position"
    );
    let reservations_sql = format!(
        "SELECT r.item_id, r.reference, r.location_id, r.quantity FROM inventory_reservations r
         JOIN inventory_items i ON i.id = r.item_id
         WHERE i.{column} = ANY($1) ORDER BY r.item_id, r.position"
    );
    let (rows, level_rows, reservation_rows) = try_join3(
        client.query_typed(&items_sql, &param),
        client.query_typed(&levels_sql, &param),
        client.query_typed(&reservations_sql, &param),
    )
    .await
    .map_err(repo_error)?;
    if rows.is_empty() {
        return Ok(vec![]);
    }

    let mut levels: HashMap<Uuid, Vec<Level>> = HashMap::new();
    for r in level_rows {
        levels.entry(col(&r, "item_id")?).or_default().push(Level {
            location: LocationId::from_uuid(col(&r, "location_id")?),
            on_hand: col(&r, "on_hand")?,
        });
    }

    let mut reservations: HashMap<Uuid, Vec<Reservation>> = HashMap::new();
    for r in reservation_rows {
        let reference: String = col(&r, "reference")?;
        let quantity: i64 = col(&r, "quantity")?;
        reservations
            .entry(col(&r, "item_id")?)
            .or_default()
            .push(Reservation {
                reference: ReservationRef::new(&reference).map_err(corrupt)?,
                location: LocationId::from_uuid(col(&r, "location_id")?),
                quantity: u32::try_from(quantity)
                    .ok()
                    .and_then(|q| Quantity::new(q).ok())
                    .ok_or_else(|| corrupt(format!("reservation quantity {quantity}")))?,
            });
    }

    rows.iter()
        .map(|r| {
            let id: Uuid = col(r, "id")?;
            let snapshot = InventoryItemSnapshot {
                id: InventoryItemId::from_uuid(id),
                version: version_from_db(col(r, "version")?)?,
                stamps: stamps_from_row(r)?,
                variant: VariantId::from_uuid(col(r, "variant_id")?),
                tracked: col(r, "tracked")?,
                continue_selling: col(r, "continue_selling")?,
                retired: col(r, "retired")?,
                levels: levels.remove(&id).unwrap_or_default(),
                reservations: reservations.remove(&id).unwrap_or_default(),
            };
            InventoryItem::restore(snapshot)
                .map_err(|e| corrupt(format!("inventory item {id}: {e}")))
        })
        .collect()
}
