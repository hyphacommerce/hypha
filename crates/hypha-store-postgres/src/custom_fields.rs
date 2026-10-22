//! Custom field definitions and their values: one row per definition, one per (definition,
//! owner) value, the values' index columns derived by the domain (`ValueRow`).

use std::collections::HashSet;

use hypha_core::{
    CustomFieldDefinitionId, Page, PageLimit, PendingEvent, Recorded, RepoError, Timestamp,
    Version, Versioned, stamp_insert, stamp_update,
};
use hypha_custom_fields::{
    Access, AdminAccess, Capabilities, CustomFieldDefinition, DefinitionRepo, DefinitionSnapshot,
    ItemsChange, ItemsResult, Key, Namespace, OwnerItems, OwnerType, Reference, ReferenceKind,
    ReferenceTargets, StoredValue, StorefrontAccess, ValueCount, ValueRepo, ValueRow, list_items,
};
use tokio_postgres::{Row, error::SqlState, types::Type};
use uuid::Uuid;

use crate::{
    columns::{col, stamp_columns, stamps_from_row, ts, version_from_db, version_to_db},
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, Queries, in_transaction},
};

fn definition_columns() -> String {
    format!(
        "id, version, owner_type, namespace, key, name, description, field_type, \
         validations::text AS validations, admin_access, storefront_access, private, \
         personal, filterable, unique_values, position, {}",
        stamp_columns("")
    )
}

/// `list`'s order: by owner type as `OwnerType` orders them, then position.
const DEFINITION_ORDER: &str = "array_position(ARRAY['product', 'variant', 'category', \
     'customer', 'order'], owner_type), position, id";

fn admin_access_to_db(access: AdminAccess) -> &'static str {
    match access {
        AdminAccess::Read => "read",
        AdminAccess::ReadWrite => "read_write",
    }
}

fn admin_access_from_db(text: &str) -> Result<AdminAccess, RepoError> {
    match text {
        "read" => Ok(AdminAccess::Read),
        "read_write" => Ok(AdminAccess::ReadWrite),
        other => Err(corrupt(format!("admin access {other:?}"))),
    }
}

fn storefront_access_to_db(access: StorefrontAccess) -> &'static str {
    match access {
        StorefrontAccess::None => "none",
        StorefrontAccess::Read => "read",
    }
}

fn storefront_access_from_db(text: &str) -> Result<StorefrontAccess, RepoError> {
    match text {
        "none" => Ok(StorefrontAccess::None),
        "read" => Ok(StorefrontAccess::Read),
        other => Err(corrupt(format!("storefront access {other:?}"))),
    }
}

fn definition_from_row(row: &Row) -> Result<CustomFieldDefinition, RepoError> {
    let id = CustomFieldDefinitionId::from_uuid(col(row, "id")?);
    let bad =
        |what: &str, e: &dyn std::fmt::Display| corrupt(format!("definition {id} {what}: {e}"));
    let owner_type: String = col(row, "owner_type")?;
    let namespace: String = col(row, "namespace")?;
    let field_type: String = col(row, "field_type")?;
    let validations: String = col(row, "validations")?;
    let position: i32 = col(row, "position")?;
    CustomFieldDefinition::restore(DefinitionSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        owner_type: owner_type.parse().map_err(|e| bad("owner type", &e))?,
        namespace: Namespace::parse(&namespace).map_err(|e| bad("namespace", &e))?,
        key: col(row, "key")?,
        name: col(row, "name")?,
        description: col(row, "description")?,
        field_type: field_type.parse().map_err(|e| bad("type", &e))?,
        validations: serde_json::from_str(&validations).map_err(|e| bad("validations", &e))?,
        access: Access {
            admin: admin_access_from_db(&col::<String>(row, "admin_access")?)?,
            storefront: storefront_access_from_db(&col::<String>(row, "storefront_access")?)?,
        },
        private: col(row, "private")?,
        personal: col(row, "personal")?,
        capabilities: Capabilities {
            filterable: col(row, "filterable")?,
            unique: col(row, "unique_values")?,
        },
        position: u32::try_from(position).map_err(|_| bad("position", &position))?,
    })
    .map_err(|e| bad("", &e))
}

