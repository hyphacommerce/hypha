use std::collections::HashMap;

use hypha_core::{
    AssetId, Money, OptionId, OptionValueId, Page, PageRequest, Price, ProductId, Recorded,
    RepoError, VariantId, Version, Versioned, Weight, stamp_insert, stamp_update,
};
use hypha_products::{
    Handle, OptionSnapshot, OptionValueSnapshot, PriceRange, Product, ProductRepo, ProductSnapshot,
    ProductStatus, ProductSummary, VariantSnapshot,
};
use tokio_postgres::{GenericClient, Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        col, currency_from_db, stamp_columns, stamps_from_row, version_from_db, version_to_db,
        write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, consistent_read, in_transaction},
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

    /// One query whatever the page holds: each product's variant count and price ranges
    /// are aggregated in SQL (per product, only for the rows the limit keeps), so no
    /// variant rows leave the database.
    async fn list(
        &self,
        status: Option<ProductStatus>,
        page: PageRequest<ProductId>,
    ) -> Result<Page<ProductSummary>, RepoError> {
        let status = status.map(|s| s.as_str());
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let rows = conn
            .client()
            .query_typed(
                &format!(
                    "SELECT p.id, p.handle, p.title, p.status, p.vendor, p.product_type, c.variants, {},
                        r.currencies, r.mins, r.maxes,
                        (SELECT asset_id FROM product_media
                         WHERE product_id = p.id AND position = 0) AS featured
                 FROM products p
                 CROSS JOIN LATERAL (
                     SELECT count(*) AS variants FROM variants WHERE product_id = p.id
                 ) c
                 CROSS JOIN LATERAL (
                     SELECT array_agg(currency ORDER BY currency) AS currencies,
                            array_agg(low ORDER BY currency) AS mins,
                            array_agg(high ORDER BY currency) AS maxes
                     FROM (
                         SELECT price_currency AS currency, min(price_amount) AS low,
                                max(price_amount) AS high
                         FROM variants WHERE product_id = p.id GROUP BY price_currency
                     ) by_currency
                 ) r
                 WHERE ($1::text IS NULL OR p.status = $1) AND ($2::uuid IS NULL OR p.id < $2)
                 ORDER BY p.id DESC LIMIT $3",
                    stamp_columns("p."),
                ),
                &[
                    (&status, Type::TEXT),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        let summaries = rows
            .iter()
            .map(summary_from_db)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(summaries, page.limit))
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
                Some(row) => Ok(load(tx, &[col(&row, "id")?]).await?.pop()),
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
                Some(row) => Ok(load(tx, &[col(&row, "product_id")?]).await?.pop()),
                None => Ok(None),
            }
        })
    }

    async fn get_by_sku(&self, sku: &str) -> Result<Option<Product>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            let row = tx
                .query_typed_opt(
                    "SELECT product_id FROM variants WHERE sku = $1",
                    &[(&sku, Type::TEXT)],
                )
                .await
                .map_err(repo_error)?;
            match row {
                Some(row) => Ok(load(tx, &[col(&row, "product_id")?]).await?.pop()),
                None => Ok(None),
            }
        })
    }

    async fn insert(&self, product: &mut Product) -> Result<(), RepoError> {
        stamp_insert(product, self.now());
        let s = product.snapshot();
        let events = product.take_events();
        let actor = self.actor();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_product(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        product.set_version(version);
        Ok(())
    }

    async fn update(&self, product: &mut Product) -> Result<(), RepoError> {
        stamp_update(product, self.now());
        let s = product.snapshot();
        let events = product.take_events();
        let actor = self.actor();
        let version = in_transaction!(self, |tx| {
            let version = update_product(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await?;
            Ok(version)
        })?;
        product.set_version(version);
        Ok(())
    }
}

/// `ProductRepo::insert` inside the caller's transaction: the row, then its children.
pub(crate) async fn insert_product(
    tx: &impl GenericClient,
    s: &ProductSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO products
             (id, version, handle, title, status, description, vendor, product_type, tags)
         VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.handle, Type::TEXT),
            (&s.title, Type::TEXT),
            (&s.status.as_str(), Type::TEXT),
            (&s.description, Type::TEXT),
            (&s.vendor, Type::TEXT),
            (&s.product_type, Type::TEXT),
            (&s.tags, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_children(tx, s).await?;
    write_stamps(tx, "products", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(())
}

/// `ProductRepo::update` inside a transaction (also a batch's): returns the new version.
pub(crate) async fn update_product(
    tx: &impl GenericClient,
    s: &ProductSnapshot,
) -> Result<Version, RepoError> {
    let expected = version_to_db(s.version)?;
    // The version check comes first; nothing else is touched if it fails.
    let bumped = tx
        .query_typed_opt(
            "UPDATE products SET version = version + 1, handle = $3, title = $4, status = $5,
                    description = $6, vendor = $7, product_type = $8, tags = $9
                 WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&expected, Type::INT8),
                (&s.handle, Type::TEXT),
                (&s.title, Type::TEXT),
                (&s.status.as_str(), Type::TEXT),
                (&s.description, Type::TEXT),
                (&s.vendor, Type::TEXT),
                (&s.product_type, Type::TEXT),
                (&s.tags, Type::TEXT_ARRAY),
            ],
        )
        .await
        .map_err(repo_error)?;
    let Some(row) = bumped else {
        return Err(RepoError::StaleVersion);
    };
    let new_version = version_from_db(col(&row, "version")?)?;

    // Children are replaced wholesale; option values go with their options (CASCADE).
    for sql in [
        "DELETE FROM product_options WHERE product_id = $1",
        "DELETE FROM variants WHERE product_id = $1",
        "DELETE FROM product_media WHERE product_id = $1",
    ] {
        tx.query_typed(sql, &[(&s.id.as_uuid(), Type::UUID)])
            .await
            .map_err(repo_error)?;
    }
    write_children(tx, s).await?;
    let written = new_version;
    write_stamps(tx, "products", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(written)
}

/// Writes options, option values, variants and media in 4 statements however many there are:
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
    let media: Vec<Option<Uuid>> = s
        .variants
        .iter()
        .map(|v| v.media.map(|id| id.as_uuid()))
        .collect();
    let weights: Vec<Option<i64>> = s
        .variants
        .iter()
        .map(|v| v.weight.map(|w| i64::from(w.grams())))
        .collect();
    let requires_shipping: Vec<bool> = s.variants.iter().map(|v| v.requires_shipping).collect();
    tx.query_typed(
        "INSERT INTO variants
            (id, product_id, position, sku, price_amount, price_currency, selections,
             media_asset_id, weight_grams, requires_shipping)
         SELECT v.id, $1, v.ord - 1, v.sku, v.amount, v.currency,
                ($7::uuid[])[(v.ord - 1) * $6 + 1 : v.ord * $6], v.media, v.weight,
                v.requires_shipping
         FROM unnest($2::uuid[], $3::text[], $4::int8[], $5::text[], $8::uuid[], $9::int8[],
                     $10::bool[])
              WITH ORDINALITY AS v(id, sku, amount, currency, media, weight,
                                   requires_shipping, ord)",
        &[
            (&product_id, Type::UUID),
            (&variant_ids, Type::UUID_ARRAY),
            (&skus, Type::TEXT_ARRAY),
            (&amounts, Type::INT8_ARRAY),
            (&currencies, Type::TEXT_ARRAY),
            (&per_variant, Type::INT8),
            (&selections, Type::UUID_ARRAY),
            (&media, Type::UUID_ARRAY),
            (&weights, Type::INT8_ARRAY),
            (&requires_shipping, Type::BOOL_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;

    let assets: Vec<Uuid> = s.media.iter().map(|id| id.as_uuid()).collect();
    tx.query_typed(
        "INSERT INTO product_media (product_id, position, asset_id)
         SELECT $1, m.ord - 1, m.asset FROM unnest($2::uuid[]) WITH ORDINALITY AS m(asset, ord)",
        &[(&product_id, Type::UUID), (&assets, Type::UUID_ARRAY)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// Loads the products with these IDs (missing ones are skipped), in 5 queries.
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
            &format!(
                "SELECT id, version, handle, title, status, description, vendor, product_type,
                        tags, {}
                 FROM products WHERE id = ANY($1)",
                stamp_columns(""),
            ),
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
            .entry(col(&row, "option_id")?)
            .or_default()
            .push(OptionValueSnapshot {
                id: OptionValueId::from_uuid(col(&row, "id")?),
                label: col(&row, "label")?,
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
        let option_id: Uuid = col(&row, "id")?;
        options
            .entry(col(&row, "product_id")?)
            .or_default()
            .push(OptionSnapshot {
                id: OptionId::from_uuid(option_id),
                name: col(&row, "name")?,
                values: values.remove(&option_id).unwrap_or_default(),
            });
    }

    let mut variants: HashMap<Uuid, Vec<VariantSnapshot>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT product_id, id, sku, price_amount, price_currency, selections, media_asset_id,
                    weight_grams, requires_shipping
             FROM variants WHERE product_id = ANY($1) ORDER BY product_id, position",
            &param,
        )
        .await
        .map_err(repo_error)?
    {
        let currency = currency_from_db(col(&row, "price_currency")?)?;
        let base_price = Money::new(col(&row, "price_amount")?, currency).map_err(corrupt)?;
        let selections: Vec<Uuid> = col(&row, "selections")?;
        variants
            .entry(col(&row, "product_id")?)
            .or_default()
            .push(VariantSnapshot {
                id: VariantId::from_uuid(col(&row, "id")?),
                sku: col(&row, "sku")?,
                base_price,
                selections: selections
                    .into_iter()
                    .map(OptionValueId::from_uuid)
                    .collect(),
                media: col::<Option<Uuid>>(&row, "media_asset_id")?.map(AssetId::from_uuid),
                weight: col::<Option<i64>>(&row, "weight_grams")?
                    .map(|g| u32::try_from(g).map(Weight::from_grams))
                    .transpose()
                    .map_err(corrupt)?,
                requires_shipping: col(&row, "requires_shipping")?,
            });
    }

    let mut media: HashMap<Uuid, Vec<AssetId>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT product_id, asset_id FROM product_media
             WHERE product_id = ANY($1) ORDER BY product_id, position",
            &param,
        )
        .await
        .map_err(repo_error)?
    {
        media
            .entry(col(&row, "product_id")?)
            .or_default()
            .push(AssetId::from_uuid(col(&row, "asset_id")?));
    }

    products
        .iter()
        .map(|row| {
            let id: Uuid = col(row, "id")?;
            let status: String = col(row, "status")?;
            let snapshot = ProductSnapshot {
                id: ProductId::from_uuid(id),
                version: version_from_db(col(row, "version")?)?,
                stamps: stamps_from_row(row)?,
                handle: col(row, "handle")?,
                title: col(row, "title")?,
                status: status
                    .parse()
                    .map_err(|s| corrupt(format!("product status {s:?}")))?,
                description: col(row, "description")?,
                vendor: col(row, "vendor")?,
                product_type: col(row, "product_type")?,
                tags: col(row, "tags")?,
                options: options.remove(&id).unwrap_or_default(),
                variants: variants.remove(&id).unwrap_or_default(),
                media: media.remove(&id).unwrap_or_default(),
            };
            Product::restore(snapshot).map_err(|e| corrupt(format!("product {id}: {e}")))
        })
        .collect()
}

/// A product summary from `list`'s row: the product's columns, its variant count and
/// three arrays (currency codes, lowest and highest amounts), aligned and ordered by
/// currency code.
fn summary_from_db(row: &Row) -> Result<ProductSummary, RepoError> {
    let id: Uuid = col(row, "id")?;
    let handle: String = col(row, "handle")?;
    let status: String = col(row, "status")?;
    let variants: i64 = col(row, "variants")?;
    // `array_agg` over no rows is NULL; every product has a variant, but no need to rely
    // on that here.
    let currencies: Vec<String> = col::<Option<_>>(row, "currencies")?.unwrap_or_default();
    let mins: Vec<i64> = col::<Option<_>>(row, "mins")?.unwrap_or_default();
    let maxes: Vec<i64> = col::<Option<_>>(row, "maxes")?.unwrap_or_default();
    let price = |amount: i64, currency| {
        Money::new(amount, currency)
            .and_then(Price::new)
            .map_err(|e| corrupt(format!("product {id} price: {e}")))
    };
    let price_ranges = currencies
        .iter()
        .zip(mins.iter().zip(&maxes))
        .map(|(code, (&min, &max))| {
            let currency = currency_from_db(code)?;
            Ok(PriceRange {
                min: price(min, currency)?,
                max: price(max, currency)?,
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;
    Ok(ProductSummary {
        id: ProductId::from_uuid(id),
        handle: Handle::new(&handle).map_err(|e| corrupt(format!("product {id}: {e}")))?,
        title: col(row, "title")?,
        status: status
            .parse()
            .map_err(|s| corrupt(format!("product status {s:?}")))?,
        vendor: col(row, "vendor")?,
        product_type: col(row, "product_type")?,
        variant_count: usize::try_from(variants).map_err(corrupt)?,
        price_ranges,
        featured_media: col::<Option<Uuid>>(row, "featured")?.map(AssetId::from_uuid),
        stamps: stamps_from_row(row)?,
    })
}
