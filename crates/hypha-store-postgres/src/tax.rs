use std::collections::HashMap;

use hypha_core::{
    CountryCode, ProductId, Recorded, RepoError, TaxClassId, TaxRegionId, Version, Versioned,
    stamp_insert, stamp_update,
};
use hypha_tax::{
    ClassRate, TaxClass, TaxClassRepo, TaxClassSnapshot, TaxRegion, TaxRegionRepo,
    TaxRegionSnapshot,
};
use tokio_postgres::{GenericClient, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        col, stamp_columns, stamps_from_row, tax_rate_from_db, tax_rate_to_db, version_from_db,
        version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, consistent_read, in_transaction},
};

// ---------- classes ----------

impl<C: PgConnection> TaxClassRepo for PgStore<C> {
    async fn get(&self, id: TaxClassId) -> Result<Option<TaxClass>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_opt(
                &format!(
                    "SELECT version, name, {} FROM tax_classes WHERE id = $1",
                    stamp_columns(""),
                ),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = row else { return Ok(None) };
        TaxClass::restore(TaxClassSnapshot {
            id,
            version: version_from_db(col(&row, "version")?)?,
            stamps: stamps_from_row(&row)?,
            name: col(&row, "name")?,
        })
        .map(Some)
        .map_err(|e| corrupt(format!("tax class {id}: {e}")))
    }

    async fn insert(&self, class: &mut TaxClass) -> Result<(), RepoError> {
        stamp_insert(class, self.now());
        let s = class.snapshot();
        let events = class.take_events();
        let actor = self.actor();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_tax_class(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        class.set_version(version);
        Ok(())
    }

    async fn assign(
        &self,
        products: &[ProductId],
        class: Option<TaxClassId>,
    ) -> Result<(), RepoError> {
        in_transaction!(self, |tx| assign_tax_class(tx, products, class).await)
    }

    async fn classes_for(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, TaxClassId>, RepoError> {
        if products.is_empty() {
            return Ok(HashMap::new());
        }
        let ids: Vec<Uuid> = products.iter().map(|p| p.as_uuid()).collect();
        let mut conn = self.conn().await;
        let rows = conn
            .client()
            .query_typed(
                "SELECT product_id, tax_class_id FROM product_tax_classes
                 WHERE product_id = ANY($1)",
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter()
            .map(|r| {
                Ok((
                    ProductId::from_uuid(col(r, "product_id")?),
                    TaxClassId::from_uuid(col(r, "tax_class_id")?),
                ))
            })
            .collect()
    }
}

/// `TaxClassRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_tax_class(
    tx: &impl GenericClient,
    s: &TaxClassSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO tax_classes (id, version, name) VALUES ($1, 1, $2)",
        &[(&s.id.as_uuid(), Type::UUID), (&s.name, Type::TEXT)],
    )
    .await
    .map_err(repo_error)?;
    let written = ();
    write_stamps(
        tx,
        "tax_classes",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await?;
    Ok(written)
}

/// `TaxClassRepo::assign` inside the caller's transaction: one statement either way.
pub(crate) async fn assign_tax_class(
    tx: &impl GenericClient,
    products: &[ProductId],
    class: Option<TaxClassId>,
) -> Result<(), RepoError> {
    let ids: Vec<Uuid> = products.iter().map(|p| p.as_uuid()).collect();
    match class {
        Some(class) => tx
            .query_typed(
                "INSERT INTO product_tax_classes (product_id, tax_class_id)
                 SELECT product_id, $2 FROM unnest($1::uuid[]) AS t (product_id)
                 ON CONFLICT (product_id) DO UPDATE SET tax_class_id = EXCLUDED.tax_class_id",
                &[(&ids, Type::UUID_ARRAY), (&class.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?,
        None => tx
            .query_typed(
                "DELETE FROM product_tax_classes WHERE product_id = ANY($1)",
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?,
    };
    Ok(())
}

// ---------- regions ----------

impl<C: PgConnection> TaxRegionRepo for PgStore<C> {
    async fn get(&self, id: TaxRegionId) -> Result<Option<TaxRegion>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            load_region(tx, "id = $1", id.as_uuid(), Type::UUID).await
        })
    }

    async fn get_by_country(&self, country: CountryCode) -> Result<Option<TaxRegion>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            load_region(tx, "country = $1", country.as_str(), Type::TEXT).await
        })
    }

