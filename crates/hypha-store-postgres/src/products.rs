use std::collections::HashMap;

use hypha_core::{
    Money, OptionId, OptionValueId, Page, PageRequest, ProductId, RepoError, VariantId, Version,
    Versioned,
};
use hypha_products::{
    Handle, OptionSnapshot, OptionValueSnapshot, Product, ProductRepo, ProductSnapshot,
    ProductStatus, VariantSnapshot,
};
use tokio_postgres::{GenericClient, types::Type};
use uuid::Uuid;

use crate::{
    columns::{currency_from_db, version_from_db, version_to_db},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, consistent_read},
};

impl<C: PgConnection> ProductRepo for PgStore<C> {
    async fn get(&self, id: ProductId) -> Result<Option<Product>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            Ok(load(tx, &[id.as_uuid()]).await?.pop())
        })
    }

    async fn get_many(&self, ids: &[ProductId]) -> Result<Vec<Product>, RepoError> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let mut conn = self.conn().await;
        let ids: Vec<Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
        consistent_read!(conn.client(), |tx| load(tx, &ids).await)
    }

    async fn list(
        &self,
        status: Option<ProductStatus>,
        page: PageRequest<ProductId>,
    ) -> Result<Page<Product>, RepoError> {
        let status = status.map(|s| s.as_str());
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            let ids: Vec<Uuid> = tx
                .query_typed(
                    "SELECT id FROM products
                     WHERE ($1::text IS NULL OR status = $1) AND ($2::uuid IS NULL OR id < $2)
                     ORDER BY id DESC LIMIT $3",
                    &[
                        (&status, Type::TEXT),
                        (&after, Type::UUID),
                        (&fetch, Type::INT8),
                    ],
                )
                .await
                .map_err(repo_error)?
                .iter()
                .map(|row| row.get(0))
                .collect();
            let mut products = load(tx, &ids).await?;
            products.sort_by_key(|p| std::cmp::Reverse(p.id().as_uuid()));
            Ok(Page::from_overfetch(products, page.limit))
        })
    }

    async fn get_by_handle(&self, handle: &Handle) -> Result<Option<Product>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            let row = tx
                .query_typed_opt(
                    "SELECT id FROM products WHERE handle = $1",
                    &[(&handle.as_str(), Type::TEXT)],
                )
                .await
                .map_err(repo_error)?;
            match row {
                Some(row) => Ok(load(tx, &[row.get(0)]).await?.pop()),
                None => Ok(None),
            }
        })
    }

    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<Product>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            let row = tx
                .query_typed_opt(
                    "SELECT product_id FROM variants WHERE id = $1",
                    &[(&variant.as_uuid(), Type::UUID)],
                )
                .await
                .map_err(repo_error)?;
            match row {
                Some(row) => Ok(load(tx, &[row.get(0)]).await?.pop()),
                None => Ok(None),
            }
        })
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

/// Writes options, option values and variants in 3 statements however many there are:
/// each column goes in as an array, `unnest` turns them back into rows, and `WITH
/// ORDINALITY` numbers them, which is the position.
async fn write_children(tx: &impl GenericClient, s: &ProductSnapshot) -> Result<(), RepoError> {
    let product_id = s.id.as_uuid();

    let option_ids: Vec<Uuid> = s.options.iter().map(|o| o.id.as_uuid()).collect();
    let option_names: Vec<&str> = s.options.iter().map(|o| o.name.as_str()).collect();
    tx.query_typed(
        "INSERT INTO product_options (id, product_id, position, name)
         SELECT o.id, $1, o.ord - 1, o.name
         FROM unnest($2::uuid[], $3::text[]) WITH ORDINALITY AS o(id, name, ord)",
        &[
            (&product_id, Type::UUID),
            (&option_ids, Type::UUID_ARRAY),
            (&option_names, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;

    // Values are numbered within their option, so their positions are computed here.
    let mut value_ids = Vec::new();
    let mut value_options = Vec::new();
    let mut value_positions = Vec::new();
    let mut value_labels = Vec::new();
    for option in &s.options {
        for (position, value) in (0i32..).zip(&option.values) {
            value_ids.push(value.id.as_uuid());
            value_options.push(option.id.as_uuid());
            value_positions.push(position);
            value_labels.push(value.label.as_str());
        }
    }
    tx.query_typed(
        "INSERT INTO product_option_values (id, option_id, position, label)
         SELECT * FROM unnest($1::uuid[], $2::uuid[], $3::int4[], $4::text[])",
        &[
            (&value_ids, Type::UUID_ARRAY),
            (&value_options, Type::UUID_ARRAY),
            (&value_positions, Type::INT4_ARRAY),
            (&value_labels, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;

    // Every variant selects one value per option, so all selections fit one flat array
    // that each row slices: variant n (from 1) takes elements (n-1)*k+1 ..= n*k. With no
    // options k is 0 and every slice is empty.
    let per_variant = i64::try_from(s.options.len()).map_err(corrupt)?;
    let variant_ids: Vec<Uuid> = s.variants.iter().map(|v| v.id.as_uuid()).collect();
    let skus: Vec<Option<&str>> = s.variants.iter().map(|v| v.sku.as_deref()).collect();
    let amounts: Vec<i64> = s.variants.iter().map(|v| v.base_price.amount()).collect();
    let currencies: Vec<&str> = s
        .variants
        .iter()
        .map(|v| v.base_price.currency().as_str())
        .collect();
    let selections: Vec<Uuid> = s
        .variants
        .iter()
        .flat_map(|v| v.selections.iter().map(|id| id.as_uuid()))
        .collect();
    tx.query_typed(
        "INSERT INTO variants
            (id, product_id, position, sku, price_amount, price_currency, selections)
         SELECT v.id, $1, v.ord - 1, v.sku, v.amount, v.currency,
                ($7::uuid[])[(v.ord - 1) * $6 + 1 : v.ord * $6]
         FROM unnest($2::uuid[], $3::text[], $4::int8[], $5::text[])
              WITH ORDINALITY AS v(id, sku, amount, currency, ord)",
        &[
            (&product_id, Type::UUID),
            (&variant_ids, Type::UUID_ARRAY),
            (&skus, Type::TEXT_ARRAY),
            (&amounts, Type::INT8_ARRAY),
            (&currencies, Type::TEXT_ARRAY),
            (&per_variant, Type::INT8),
            (&selections, Type::UUID_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// Loads the products with these IDs (missing ones are skipped), in 4 queries.
async fn load(client: &impl GenericClient, ids: &[Uuid]) -> Result<Vec<Product>, RepoError> {
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