/// `CustomFieldDefinitionRepo::insert` inside the caller's transaction (also a batch's).
pub(crate) async fn insert_definition(
    tx: &impl Queries,
    s: &DefinitionSnapshot,
) -> Result<(), RepoError> {
    let validations = serde_json::to_string(&s.validations).map_err(corrupt)?;
    let created = s.stamps.map(|st| st.created_at.unix_micros());
    let updated = s.stamps.map(|st| st.updated_at.unix_micros());
    let position = i32::try_from(s.position).map_err(|_| corrupt("definition position"))?;
    tx.query_typed(
        &format!(
            "INSERT INTO custom_field_definitions (id, version, owner_type, namespace, key,
                 app, name, description, field_type, validations, admin_access,
                 storefront_access, private, personal, filterable, unique_values, position,
                 created_at, updated_at)
             VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9::text::jsonb, $10, $11, $12, $13,
                 $14, $15, $16, coalesce({}, now()), coalesce({}, now()))",
            ts(17),
            ts(18)
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.owner_type.as_str(), Type::TEXT),
            (&s.namespace.as_str(), Type::TEXT),
            (&s.key, Type::TEXT),
            (&s.namespace.managed_by().map(|k| k.as_str()), Type::TEXT),
            (&s.name, Type::TEXT),
            (&s.description, Type::TEXT),
            (&s.field_type.to_string(), Type::TEXT),
            (&validations, Type::TEXT),
            (&admin_access_to_db(s.access.admin), Type::TEXT),
            (&storefront_access_to_db(s.access.storefront), Type::TEXT),
            (&s.private, Type::BOOL),
            (&s.personal, Type::BOOL),
            (&s.capabilities.filterable, Type::BOOL),
            (&s.capabilities.unique, Type::BOOL),
            (&position, Type::INT4),
            (&created, Type::INT8),
            (&updated, Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// `CustomFieldDefinitionRepo::update` inside the caller's transaction; the new
/// version. Identity (owner type, namespace, key, type) never changes, so it isn't
/// written.
pub(crate) async fn update_definition(
    tx: &impl Queries,
    s: &DefinitionSnapshot,
) -> Result<Version, RepoError> {
    let validations = serde_json::to_string(&s.validations).map_err(corrupt)?;
    let position = i32::try_from(s.position).map_err(|_| corrupt("definition position"))?;
    let updated = s.stamps.map(|st| st.updated_at.unix_micros());
    let row = tx
        .query_typed_opt(
            &format!(
                "UPDATE custom_field_definitions SET version = version + 1, name = $3,
                     description = $4, validations = $5::text::jsonb, admin_access = $6,
                     storefront_access = $7, private = $8, personal = $9, filterable = $10,
                     unique_values = $11, position = $12,
                     updated_at = coalesce({}, updated_at)
                 WHERE id = $1 AND version = $2 RETURNING version",
                ts(13)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&s.description, Type::TEXT),
                (&validations, Type::TEXT),
                (&admin_access_to_db(s.access.admin), Type::TEXT),
                (&storefront_access_to_db(s.access.storefront), Type::TEXT),
                (&s.private, Type::BOOL),
                (&s.personal, Type::BOOL),
                (&s.capabilities.filterable, Type::BOOL),
                (&s.capabilities.unique, Type::BOOL),
                (&position, Type::INT4),
                (&updated, Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    version_from_db(col(&row, "version")?)
}

/// Deletes a definition at `version`; its values go with it (the foreign key cascades).
pub(crate) async fn delete_definition(
    tx: &impl Queries,
    id: CustomFieldDefinitionId,
    version: Version,
) -> Result<(), RepoError> {
    let deleted = tx
        .execute_typed(
            "DELETE FROM custom_field_definitions WHERE id = $1 AND version = $2",
            &[
                (&id.as_uuid(), Type::UUID),
                (&version_to_db(version)?, Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    if deleted == 0 {
        return Err(RepoError::StaleVersion);
    }
    Ok(())
}

impl<C: PgConnection> DefinitionRepo for PgStore<C> {
    async fn get(
        &self,
        id: CustomFieldDefinitionId,
    ) -> Result<Option<CustomFieldDefinition>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM custom_field_definitions WHERE id = $1",
                    definition_columns()
                ),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(definition_from_row).transpose()
    }

    async fn get_by_key(
        &self,
        owner_type: OwnerType,
        namespace: &Namespace,
        key: &Key,
    ) -> Result<Option<CustomFieldDefinition>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM custom_field_definitions
                     WHERE owner_type = $1 AND namespace = $2 AND key = $3",
                    definition_columns()
                ),
                &[
                    (&owner_type.as_str(), Type::TEXT),
                    (&namespace.as_str(), Type::TEXT),
                    (&key.as_str(), Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(definition_from_row).transpose()
    }

    async fn list(
        &self,
        owner_type: Option<OwnerType>,
    ) -> Result<Vec<CustomFieldDefinition>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM custom_field_definitions
                     WHERE $1::text IS NULL OR owner_type = $1
                     ORDER BY {DEFINITION_ORDER}",
                    definition_columns()
                ),
                &[(&owner_type.map(OwnerType::as_str), Type::TEXT)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(definition_from_row)
            .collect()
    }

    async fn list_namespace(
        &self,
        namespace: &Namespace,
    ) -> Result<Vec<CustomFieldDefinition>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM custom_field_definitions WHERE namespace = $1
                     ORDER BY {DEFINITION_ORDER}",
                    definition_columns()
                ),
                &[(&namespace.as_str(), Type::TEXT)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(definition_from_row)
            .collect()
    }

    async fn insert(&self, definition: &mut CustomFieldDefinition) -> Result<(), RepoError> {
        stamp_insert(definition, self.now());
        let s = definition.snapshot();
        let events = definition.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_definition(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        definition.set_version(version);
        Ok(())
    }

    async fn update(&self, definition: &mut CustomFieldDefinition) -> Result<(), RepoError> {
        stamp_update(definition, self.now());
        let s = definition.snapshot();
        let events = definition.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_definition(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        definition.set_version(version);
        Ok(())
    }

    async fn delete(&self, definition: &mut CustomFieldDefinition) -> Result<(), RepoError> {
        let (id, version) = (definition.id(), definition.version());
        let events = definition.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            delete_definition(tx, id, version).await?;
            insert_events(tx, &events, None, stamp).await
        })
    }
}

const VALUE_COLUMNS: &str = "definition_id, owner_id, owner_parent_id, value::text AS value, \
     version";

fn value_columns() -> String {
    format!("{VALUE_COLUMNS}, {}", stamp_columns(""))
}

fn value_from_row(row: &Row) -> Result<StoredValue, RepoError> {
    let value: String = col(row, "value")?;
    Ok(StoredValue {
        definition: CustomFieldDefinitionId::from_uuid(col(row, "definition_id")?),
        owner: col(row, "owner_id")?,
        owner_parent: col(row, "owner_parent_id")?,
        value: serde_json::from_str(&value).map_err(corrupt)?,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
    })
}

/// A unique violation of the values' primary key: a row that was expected not to exist
/// does (another write got there first), which is a stale write.
fn stale_on_existing_row(e: tokio_postgres::Error) -> RepoError {
    if let Some(db) = e.as_db_error()
        && *db.code() == SqlState::UNIQUE_VIOLATION
        && db.constraint() == Some("custom_field_values_pkey")
    {
        return RepoError::StaleVersion;
    }
    repo_error(e)
}

/// `StageCustomFieldValues::set_custom_field_value` inside a batch's transaction:
/// `expected` `None` writes whatever is stored, `Some(Version::NEW)` only a new row,
/// `Some(v)` only over version `v`.
pub(crate) async fn set_value(
    tx: &impl Queries,
    row: &ValueRow,
    expected: Option<Version>,
    now: Timestamp,
) -> Result<(), RepoError> {
    let value = row.value.to_string();
    let num = row.scalar_num.map(|n| n.to_string());
    let items = row.list_items.as_ref().map(|i| i.to_string());
    let now = now.unix_micros();
    let definition = row.definition.as_uuid();
    let params: [(&(dyn tokio_postgres::types::ToSql + Sync), Type); 9] = [
        (&definition, Type::UUID),
        (&row.owner, Type::UUID),
        (&row.owner_parent, Type::UUID),
        (&value, Type::TEXT),
        (&row.scalar_text, Type::TEXT),
        (&num, Type::TEXT),
        (&row.unique_text, Type::TEXT),
        (&now, Type::INT8),
        (&items, Type::TEXT),
    ];
    let insert = format!(
        "INSERT INTO custom_field_values (definition_id, owner_id, owner_parent_id, value,
             scalar_text, scalar_num, unique_text, list_items, version, created_at,
             updated_at)
         VALUES ($1, $2, $3, $4::text::jsonb, $5, $6::text::numeric, $7, $9::text::jsonb, 1,
                 {now}, {now})",
        now = ts(8)
    );
    match expected {
        None => {
            tx.execute_typed(
                &format!(
                    "{insert}
                     ON CONFLICT (definition_id, owner_id) DO UPDATE SET
                         owner_parent_id = EXCLUDED.owner_parent_id, value = EXCLUDED.value,
                         scalar_text = EXCLUDED.scalar_text, scalar_num = EXCLUDED.scalar_num,
                         unique_text = EXCLUDED.unique_text,
                         list_items = EXCLUDED.list_items,
                         version = custom_field_values.version + 1,
                         updated_at = EXCLUDED.updated_at"
                ),
                &params,
            )
            .await
            .map_err(repo_error)?;
        }
        Some(v) if v == Version::NEW => {
            tx.execute_typed(&insert, &params)
                .await
                .map_err(stale_on_existing_row)?;
        }
        Some(v) => {
            let version = version_to_db(v)?;
            let mut with_version = params.to_vec();
            with_version.push((&version, Type::INT8));
            let updated = tx
                .execute_typed(
                    &format!(
                        "UPDATE custom_field_values SET owner_parent_id = $3,
                             value = $4::text::jsonb, scalar_text = $5,
                             scalar_num = $6::text::numeric, unique_text = $7,
                             list_items = $9::text::jsonb,
                             version = version + 1, updated_at = {}
                         WHERE definition_id = $1 AND owner_id = $2 AND version = $10",
                        ts(8)
                    ),
                    &with_version,
                )
                .await
                .map_err(repo_error)?;
            if updated == 0 {
                return Err(RepoError::StaleVersion);
            }
        }
    }
    Ok(())
}

/// `StageCustomFieldValues::reindex_custom_field_value` inside a batch's transaction:
/// the derived columns rewritten, the value, version and `updated_at` kept, only over
/// version `at`.
pub(crate) async fn reindex_value(
    tx: &impl Queries,
    row: &ValueRow,
    at: Version,
) -> Result<(), RepoError> {
    let num = row.scalar_num.map(|n| n.to_string());
    let items = row.list_items.as_ref().map(|i| i.to_string());
    let updated = tx
        .execute_typed(
            "UPDATE custom_field_values SET scalar_text = $3, scalar_num = $4::text::numeric,
                 unique_text = $5, list_items = $6::text::jsonb
             WHERE definition_id = $1 AND owner_id = $2 AND version = $7",
            &[
                (&row.definition.as_uuid(), Type::UUID),
                (&row.owner, Type::UUID),
                (&row.scalar_text, Type::TEXT),
                (&num, Type::TEXT),
                (&row.unique_text, Type::TEXT),
                (&items, Type::TEXT),
                (&version_to_db(at)?, Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    if updated == 0 {
        return Err(RepoError::StaleVersion);
    }
    Ok(())
}

/// `StageCustomFieldValues::remove_custom_field_value` inside a batch's transaction.
pub(crate) async fn remove_value(
    tx: &impl Queries,
    definition: CustomFieldDefinitionId,
    owner: Uuid,
    expected: Option<Version>,
) -> Result<(), RepoError> {
    let definition = definition.as_uuid();
    match expected {
        None => {
            tx.execute_typed(
                "DELETE FROM custom_field_values WHERE definition_id = $1 AND owner_id = $2",
                &[(&definition, Type::UUID), (&owner, Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        }
        // Nothing to remove, and there must be nothing: a row is a stale expectation.
        Some(v) if v == Version::NEW => {
            let found = tx
                .query_typed_opt(
                    "SELECT 1 AS found FROM custom_field_values
                     WHERE definition_id = $1 AND owner_id = $2",
                    &[(&definition, Type::UUID), (&owner, Type::UUID)],
                )
                .await
                .map_err(repo_error)?;
            if found.is_some() {
                return Err(RepoError::StaleVersion);
            }
        }
        Some(v) => {
            let deleted = tx
                .execute_typed(
                    "DELETE FROM custom_field_values
                     WHERE definition_id = $1 AND owner_id = $2 AND version = $3",
                    &[
                        (&definition, Type::UUID),
                        (&owner, Type::UUID),
                        (&version_to_db(v)?, Type::INT8),
                    ],
                )
                .await
                .map_err(repo_error)?;
            if deleted == 0 {
                return Err(RepoError::StaleVersion);
            }
        }
    }
    Ok(())
}

impl<C: PgConnection> ValueRepo for PgStore<C> {
    async fn get_many(
        &self,
        owners: &[Uuid],
        definitions: Option<&[CustomFieldDefinitionId]>,
    ) -> Result<Vec<StoredValue>, RepoError> {
        if owners.is_empty() {
            return Ok(Vec::new());
        }
        let definitions: Option<Vec<Uuid>> =
            definitions.map(|ds| ds.iter().map(|d| d.as_uuid()).collect());
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM custom_field_values
                     WHERE owner_id = ANY($1)
                       AND ($2::uuid[] IS NULL OR definition_id = ANY($2))",
                    value_columns()
                ),
                &[
                    (&owners, Type::UUID_ARRAY),
                    (&definitions, Type::UUID_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(value_from_row)
            .collect()
    }

    async fn page(
        &self,
        definition: CustomFieldDefinitionId,
        after: Option<Uuid>,
        limit: PageLimit,
    ) -> Result<Page<StoredValue>, RepoError> {
        let fetch = i64::from(limit.get()) + 1;
        let mut conn = self.conn().await;
        let found = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM custom_field_values
                     WHERE definition_id = $1 AND ($2::uuid IS NULL OR owner_id > $2)
                     ORDER BY owner_id LIMIT $3",
                    value_columns()
                ),
                &[
                    (&definition.as_uuid(), Type::UUID),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(value_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(found, limit))
    }

    async fn find_unique(
        &self,
        definition: CustomFieldDefinitionId,
        unique_text: &str,
    ) -> Result<Option<StoredValue>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM custom_field_values
                     WHERE definition_id = $1 AND unique_text = $2",
                    value_columns()
                ),
                &[
                    (&definition.as_uuid(), Type::UUID),
                    (&unique_text, Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(value_from_row).transpose()
    }

    async fn values_in_use(
        &self,
        definition: CustomFieldDefinitionId,
        limit: u32,
    ) -> Result<Vec<ValueCount>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                r#"SELECT i.item::text AS value, count(DISTINCT v.owner_id) AS owners
                   FROM custom_field_values v
                   CROSS JOIN LATERAL jsonb_array_elements(
                       CASE WHEN jsonb_typeof(v.value) = 'array' THEN v.value
                            ELSE jsonb_build_array(v.value) END) AS i (item)
                   WHERE v.definition_id = $1
                   GROUP BY i.item
                   ORDER BY owners DESC, i.item::text COLLATE "C"
                   LIMIT $2"#,
                &[
                    (&definition.as_uuid(), Type::UUID),
                    (&i64::from(limit), Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|row| {
                let value: String = col(row, "value")?;
                let owners: i64 = col(row, "owners")?;
                Ok(ValueCount {
                    value: serde_json::from_str(&value).map_err(corrupt)?,
                    owners: u64::try_from(owners).map_err(corrupt)?,
                })
            })
            .collect()
    }

    async fn referencing(
        &self,
        definitions: &[CustomFieldDefinitionId],
        targets: &[String],
    ) -> Result<Vec<StoredValue>, RepoError> {
        if definitions.is_empty() || targets.is_empty() {
            return Ok(Vec::new());
        }
        let definitions: Vec<Uuid> = definitions.iter().map(|d| d.as_uuid()).collect();
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM custom_field_values
                     WHERE definition_id = ANY($1)
                       AND EXISTS (SELECT 1 FROM unnest($2::text[]) AS t(target)
                                   WHERE value = to_jsonb(t.target)
                                      OR value @> jsonb_build_array(t.target))",
                    value_columns()
                ),
                &[
                    (&definitions, Type::UUID_ARRAY),
                    (&targets, Type::TEXT_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(value_from_row)
            .collect()
    }

    /// One transaction. Each owner's (definition, owner) is locked first with a
    /// transaction advisory lock, in owner order, so concurrent deltas queue even on a
    /// row that doesn't exist yet (a row lock needs the row); then the rows are read
    /// `FOR UPDATE`, changed in Rust (`ItemsChange::apply`) and written back.
    async fn change_items(&self, change: &ItemsChange) -> Result<ItemsResult, RepoError> {
        let definition = change.definition.as_uuid();
        let mut owners: Vec<Uuid> = change.owners.iter().map(|o| o.id()).collect();
        owners.sort();
        owners.dedup();
        let now = self.now().unix_micros();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            let exists = tx
                .query_typed_opt(
                    "SELECT 1 AS found FROM custom_field_definitions WHERE id = $1 FOR KEY SHARE",
                    &[(&definition, Type::UUID)],
                )
                .await
                .map_err(repo_error)?;
            if exists.is_none() {
                return Err(RepoError::Backend(format!(
                    "custom field definition {} does not exist",
                    change.definition
                )));
            }
            tx.query_typed(
                "SELECT pg_advisory_xact_lock(hashtextextended($1::text || o::text, 0))
                 FROM unnest($2::uuid[]) AS o",
                &[(&definition, Type::UUID), (&owners, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
            let rows = tx
                .query_typed(
                    "SELECT owner_id, value::text AS value, version FROM custom_field_values
                     WHERE definition_id = $1 AND owner_id = ANY($2)
                     ORDER BY owner_id FOR UPDATE",
                    &[(&definition, Type::UUID), (&owners, Type::UUID_ARRAY)],
                )
                .await
                .map_err(repo_error)?;
            let mut current = std::collections::HashMap::new();
            for row in &rows {
                let owner: Uuid = col(row, "owner_id")?;
                let value: String = col(row, "value")?;
                let value: serde_json::Value = serde_json::from_str(&value).map_err(corrupt)?;
                current.insert(owner, (value, version_from_db(col(row, "version")?)?));
            }
            let mut applied = Vec::with_capacity(change.owners.len());
            for owner in &change.owners {
                let result = change.apply(current.get(&owner.id()).map(|(v, _)| v));
                if let Some(problem) = change.problem(&result) {
                    return Ok(ItemsResult::Refused {
                        owner: owner.id(),
                        problem,
                    });
                }
                applied.push((owner, result));
            }
            let mut events: Vec<PendingEvent> = Vec::new();
            let mut outcomes = Vec::with_capacity(applied.len());
            for (owner, result) in applied {
                let mut version = current.get(&owner.id()).map(|(_, v)| *v);
                if result.changed() {
                    let id = owner.id();
                    match &result.value {
                        Some(list) => {
                            let parent = owner.product_of_variant().map(|p| p.as_uuid());
                            let row = tx
                                .query_typed_one(
                                    &format!(
                                        "INSERT INTO custom_field_values (definition_id,
                                             owner_id, owner_parent_id, value, list_items,
                                             version, created_at, updated_at)
                                         VALUES ($1, $2, $3, $4::text::jsonb,
                                                 $6::text::jsonb, 1, {now}, {now})
                                         ON CONFLICT (definition_id, owner_id) DO UPDATE SET
                                             value = EXCLUDED.value,
                                             list_items = EXCLUDED.list_items,
                                             version = custom_field_values.version + 1,
                                             updated_at = EXCLUDED.updated_at
                                         RETURNING version",
                                        now = ts(5)
                                    ),
                                    &[
                                        (&definition, Type::UUID),
                                        (&id, Type::UUID),
                                        (&parent, Type::UUID),
                                        (&list.to_string(), Type::TEXT),
                                        (&now, Type::INT8),
                                        (&list_items(list).map(|i| i.to_string()), Type::TEXT),
                                    ],
                                )
                                .await
                                .map_err(repo_error)?;
                            version = Some(version_from_db(col(&row, "version")?)?);
                        }
                        None => {
                            tx.execute_typed(
                                "DELETE FROM custom_field_values
                                 WHERE definition_id = $1 AND owner_id = $2",
                                &[(&definition, Type::UUID), (&id, Type::UUID)],
                            )
                            .await
                            .map_err(repo_error)?;
                            version = None;
                        }
                    }
                    events.extend(change.event(owner, &result));
                }
                outcomes.push(OwnerItems {
                    owner: owner.id(),
                    added: result.added,
                    removed: result.removed,
                    version,
                });
            }
            insert_events(tx, &events, None, stamp).await?;
            Ok(ItemsResult::Applied(outcomes))
        })
    }

    async fn delete_for_owners(&self, owners: &[Uuid]) -> Result<u32, RepoError> {
        if owners.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn().await;
        let deleted = conn
            .db()
            .execute_typed(
                "DELETE FROM custom_field_values WHERE owner_id = ANY($1)",
                &[(&owners, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        u32::try_from(deleted).map_err(corrupt)
    }
}

impl<C: PgConnection> ReferenceTargets for PgStore<C> {
    /// One query over the targets' tables, each kind's IDs against its own.
    async fn existing(&self, references: &[Reference]) -> Result<HashSet<Reference>, RepoError> {
        if references.is_empty() {
            return Ok(HashSet::new());
        }
        let of = |kind: ReferenceKind| -> Vec<Uuid> {
            references
                .iter()
                .filter(|r| r.kind == kind)
                .map(|r| r.id)
                .collect()
        };
        let (products, variants, categories, customers, assets) = (
            of(ReferenceKind::Product),
            of(ReferenceKind::Variant),
            of(ReferenceKind::Category),
            of(ReferenceKind::Customer),
            of(ReferenceKind::Asset),
        );
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT 'product' AS kind, id FROM products WHERE id = ANY($1)
                 UNION ALL SELECT 'variant', id FROM variants WHERE id = ANY($2)
                 UNION ALL SELECT 'category', id FROM categories WHERE id = ANY($3)
                 UNION ALL SELECT 'customer', id FROM customers WHERE id = ANY($4)
                 UNION ALL SELECT 'asset', id FROM assets WHERE id = ANY($5)",
                &[
                    (&products, Type::UUID_ARRAY),
                    (&variants, Type::UUID_ARRAY),
                    (&categories, Type::UUID_ARRAY),
                    (&customers, Type::UUID_ARRAY),
                    (&assets, Type::UUID_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?;
        rows.iter()
            .map(|row| {
                let kind: String = col(row, "kind")?;
                Ok(Reference {
                    kind: ReferenceKind::parse(&kind)
                        .ok_or_else(|| corrupt(format!("reference kind {kind:?}")))?,
                    id: col(row, "id")?,
                })
            })
            .collect()
    }
}
