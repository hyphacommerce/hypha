//! Customer groups and their membership (design note 0032, §6).

use std::collections::HashMap;

use hypha_core::{
    CustomerGroupId, CustomerId, Page, PageRequest, Recorded, RepoError, Version, Versioned,
    stamp_insert, stamp_update,
};
use hypha_customers::{CustomerGroup, CustomerGroupRepo, CustomerGroupSnapshot};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{col, stamp_columns, stamps_from_row, version_from_db, version_to_db, write_stamps},
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, Queries, in_transaction},
};

fn columns() -> String {
    format!(
        "id, version, name, handle, description, {}",
        stamp_columns("")
    )
}

fn group_from_row(row: &Row) -> Result<CustomerGroup, RepoError> {
    let id: Uuid = col(row, "id")?;
    let snapshot = CustomerGroupSnapshot {
        id: CustomerGroupId::from_uuid(id),
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        name: col(row, "name")?,
        handle: col(row, "handle")?,
        description: col(row, "description")?,
    };
    CustomerGroup::restore(snapshot).map_err(|e| corrupt(format!("customer group {id}: {e}")))
}

fn uuids(customers: &[CustomerId]) -> Vec<Uuid> {
    customers.iter().map(|c| c.as_uuid()).collect()
}

pub(crate) async fn insert_group(
    tx: &impl Queries,
    s: &CustomerGroupSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO customer_groups (id, version, name, handle, description)
         VALUES ($1, 1, $2, $3, $4)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.name, Type::TEXT),
            (&s.handle, Type::TEXT),
            (&s.description, Type::TEXT),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_stamps(
        tx,
        "customer_groups",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await
}

/// `CustomerGroupRepo::update` inside the caller's transaction; the new version.
pub(crate) async fn update_group(
    tx: &impl Queries,
    s: &CustomerGroupSnapshot,
) -> Result<Version, RepoError> {
    let row = tx
        .query_typed_opt(
            "UPDATE customer_groups SET version = version + 1, name = $3, handle = $4,
                 description = $5
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&s.handle, Type::TEXT),
                (&s.description, Type::TEXT),
            ],
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    let version = version_from_db(col(&row, "version")?)?;
    write_stamps(
        tx,
        "customer_groups",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await?;
    Ok(version)
}

/// Deletes a group (its membership cascades).
pub(crate) async fn delete_group(
    tx: &impl Queries,
    group: CustomerGroupId,
) -> Result<(), RepoError> {
    tx.query_typed(
        "DELETE FROM customer_groups WHERE id = $1",
        &[(&group.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// Adds membership; a missing group or customer fails on its foreign key.
pub(crate) async fn add_group_members(
    tx: &impl Queries,
    group: CustomerGroupId,
    customers: &[CustomerId],
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO customer_group_members (group_id, customer_id)
         SELECT $1, customer_id FROM unnest($2::uuid[]) AS t (customer_id)
         ON CONFLICT DO NOTHING",
        &[
            (&group.as_uuid(), Type::UUID),
            (&uuids(customers), Type::UUID_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

pub(crate) async fn remove_group_members(
    tx: &impl Queries,
    group: CustomerGroupId,
    customers: &[CustomerId],
) -> Result<(), RepoError> {
    tx.query_typed(
        "DELETE FROM customer_group_members WHERE group_id = $1 AND customer_id = ANY($2)",
        &[
            (&group.as_uuid(), Type::UUID),
            (&uuids(customers), Type::UUID_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

impl<C: PgConnection> CustomerGroupRepo for PgStore<C> {
    async fn get(&self, id: CustomerGroupId) -> Result<Option<CustomerGroup>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!("SELECT {} FROM customer_groups WHERE id = $1", columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .map(|row| group_from_row(&row))
            .transpose()
    }

    async fn get_by_handle(&self, handle: &str) -> Result<Option<CustomerGroup>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM customer_groups WHERE handle = $1",
                    columns()
                ),
                &[(&handle, Type::TEXT)],
            )
            .await
            .map_err(repo_error)?
            .map(|row| group_from_row(&row))
            .transpose()
    }

    async fn list_all(&self) -> Result<Vec<CustomerGroup>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM customer_groups ORDER BY lower(name), id",
                    columns()
                ),
                &[],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(group_from_row)
            .collect()
    }

    async fn insert(&self, group: &mut CustomerGroup) -> Result<(), RepoError> {
        stamp_insert(group, self.now());
        let s = group.snapshot();
        let events = group.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_group(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        group.set_version(version);
        Ok(())
    }

    async fn update(&self, group: &mut CustomerGroup) -> Result<(), RepoError> {
        stamp_update(group, self.now());
        let s = group.snapshot();
        let events = group.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_group(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        group.set_version(version);
        Ok(())
    }

    async fn add_members(
        &self,
        group: CustomerGroupId,
        customers: &[CustomerId],
    ) -> Result<(), RepoError> {
        in_transaction!(self, |tx| add_group_members(tx, group, customers).await)
    }

    async fn member_page(
        &self,
        group: CustomerGroupId,
        page: PageRequest<CustomerId>,
    ) -> Result<Page<CustomerId>, RepoError> {
        let fetch = i64::from(page.limit.get()) + 1;
        let after = page.after.map(|c| c.as_uuid());
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT customer_id FROM customer_group_members
                 WHERE group_id = $1 AND ($2::uuid IS NULL OR customer_id < $2)
                 ORDER BY customer_id DESC LIMIT $3",
                &[
                    (&group.as_uuid(), Type::UUID),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        let items = rows
            .iter()
            .map(|row| Ok(CustomerId::from_uuid(col(row, "customer_id")?)))
            .collect::<Result<Vec<_>, RepoError>>()?;
        Ok(Page::from_overfetch(items, page.limit))
    }

    async fn members_in(&self, group: CustomerGroupId) -> Result<Vec<CustomerId>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "SELECT customer_id FROM customer_group_members
                 WHERE group_id = $1 ORDER BY customer_id",
                &[(&group.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|row| Ok(CustomerId::from_uuid(col(row, "customer_id")?)))
            .collect()
    }

    async fn groups_of(
        &self,
        customers: &[CustomerId],
    ) -> Result<HashMap<CustomerId, Vec<CustomerGroupId>>, RepoError> {
        if customers.is_empty() {
            return Ok(HashMap::new());
        }
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT customer_id, group_id FROM customer_group_members
                 WHERE customer_id = ANY($1) ORDER BY customer_id, group_id",
                &[(&uuids(customers), Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        let mut found: HashMap<CustomerId, Vec<CustomerGroupId>> = HashMap::new();
        for row in &rows {
            found
                .entry(CustomerId::from_uuid(col(row, "customer_id")?))
                .or_default()
                .push(CustomerGroupId::from_uuid(col(row, "group_id")?));
        }
        Ok(found)
    }
}
