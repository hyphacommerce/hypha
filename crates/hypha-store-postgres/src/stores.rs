use hypha_core::{
    Email, Language, RepoError, StoreId, Timezone, Version, Versioned, stamp_insert, stamp_update,
};
use hypha_stores::{Store, StoreRepo, StoreSnapshot};
use tokio_postgres::{
    GenericClient, Row,
    types::{ToSql, Type},
};

use crate::{
    columns::{
        ADDRESS_LEN, address_assignments, address_columns, address_from_row, address_values, col,
        id_and_address, micros, opt_micros, opt_ts, stamp_columns, stamps_from_row,
        version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, in_transaction},
};

/// `stores`' columns, as `store_from_row` reads them.
fn store_columns() -> String {
    format!(
        "id, version, name, contact_email, description, legal_name, timezone, \
         default_language, setup_step, {}, {}, {}",
        micros("setup_finished_at"),
        address_columns("legal_address_"),
        stamp_columns(""),
    )
}

fn store_from_row(row: &Row) -> Result<Store, RepoError> {
    let id = StoreId::from_uuid(col(row, "id")?);
    let bad = |e: &dyn std::fmt::Display| corrupt(format!("store {id}: {e}"));
    let email: Option<String> = col(row, "contact_email")?;
    let timezone: Option<String> = col(row, "timezone")?;
    let language: String = col(row, "default_language")?;
    Store::restore(StoreSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        name: col(row, "name")?,
        contact_email: email
            .as_deref()
            .map(Email::new)
            .transpose()
            .map_err(|e| bad(&e))?,
        description: col(row, "description")?,
        legal_name: col(row, "legal_name")?,
        legal_address: address_from_row(row, "legal_address_")?,
        timezone: timezone
            .as_deref()
            .map(Timezone::new)
            .transpose()
            .map_err(|e| bad(&e))?,
        default_language: Language::new(&language).map_err(|e| bad(&e))?,
        setup_step: col(row, "setup_step")?,
        setup_finished_at: opt_ts(col(row, "setup_finished_at")?),
    })
    .map_err(|e| bad(&e))
}

/// Sets every detail column to `s`'s (inside the caller's transaction).
async fn write_details(tx: &impl GenericClient, s: &StoreSnapshot) -> Result<(), RepoError> {
    let id = s.id.as_uuid();
    let address = address_values(s.legal_address.as_ref());
    let email = s.contact_email.as_ref().map(|e| e.as_str());
    let timezone = s.timezone.map(|t| t.as_str());
    let language = s.default_language.as_str();
    let finished = opt_micros(s.setup_finished_at);
    let mut params = id_and_address(&id, &address);
    let details: [(&(dyn ToSql + Sync), Type); 7] = [
        (&email, Type::TEXT),
        (&s.description, Type::TEXT),
        (&s.legal_name, Type::TEXT),
        (&timezone, Type::TEXT),
        (&language, Type::TEXT),
        (&s.setup_step, Type::TEXT),
        (&finished, Type::INT8),
    ];
    params.extend(details);
    let first = 2 + ADDRESS_LEN;
    tx.query_typed(
        &format!(
            "UPDATE stores SET {}, contact_email = ${}, description = ${}, legal_name = ${},
                 timezone = ${}, default_language = ${}, setup_step = ${},
                 setup_finished_at = 'epoch'::timestamptz + ${}::bigint * interval '1 microsecond'
             WHERE id = $1",
            address_assignments("legal_address_", 2),
            first,
            first + 1,
            first + 2,
            first + 3,
            first + 4,
            first + 5,
            first + 6,
        ),
        &params,
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

impl<C: PgConnection> StoreRepo for PgStore<C> {
    async fn get(&self, id: StoreId) -> Result<Option<Store>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_opt(
                &format!("SELECT {} FROM stores WHERE id = $1", store_columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(store_from_row).transpose()
    }

    async fn list_all(&self) -> Result<Vec<Store>, RepoError> {
        let mut conn = self.conn().await;
        conn.client()
            .query_typed(
                &format!("SELECT {} FROM stores ORDER BY id", store_columns()),
                &[],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(store_from_row)
            .collect()
    }

    async fn insert(&self, store: &mut Store) -> Result<(), RepoError> {
        stamp_insert(store, self.now());
        let s = store.snapshot();
        in_transaction!(self, |tx| insert_store(tx, &s).await)?;
        store.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, store: &mut Store) -> Result<(), RepoError> {
        stamp_update(store, self.now());
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
    write_details(tx, s).await?;
    write_stamps(tx, "stores", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
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
    let written = version_from_db(col(&row, "version")?)?;
    write_details(tx, s).await?;
    write_stamps(tx, "stores", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(written)
}
