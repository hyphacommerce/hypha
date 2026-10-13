use std::collections::HashMap;

use hypha_core::{InventoryItemId, LocationId, MarketId, RepoError, VariantId, Version, Versioned};
use hypha_inventory::{
    InventoryItem, InventoryItemRepo, InventoryItemSnapshot, Level, Location, LocationRepo,
    LocationSnapshot, Reservation, ReservationRef,
};
use tokio_postgres::{Client, GenericClient, types::Type};
use uuid::Uuid;

use crate::{
    columns::{version_from_db, version_to_db},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore},
};

// ---------- locations ----------

impl<C: PgConnection> LocationRepo for PgStore<C> {
    async fn get(&self, id: LocationId) -> Result<Option<Location>, RepoError> {
        let mut conn = self.conn().await;
        let ids = [id.as_uuid()];
        Ok(load_locations(conn.client(), &ids).await?.pop())
    }

    async fn serving(&self, market: MarketId) -> Result<Vec<Location>, RepoError> {
        let mut conn = self.conn().await;
        let client = conn.client();
        let ids: Vec<Uuid> = client
            .query_typed(
                "SELECT location_id FROM location_markets WHERE market_id = $1",
                &[(&market.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|r| r.get(0))
            .collect();
        load_locations(client, &ids).await
    }

    async fn insert(&self, location: &mut Location) -> Result<(), RepoError> {
        let s = location.snapshot();
        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
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
        write_location_markets(&tx, s.id, &s.markets).await?;
        tx.commit().await.map_err(repo_error)?;
        location.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, location: &mut Location) -> Result<(), RepoError> {
        let s = location.snapshot();
        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        // The version check comes first; the markets are only replaced if it passes.
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
        let Some(row) = bumped else {
            tx.rollback().await.map_err(repo_error)?;
            return Err(RepoError::StaleVersion);
        };
        let new_version = version_from_db(row.get(0))?;
        tx.query_typed(
            "DELETE FROM location_markets WHERE location_id = $1",
            &[(&s.id.as_uuid(), Type::UUID)],
        )
        .await
        .map_err(repo_error)?;
        write_location_markets(&tx, s.id, &s.markets).await?;
        tx.commit().await.map_err(repo_error)?;
        location.set_version(new_version);
        Ok(())
    }
}

/// All of a location's markets in one statement; `WITH ORDINALITY` keeps their order.
async fn write_location_markets(
    tx: &impl GenericClient,
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

/// The locations with these IDs and their markets, by priority then ID, in 2 queries.
async fn load_locations(client: &Client, ids: &[Uuid]) -> Result<Vec<Location>, RepoError> {
    let rows = client
        .query_typed(
            "SELECT id, version, name, priority FROM locations WHERE id = ANY($1)
             ORDER BY priority, id",
            &[(&ids, Type::UUID_ARRAY)],
        )
        .await
        .map_err(repo_error)?;
    let found: Vec<Uuid> = rows.iter().map(|r| r.get(0)).collect();
    let mut markets: HashMap<Uuid, Vec<MarketId>> = HashMap::new();
    for r in client
        .query_typed(
            "SELECT location_id, market_id FROM location_markets
             WHERE location_id = ANY($1) ORDER BY location_id, position",
            &[(&found, Type::UUID_ARRAY)],
        )
        .await
        .map_err(repo_error)?
    {
        markets
            .entry(r.get(0))
            .or_default()
            .push(MarketId::from_uuid(r.get(1)));
    }

    rows.iter()
        .map(|r| {
            let id: Uuid = r.get(0);
            let priority: i64 = r.get(3);
            let snapshot = LocationSnapshot {
                id: LocationId::from_uuid(id),
                version: version_from_db(r.get(1))?,
                name: r.get(2),
                markets: markets.remove(&id).unwrap_or_default(),
                priority: u32::try_from(priority)
                    .map_err(|_| corrupt(format!("location priority {priority}")))?,
            };
            Location::restore(snapshot).map_err(|e| corrupt(format!("location {id}: {e}")))
        })
        .collect()
}

// ---------- items ----------

impl<C: PgConnection> InventoryItemRepo for PgStore<C> {
    async fn get(&self, id: InventoryItemId) -> Result<Option<InventoryItem>, RepoError> {
        let mut conn = self.conn().await;
        Ok(load_items(conn.client(), "id", &[id.as_uuid()])
            .await?
            .pop())
    }

    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<InventoryItem>, RepoError> {
        let mut conn = self.conn().await;
        Ok(
            load_items(conn.client(), "variant_id", &[variant.as_uuid()])
                .await?
                .pop(),
        )
    }

    async fn for_variants(&self, variants: &[VariantId]) -> Result<Vec<InventoryItem>, RepoError> {
        let ids: Vec<Uuid> = variants.iter().map(|v| v.as_uuid()).collect();
        let mut conn = self.conn().await;
        load_items(conn.client(), "variant_id", &ids).await
    }

    async fn insert(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        let s = item.snapshot();
        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        tx.query_typed(
            "INSERT INTO inventory_items (id, version, variant_id, tracked, continue_selling)
             VALUES ($1, 1, $2, $3, $4)",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&s.variant.as_uuid(), Type::UUID),
                (&s.tracked, Type::BOOL),
                (&s.continue_selling, Type::BOOL),
            ],
        )
        .await
        .map_err(repo_error)?;
        write_children(&tx, std::slice::from_ref(&s)).await?;
        tx.commit().await.map_err(repo_error)?;
        item.set_version(Version::NEW.next());
        Ok(())
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
        let snapshots: Vec<InventoryItemSnapshot> = items.iter().map(|i| i.snapshot()).collect();
        let ids: Vec<Uuid> = snapshots.iter().map(|s| s.id.as_uuid()).collect();
        let versions = snapshots
            .iter()
            .map(|s| version_to_db(s.version))
            .collect::<Result<Vec<i64>, _>>()?;
        let variants: Vec<Uuid> = snapshots.iter().map(|s| s.variant.as_uuid()).collect();
        let tracked: Vec<bool> = snapshots.iter().map(|s| s.tracked).collect();
        let continue_selling: Vec<bool> = snapshots.iter().map(|s| s.continue_selling).collect();

        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        let bumped = tx
            .query_typed(
                "UPDATE inventory_items AS i
                 SET version = i.version + 1, variant_id = t.variant_id, tracked = t.tracked,
                     continue_selling = t.continue_selling
                 FROM unnest($1::uuid[], $2::bigint[], $3::uuid[], $4::boolean[], $5::boolean[])
                     AS t (id, version, variant_id, tracked, continue_selling)
                 WHERE i.id = t.id AND i.version = t.version
                 RETURNING i.id, i.version",
                &[
                    (&ids, Type::UUID_ARRAY),
                    (&versions, Type::INT8_ARRAY),
                    (&variants, Type::UUID_ARRAY),
                    (&tracked, Type::BOOL_ARRAY),
                    (&continue_selling, Type::BOOL_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?;
        if bumped.len() != items.len() {
            // Some item changed or is gone: write none of them.
            tx.rollback().await.map_err(repo_error)?;
            return Err(RepoError::StaleVersion);
        }
        let new_versions = bumped
            .iter()
            .map(|r| Ok((r.get::<_, Uuid>(0), version_from_db(r.get(1))?)))
            .collect::<Result<HashMap<Uuid, Version>, RepoError>>()?;

        for table in ["inventory_levels", "inventory_reservations"] {
            tx.query_typed(
                &format!("DELETE FROM {table} WHERE item_id = ANY($1)"),
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        }
        write_children(&tx, &snapshots).await?;
        tx.commit().await.map_err(repo_error)?;

        for item in items.iter_mut() {
            if let Some(version) = new_versions.get(&item.id().as_uuid()) {
                item.set_version(*version);
            }
        }
        Ok(())
    }
}

/// The levels and reservations of `items`, one statement each. Positions are counted
/// per item, so each item's order survives.
async fn write_children(
    tx: &impl GenericClient,
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
            quantities.push(i64::from(r.quantity));
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
async fn load_items(
    client: &Client,
    column: &'static str,
    values: &[Uuid],
) -> Result<Vec<InventoryItem>, RepoError> {
    let rows = client
        .query_typed(
            &format!(
                "SELECT id, version, variant_id, tracked, continue_selling FROM inventory_items
                 WHERE {column} = ANY($1) ORDER BY id"
            ),
            &[(&values, Type::UUID_ARRAY)],
        )
        .await
        .map_err(repo_error)?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.get(0)).collect();

    let mut levels: HashMap<Uuid, Vec<Level>> = HashMap::new();
    for r in client
        .query_typed(
            "SELECT item_id, location_id, on_hand FROM inventory_levels
             WHERE item_id = ANY($1) ORDER BY item_id, position",
            &[(&ids, Type::UUID_ARRAY)],
        )
        .await
        .map_err(repo_error)?
    {
        levels.entry(r.get(0)).or_default().push(Level {
            location: LocationId::from_uuid(r.get(1)),
            on_hand: r.get(2),
        });
    }

    let mut reservations: HashMap<Uuid, Vec<Reservation>> = HashMap::new();
    for r in client
        .query_typed(
            "SELECT item_id, reference, location_id, quantity FROM inventory_reservations
             WHERE item_id = ANY($1) ORDER BY item_id, position",
            &[(&ids, Type::UUID_ARRAY)],
        )
        .await
        .map_err(repo_error)?
    {
        let reference: String = r.get(1);
        let quantity: i64 = r.get(3);
        reservations.entry(r.get(0)).or_default().push(Reservation {
            reference: ReservationRef::new(&reference).map_err(corrupt)?,
            location: LocationId::from_uuid(r.get(2)),
            quantity: u32::try_from(quantity)
                .map_err(|_| corrupt(format!("reservation quantity {quantity}")))?,
        });
    }

    rows.iter()
        .map(|r| {
            let id: Uuid = r.get(0);
            let snapshot = InventoryItemSnapshot {
                id: InventoryItemId::from_uuid(id),
                version: version_from_db(r.get(1))?,
                variant: VariantId::from_uuid(r.get(2)),
                tracked: r.get(3),
                continue_selling: r.get(4),
                levels: levels.remove(&id).unwrap_or_default(),
                reservations: reservations.remove(&id).unwrap_or_default(),
            };
            InventoryItem::restore(snapshot)
                .map_err(|e| corrupt(format!("inventory item {id}: {e}")))
        })
        .collect()
}
