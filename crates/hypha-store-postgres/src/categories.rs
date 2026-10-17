//! Categories and their membership (design note 0022).

use std::collections::HashMap;

use hypha_categories::{Category, CategoryRepo, CategorySnapshot};
use hypha_core::{
    AssetId, CategoryId, Page, PageRequest, ProductId, Recorded, RepoError, Version, Versioned,
    stamp_insert, stamp_update,
};
use tokio_postgres::{GenericClient, Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{col, stamp_columns, stamps_from_row, version_from_db, version_to_db, write_stamps},
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, in_transaction},
};

fn columns() -> String {
    format!(
        "id, version, name, handle, description, image_id, parent_id, position, visible, {}",
        stamp_columns("")
    )
}

fn category_from_row(row: &Row) -> Result<Category, RepoError> {
    let id = CategoryId::from_uuid(col(row, "id")?);
    let position: i32 = col(row, "position")?;
    Category::restore(CategorySnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        name: col(row, "name")?,
        handle: col(row, "handle")?,
        description: col(row, "description")?,
        image: col::<Option<Uuid>>(row, "image_id")?.map(AssetId::from_uuid),
        parent: col::<Option<Uuid>>(row, "parent_id")?.map(CategoryId::from_uuid),
        position: u32::try_from(position)
            .map_err(|_| corrupt(format!("category {id} position {position}")))?,
        visible: col(row, "visible")?,
    })
    .map_err(|e| corrupt(format!("category {id}: {e}")))
}

fn uuids(products: &[ProductId]) -> Vec<Uuid> {
    products.iter().map(|p| p.as_uuid()).collect()
}

fn position_to_db(s: &CategorySnapshot) -> Result<i32, RepoError> {
    i32::try_from(s.position).map_err(|_| corrupt(format!("category {} position", s.id)))
}

/// `CategoryRepo::insert` inside the caller's transaction (also a batch's).
pub(crate) async fn insert_category(
    tx: &impl GenericClient,
    s: &CategorySnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO categories (id, version, name, handle, description, image_id, parent_id,
             position, visible)
         VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.name, Type::TEXT),
            (&s.handle, Type::TEXT),
            (&s.description, Type::TEXT),
            (&s.image.map(|a| a.as_uuid()), Type::UUID),
            (&s.parent.map(|p| p.as_uuid()), Type::UUID),
            (&position_to_db(s)?, Type::INT4),
            (&s.visible, Type::BOOL),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_stamps(
        tx,
        "categories",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await
}

/// `CategoryRepo::update` inside the caller's transaction; the new version.
pub(crate) async fn update_category(
    tx: &impl GenericClient,
    s: &CategorySnapshot,
) -> Result<Version, RepoError> {
    let row = tx
        .query_typed_opt(
            "UPDATE categories SET version = version + 1, name = $3, handle = $4,
                 description = $5, image_id = $6, parent_id = $7, position = $8, visible = $9
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&s.handle, Type::TEXT),
                (&s.description, Type::TEXT),
                (&s.image.map(|a| a.as_uuid()), Type::UUID),
                (&s.parent.map(|p| p.as_uuid()), Type::UUID),
                (&position_to_db(s)?, Type::INT4),
                (&s.visible, Type::BOOL),
            ],
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    let version = version_from_db(col(&row, "version")?)?;
    write_stamps(
        tx,
        "categories",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await?;
    Ok(version)
}

