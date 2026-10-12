use std::collections::HashMap;

use hypha_core::{
    Money, OptionId, OptionValueId, ProductId, RepoError, VariantId, Version, Versioned,
};
use hypha_products::{
    Handle, OptionSnapshot, OptionValueSnapshot, Product, ProductRepo, ProductSnapshot,
    VariantSnapshot,
};
use tokio_postgres::{Client, GenericClient, types::Type};
use uuid::Uuid;

use crate::{
    columns::{currency_from_db, version_from_db, version_to_db},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore},
};

impl<C: PgConnection> ProductRepo for PgStore<C> {
    async fn get(&self, id: ProductId) -> Result<Option<Product>, RepoError> {
        let mut conn = self.conn().await;
        Ok(load(conn.client(), &[id.as_uuid()]).await?.pop())
    }

    async fn get_many(&self, ids: &[ProductId]) -> Result<Vec<Product>, RepoError> {
        let mut conn = self.conn().await;
        let ids: Vec<Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
        load(conn.client(), &ids).await
    }

    async fn get_by_handle(&self, handle: &Handle) -> Result<Option<Product>, RepoError> {
        let mut conn = self.conn().await;
        let client = conn.client();
        let row = client
            .query_typed_opt(
                "SELECT id FROM products WHERE handle = $1",
                &[(&handle.as_str(), Type::TEXT)],
            )
            .await
            .map_err(repo_error)?;
        match row {
            Some(row) => Ok(load(client, &[row.get(0)]).await?.pop()),
            None => Ok(None),
        }
    }

    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<Product>, RepoError> {
        let mut conn = self.conn().await;
        let client = conn.client();
        let row = client
            .query_typed_opt(
                "SELECT product_id FROM variants WHERE id = $1",
                &[(&variant.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        match row {
            Some(row) => Ok(load(client, &[row.get(0)]).await?.pop()),
            None => Ok(None),
        }
    }

    async fn insert(&self, product: &mut Product) -> Result<(), RepoError> {
        let s = product.snapshot();
        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        tx.query_typed(
            "INSERT INTO products (id, version, handle, title, status) VALUES ($1, 1, $2, $3, $4)",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&s.handle, Type::TEXT),
                (&s.title, Type::TEXT),
                (&s.status.as_str(), Type::TEXT),
            ],
        )
        .await
        .map_err(repo_error)?;
        write_children(&tx, &s).await?;
        tx.commit().await.map_err(repo_error)?;
        product.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, product: &mut Product) -> Result<(), RepoError> {
        let s = product.snapshot();
        let expected = version_to_db(s.version)?;
        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;

        // The version check comes first; nothing else is touched if it fails.
        let bumped = tx
            .query_typed_opt(
                "UPDATE products SET version = version + 1, handle = $3, title = $4, status = $5
                 WHERE id = $1 AND version = $2 RETURNING version",
                &[
                    (&s.id.as_uuid(), Type::UUID),
                    (&expected, Type::INT8),
                    (&s.handle, Type::TEXT),
                    (&s.title, Type::TEXT),
                    (&s.status.as_str(), Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = bumped else {
            tx.rollback().await.map_err(repo_error)?;
            return Err(RepoError::StaleVersion);
        };
        let new_version = version_from_db(row.get(0))?;

        // Children are replaced wholesale; option values go with their options (CASCADE).
        for sql in [
            "DELETE FROM product_options WHERE product_id = $1",
            "DELETE FROM variants WHERE product_id = $1",
        ] {
            tx.query_typed(sql, &[(&s.id.as_uuid(), Type::UUID)])
                .await
                .map_err(repo_error)?;
        }
        write_children(&tx, &s).await?;
        tx.commit().await.map_err(repo_error)?;
        product.set_version(new_version);
        Ok(())
    }
}

async fn write_children(tx: &impl GenericClient, s: &ProductSnapshot) -> Result<(), RepoError> {
    let product_id = s.id.as_uuid();
    for (position, option) in (0i32..).zip(&s.options) {
        tx.query_typed(
            "INSERT INTO product_options (id, product_id, position, name) VALUES ($1, $2, $3, $4)",
            &[
                (&option.id.as_uuid(), Type::UUID),
                (&product_id, Type::UUID),
                (&position, Type::INT4),
                (&option.name, Type::TEXT),
            ],
        )
        .await
        .map_err(repo_error)?;
        for (position, value) in (0i32..).zip(&option.values) {
            tx.query_typed(
                "INSERT INTO product_option_values (id, option_id, position, label)
                 VALUES ($1, $2, $3, $4)",
                &[
                    (&value.id.as_uuid(), Type::UUID),
                    (&option.id.as_uuid(), Type::UUID),
                    (&position, Type::INT4),
                    (&value.label, Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        }
    }
    for (position, variant) in (0i32..).zip(&s.variants) {
        let selections: Vec<Uuid> = variant.selections.iter().map(|v| v.as_uuid()).collect();
        tx.query_typed(
            "INSERT INTO variants
                (id, product_id, position, sku, price_amount, price_currency, selections)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[
                (&variant.id.as_uuid(), Type::UUID),
                (&product_id, Type::UUID),
                (&position, Type::INT4),
                (&variant.sku, Type::TEXT),
                (&variant.base_price.amount(), Type::INT8),
                (&variant.base_price.currency().as_str(), Type::TEXT),
                (&selections, Type::UUID_ARRAY),
            ],
        )
        .await
        .map_err(repo_error)?;
    }
    Ok(())
}

/// Loads the products with these IDs (missing ones are skipped), in 4 queries.
async fn load(client: &Client, ids: &[Uuid]) -> Result<Vec<Product>, RepoError> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let ids = ids.to_vec();
    let param = [(
        &ids as &(dyn tokio_postgres::types::ToSql + Sync),
        Type::UUID_ARRAY,
    )];

    let products = client
        .query_typed(
            "SELECT id, version, handle, title, status FROM products WHERE id = ANY($1)",
            &param,
        )
        .await
        .map_err(repo_error)?;
    if products.is_empty() {
        return Ok(vec![]);
    }

    let mut values: HashMap<Uuid, Vec<OptionValueSnapshot>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT v.option_id, v.id, v.label FROM product_option_values v
             JOIN product_options o ON o.id = v.option_id
             WHERE o.product_id = ANY($1) ORDER BY v.option_id, v.position",
            &param,
        )
        .await
        .map_err(repo_error)?
    {
        values
            .entry(row.get(0))
            .or_default()
            .push(OptionValueSnapshot {
                id: OptionValueId::from_uuid(row.get(1)),
                label: row.get(2),
            });
    }

    let mut options: HashMap<Uuid, Vec<OptionSnapshot>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT product_id, id, name FROM product_options
             WHERE product_id = ANY($1) ORDER BY product_id, position",
            &param,
        )
        .await
        .map_err(repo_error)?
    {
        let option_id: Uuid = row.get(1);
        options.entry(row.get(0)).or_default().push(OptionSnapshot {
            id: OptionId::from_uuid(option_id),
            name: row.get(2),
            values: values.remove(&option_id).unwrap_or_default(),
        });
    }

    let mut variants: HashMap<Uuid, Vec<VariantSnapshot>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT product_id, id, sku, price_amount, price_currency, selections FROM variants
             WHERE product_id = ANY($1) ORDER BY product_id, position",
            &param,
        )
        .await
        .map_err(repo_error)?
    {
        let currency = currency_from_db(row.get(4))?;
        let base_price = Money::new(row.get(3), currency).map_err(corrupt)?;
        let selections: Vec<Uuid> = row.get(5);
        variants
            .entry(row.get(0))
            .or_default()
            .push(VariantSnapshot {
                id: VariantId::from_uuid(row.get(1)),
                sku: row.get(2),
                base_price,
                selections: selections
                    .into_iter()
                    .map(OptionValueId::from_uuid)
                    .collect(),
            });
    }

    products
        .iter()
        .map(|row| {
            let id: Uuid = row.get(0);
            let status: String = row.get(4);
            let snapshot = ProductSnapshot {
                id: ProductId::from_uuid(id),
                version: version_from_db(row.get(1))?,
                handle: row.get(2),
                title: row.get(3),
                status: status
                    .parse()
                    .map_err(|s| corrupt(format!("product status {s:?}")))?,
                options: options.remove(&id).unwrap_or_default(),
                variants: variants.remove(&id).unwrap_or_default(),
            };
            Product::restore(snapshot).map_err(|e| corrupt(format!("product {id}: {e}")))
        })
        .collect()
}
