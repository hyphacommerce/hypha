use hypha_core::{RepoError, StoreId, Version, Versioned};
use hypha_stores::{Store, StoreRepo, StoreSnapshot};
use tokio_postgres::{GenericClient, Row, types::Type};

use crate::{
    columns::{col, version_from_db, version_to_db},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, in_transaction},
};

fn store_from_row(row: &Row) -> Result<Store, RepoError> {
    let id = StoreId::from_uuid(col(row, "id")?);
    Store::restore(StoreSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        name: col(row, "name")?,
    })
    .map_err(|e| corrupt(format!("store {id}: {e}")))
}

impl<C: PgConnection> StoreRepo for PgStore<C> {
    async fn get(&self, id: StoreId) -> Result<Option<Store>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_opt(
                "SELECT id, version, name FROM stores WHERE id = $1",
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(store_from_row).transpose()
    }

    async fn list_all(&self) -> Result<Vec<Store>, RepoError> {
        let mut conn = self.conn().await;
        conn.client()
            .query_typed("SELECT id, version, name FROM stores", &[])
            .await
            .map_err(repo_error)?
            .iter()
            .map(store_from_row)
            .collect()
    }

    async fn insert(&self, store: &mut Store) -> Result<(), RepoError> {
        let s = store.snapshot();
        in_transaction!(self, |tx| insert_store(tx, &s).await)?;
        store.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, store: &mut Store) -> Result<(), RepoError> {
        let s = store.snapshot();
        let version = in_transaction!(self, |tx| update_store(tx, &s).await)?;
        store.set_version(version);
        Ok(())
    }
}

/// `StoreRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_store(
    tx: &impl GenericClient,
    s: &StoreSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO stores (id, version, name) VALUES ($1, 1, $2)",
        &[(&s.id.as_uuid(), Type::UUID), (&s.name, Type::TEXT)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// `StoreRepo::update` inside the caller's transaction; the new version.
pub(crate) async fn update_store(
    tx: &impl GenericClient,
    s: &StoreSnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE stores SET version = version + 1, name = $3
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    version_from_db(col(&row, "version")?)
}
