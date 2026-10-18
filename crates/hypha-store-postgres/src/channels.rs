//! Channels and their publications (design note 0020).

use std::collections::{HashMap, HashSet};

use hypha_channels::{Channel, ChannelRepo, ChannelSnapshot};
use hypha_core::{
    ChannelId, ProductId, Recorded, RepoError, Version, Versioned, stamp_insert, stamp_update,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{col, stamp_columns, stamps_from_row, version_from_db, version_to_db, write_stamps},
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::Queries,
    store::{PgConnection, PgStore, in_transaction},
};

fn channel_from_row(row: &Row) -> Result<Channel, RepoError> {
    let id = ChannelId::from_uuid(col(row, "id")?);
    let kind: String = col(row, "kind")?;
    Channel::restore(ChannelSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        name: col(row, "name")?,
        kind: kind
            .parse()
            .map_err(|k| corrupt(format!("channel {id} kind {k:?}")))?,
    })
    .map_err(|e| corrupt(format!("channel {id}: {e}")))
}

fn uuids(products: &[ProductId]) -> Vec<Uuid> {
    products.iter().map(|p| p.as_uuid()).collect()
}

impl<C: PgConnection> ChannelRepo for PgStore<C> {
    async fn get(&self, id: ChannelId) -> Result<Option<Channel>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT id, version, name, kind, {} FROM channels WHERE id = $1",
                    stamp_columns("")
                ),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(channel_from_row).transpose()
    }

    async fn find_by_name(&self, name_key: &str) -> Result<Option<Channel>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT id, version, name, kind, {} FROM channels WHERE name_key = $1",
                    stamp_columns("")
                ),
                &[(&name_key, Type::TEXT)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(channel_from_row).transpose()
    }

    async fn list(&self) -> Result<Vec<Channel>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT id, version, name, kind, {} FROM channels ORDER BY created_at, id",
                    stamp_columns("")
                ),
                &[],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(channel_from_row)
            .collect()
    }

    async fn insert(&self, channel: &mut Channel) -> Result<(), RepoError> {
        stamp_insert(channel, self.now());
        let s = channel.snapshot();
        let name_key = channel.name_key();
        let events = channel.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            tx.query_typed(
                "INSERT INTO channels (id, version, name, name_key, kind)
                 VALUES ($1, 1, $2, $3, $4)",
                &[
                    (&s.id.as_uuid(), Type::UUID),
                    (&s.name, Type::TEXT),
                    (&name_key, Type::TEXT),
                    (&s.kind.as_str(), Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
            write_stamps(tx, "channels", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        channel.set_version(version);
        Ok(())
    }

    async fn update(&self, channel: &mut Channel) -> Result<(), RepoError> {
        stamp_update(channel, self.now());
        let s = channel.snapshot();
        let name_key = channel.name_key();
        let events = channel.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let row = tx
                .query_typed_opt(
                    "UPDATE channels SET version = version + 1, name = $3, name_key = $4
                     WHERE id = $1 AND version = $2 RETURNING version",
                    &[
                        (&s.id.as_uuid(), Type::UUID),
                        (&version_to_db(s.version)?, Type::INT8),
                        (&s.name, Type::TEXT),
                        (&name_key, Type::TEXT),
                    ],
                )
                .await
                .map_err(repo_error)?
                .ok_or(RepoError::StaleVersion)?;
            let version = version_from_db(col(&row, "version")?)?;
            write_stamps(tx, "channels", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        channel.set_version(version);
        Ok(())
    }

    async fn publish(&self, channel: ChannelId, products: &[ProductId]) -> Result<(), RepoError> {
        let mut conn = self.conn().await;
        publish(&conn.db(), channel, products).await
    }

    async fn unpublish(&self, channel: ChannelId, products: &[ProductId]) -> Result<(), RepoError> {
        let mut conn = self.conn().await;
        unpublish(&conn.db(), channel, products).await
    }

    async fn published(
        &self,
        channel: ChannelId,
        products: &[ProductId],
    ) -> Result<HashSet<ProductId>, RepoError> {
        if products.is_empty() {
            return Ok(HashSet::new());
        }
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT product_id FROM channel_products
                 WHERE channel_id = $1 AND product_id = ANY($2)",
                &[
                    (&channel.as_uuid(), Type::UUID),
                    (&uuids(products), Type::UUID_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?;
        rows.iter()
            .map(|row| Ok(ProductId::from_uuid(col(row, "product_id")?)))
            .collect()
    }

    async fn channels_of(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, Vec<ChannelId>>, RepoError> {
        if products.is_empty() {
            return Ok(HashMap::new());
        }
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT product_id, channel_id FROM channel_products
                 WHERE product_id = ANY($1) ORDER BY product_id, channel_id",
                &[(&uuids(products), Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        let mut found: HashMap<ProductId, Vec<ChannelId>> = HashMap::new();
        for row in &rows {
            found
                .entry(ProductId::from_uuid(col(row, "product_id")?))
                .or_default()
                .push(ChannelId::from_uuid(col(row, "channel_id")?));
        }
        Ok(found)
    }
}

/// `ChannelRepo::publish` inside the caller's transaction (also a batch's). A missing
/// channel fails on its foreign key.
pub(crate) async fn publish(
    tx: &impl Queries,
    channel: ChannelId,
    products: &[ProductId],
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO channel_products (channel_id, product_id)
         SELECT $1, product_id FROM unnest($2::uuid[]) AS t (product_id)
         ON CONFLICT DO NOTHING",
        &[
            (&channel.as_uuid(), Type::UUID),
            (&uuids(products), Type::UUID_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// `ChannelRepo::unpublish` inside the caller's transaction.
pub(crate) async fn unpublish(
    tx: &impl Queries,
    channel: ChannelId,
    products: &[ProductId],
) -> Result<(), RepoError> {
    tx.query_typed(
        "DELETE FROM channel_products WHERE channel_id = $1 AND product_id = ANY($2)",
        &[
            (&channel.as_uuid(), Type::UUID),
            (&uuids(products), Type::UUID_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}