/// Deletes a category (its membership cascades). A child left behind fails the parent's
/// foreign key when the transaction commits.
pub(crate) async fn delete_category(
    tx: &impl GenericClient,
    category: CategoryId,
) -> Result<(), RepoError> {
    tx.query_typed(
        "DELETE FROM categories WHERE id = $1",
        &[(&category.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// Adds membership; a missing category fails on its foreign key.
pub(crate) async fn add_category_products(
    tx: &impl GenericClient,
    category: CategoryId,
    products: &[ProductId],
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO category_products (category_id, product_id)
         SELECT $1, product_id FROM unnest($2::uuid[]) AS t (product_id)
         ON CONFLICT DO NOTHING",
        &[
            (&category.as_uuid(), Type::UUID),
            (&uuids(products), Type::UUID_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

pub(crate) async fn remove_category_products(
    tx: &impl GenericClient,
    category: CategoryId,
    products: &[ProductId],
) -> Result<(), RepoError> {
    tx.query_typed(
        "DELETE FROM category_products WHERE category_id = $1 AND product_id = ANY($2)",
        &[
            (&category.as_uuid(), Type::UUID),
            (&uuids(products), Type::UUID_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

impl<C: PgConnection> CategoryRepo for PgStore<C> {
    async fn get(&self, id: CategoryId) -> Result<Option<Category>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_opt(
                &format!("SELECT {} FROM categories WHERE id = $1", columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(category_from_row).transpose()
    }

    async fn get_by_handle(&self, handle: &str) -> Result<Option<Category>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_opt(
                &format!("SELECT {} FROM categories WHERE handle = $1", columns()),
                &[(&handle, Type::TEXT)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(category_from_row).transpose()
    }

    async fn list_all(&self) -> Result<Vec<Category>, RepoError> {
        let mut conn = self.conn().await;
        conn.client()
            .query_typed(&format!("SELECT {} FROM categories", columns()), &[])
            .await
            .map_err(repo_error)?
            .iter()
            .map(category_from_row)
            .collect()
    }

    async fn insert(&self, category: &mut Category) -> Result<(), RepoError> {
        stamp_insert(category, self.now());
        let s = category.snapshot();
        let events = category.take_events();
        let actor = self.actor();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_category(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        category.set_version(version);
        Ok(())
    }

    async fn update(&self, category: &mut Category) -> Result<(), RepoError> {
        stamp_update(category, self.now());
        let s = category.snapshot();
        let events = category.take_events();
        let actor = self.actor();
        let version = in_transaction!(self, |tx| {
            let version = update_category(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await?;
            Ok(version)
        })?;
        category.set_version(version);
        Ok(())
    }

    async fn add_products(
        &self,
        category: CategoryId,
        products: &[ProductId],
    ) -> Result<(), RepoError> {
        let mut conn = self.conn().await;
        add_category_products(conn.client(), category, products).await
    }

    async fn product_page(
        &self,
        category: CategoryId,
        page: PageRequest<ProductId>,
    ) -> Result<Page<ProductId>, RepoError> {
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let ids = conn
            .client()
            .query_typed(
                "SELECT product_id FROM category_products
                 WHERE category_id = $1 AND ($2::uuid IS NULL OR product_id < $2)
                 ORDER BY product_id DESC LIMIT $3",
                &[
                    (&category.as_uuid(), Type::UUID),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|row| Ok(ProductId::from_uuid(col(row, "product_id")?)))
            .collect::<Result<Vec<_>, RepoError>>()?;
        Ok(Page::from_overfetch(ids, page.limit))
    }

    async fn products_in(&self, categories: &[CategoryId]) -> Result<Vec<ProductId>, RepoError> {
        if categories.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = categories.iter().map(|c| c.as_uuid()).collect();
        let mut conn = self.conn().await;
        conn.client()
            .query_typed(
                "SELECT DISTINCT product_id FROM category_products WHERE category_id = ANY($1)
                 ORDER BY product_id",
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|row| Ok(ProductId::from_uuid(col(row, "product_id")?)))
            .collect()
    }

    async fn categories_of(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, Vec<CategoryId>>, RepoError> {
        if products.is_empty() {
            return Ok(HashMap::new());
        }
        let mut conn = self.conn().await;
        let rows = conn
            .client()
            .query_typed(
                "SELECT product_id, category_id FROM category_products
                 WHERE product_id = ANY($1) ORDER BY product_id, category_id",
                &[(&uuids(products), Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        let mut found: HashMap<ProductId, Vec<CategoryId>> = HashMap::new();
        for row in &rows {
            found
                .entry(ProductId::from_uuid(col(row, "product_id")?))
                .or_default()
                .push(CategoryId::from_uuid(col(row, "category_id")?));
        }
        Ok(found)
    }
}