    async fn insert(&self, region: &mut TaxRegion) -> Result<(), RepoError> {
        stamp_insert(region, self.now());
        let s = region.snapshot();
        let events = region.take_events();
        let actor = self.actor();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_tax_region(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        region.set_version(version);
        Ok(())
    }

    async fn update(&self, region: &mut TaxRegion) -> Result<(), RepoError> {
        stamp_update(region, self.now());
        let s = region.snapshot();
        let events = region.take_events();
        let actor = self.actor();
        let version = in_transaction!(self, |tx| {
            let version = update_tax_region(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await?;
            Ok(version)
        })?;
        region.set_version(version);
        Ok(())
    }
}

/// `TaxRegionRepo::insert` inside the caller's transaction: the row, then its rates.
pub(crate) async fn insert_tax_region(
    tx: &impl GenericClient,
    s: &TaxRegionSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO tax_regions (id, version, country, default_rate) VALUES ($1, 1, $2, $3)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.country.as_str(), Type::TEXT),
            (&tax_rate_to_db(s.default_rate)?, Type::INT4),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_class_rates(tx, s.id, &s.class_rates).await?;
    write_stamps(
        tx,
        "tax_regions",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await?;
    Ok(())
}

/// `TaxRegionRepo::update` inside the caller's transaction; the new version. The
/// version check comes first; the rates are only replaced if it passes.
pub(crate) async fn update_tax_region(
    tx: &impl GenericClient,
    s: &TaxRegionSnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE tax_regions SET version = version + 1, country = $3, default_rate = $4
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.country.as_str(), Type::TEXT),
                (&tax_rate_to_db(s.default_rate)?, Type::INT4),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    tx.query_typed(
        "DELETE FROM tax_region_class_rates WHERE tax_region_id = $1",
        &[(&s.id.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    write_class_rates(tx, s.id, &s.class_rates).await?;
    let written = version_from_db(col(&row, "version")?)?;
    write_stamps(
        tx,
        "tax_regions",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await?;
    Ok(written)
}

/// All class rates in one statement; `WITH ORDINALITY` keeps their order.
async fn write_class_rates(
    tx: &impl GenericClient,
    region: TaxRegionId,
    rates: &[ClassRate],
) -> Result<(), RepoError> {
    let classes: Vec<Uuid> = rates.iter().map(|r| r.class.as_uuid()).collect();
    let ppm = rates
        .iter()
        .map(|r| tax_rate_to_db(r.rate))
        .collect::<Result<Vec<i32>, _>>()?;
    tx.query_typed(
        "INSERT INTO tax_region_class_rates (tax_region_id, position, tax_class_id, rate)
         SELECT $1, (n - 1)::integer, c, r
         FROM unnest($2::uuid[], $3::integer[]) WITH ORDINALITY AS t (c, r, n)",
        &[
            (&region.as_uuid(), Type::UUID),
            (&classes, Type::UUID_ARRAY),
            (&ppm, Type::INT4_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// One region found by `filter` (a condition on `$1`), with its class rates, in 2 queries.
async fn load_region<T: tokio_postgres::types::ToSql + Sync>(
    client: &impl GenericClient,
    filter: &'static str,
    value: T,
    value_type: Type,
) -> Result<Option<TaxRegion>, RepoError> {
    let Some(row) = client
        .query_typed_opt(
            &format!(
                "SELECT id, version, country, default_rate, {} FROM tax_regions WHERE {filter}",
                stamp_columns(""),
            ),
            &[(&value, value_type)],
        )
        .await
        .map_err(repo_error)?
    else {
        return Ok(None);
    };
    let id: Uuid = col(&row, "id")?;
    let class_rates = client
        .query_typed(
            "SELECT tax_class_id, rate FROM tax_region_class_rates
             WHERE tax_region_id = $1 ORDER BY position",
            &[(&id, Type::UUID)],
        )
        .await
        .map_err(repo_error)?
        .iter()
        .map(|r| {
            Ok(ClassRate {
                class: TaxClassId::from_uuid(col(r, "tax_class_id")?),
                rate: tax_rate_from_db(col(r, "rate")?)?,
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;
    let country: String = col(&row, "country")?;
    let snapshot = TaxRegionSnapshot {
        id: TaxRegionId::from_uuid(id),
        version: version_from_db(col(&row, "version")?)?,
        stamps: stamps_from_row(&row)?,
        country: CountryCode::new(&country).map_err(corrupt)?,
        default_rate: tax_rate_from_db(col(&row, "default_rate")?)?,
        class_rates,
    };
    TaxRegion::restore(snapshot)
        .map(Some)
        .map_err(|e| corrupt(format!("tax region {id}: {e}")))
}
