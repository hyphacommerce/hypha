//! Translations: one row per resource and language, the translated fields as JSON.

use std::collections::BTreeMap;

use hypha_core::{
    Language, Page, PageRequest, Recorded, RepoError, Version, Versioned, stamp_insert,
    stamp_update,
};
use hypha_translations::{
    ResourceKind, ResourceRef, TranslatedField, Translation, TranslationFilter, TranslationKey,
    TranslationRepo, TranslationSnapshot,
};
use serde::{Deserialize, Serialize};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{col, stamp_columns, stamps_from_row, ts, version_from_db, version_to_db},
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, Queries, in_transaction},
};

/// A translated field as the `fields` column holds it. Stored data: add to it, never
/// rename.
#[derive(Serialize, Deserialize)]
struct StoredField {
    value: String,
    source_digest: String,
}

fn columns() -> String {
    format!(
        "resource_kind, resource_id, language, version, fields::text AS fields, {}",
        stamp_columns("")
    )
}

/// In `TranslationKey` order: the columns compare as bytes.
const ORDER: &str = "ORDER BY resource_kind, resource_id, language";

fn translation_from_row(row: &Row) -> Result<Translation, RepoError> {
    let kind: String = col(row, "resource_kind")?;
    let id: Uuid = col(row, "resource_id")?;
    let language: String = col(row, "language")?;
    let named = || format!("translation of {kind} {id} into {language}");
    let fields: String = col(row, "fields")?;
    let fields: BTreeMap<String, StoredField> =
        serde_json::from_str(&fields).map_err(|e| corrupt(format!("{}: {e}", named())))?;
    Translation::restore(TranslationSnapshot {
        resource: ResourceRef::new(
            kind.parse::<ResourceKind>()
                .map_err(|e| corrupt(format!("{}: {e}", named())))?,
            id,
        ),
        language: Language::new(&language).map_err(|e| corrupt(format!("{}: {e}", named())))?,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        fields: fields
            .into_iter()
            .map(|(key, f)| {
                (
                    key,
                    TranslatedField {
                        value: f.value,
                        source_digest: f.source_digest,
                    },
                )
            })
            .collect(),
    })
    .map_err(|e| corrupt(format!("{}: {e}", named())))
}

/// A translation's columns as owned values, so the parameters can borrow them.
struct Values {
    kind: &'static str,
    id: Uuid,
    language: String,
    fields: String,
}

impl Values {
    fn of(s: &TranslationSnapshot) -> Result<Self, RepoError> {
        let fields: BTreeMap<&str, StoredField> = s
            .fields
            .iter()
            .map(|(key, f)| {
                (
                    key.as_str(),
                    StoredField {
                        value: f.value.clone(),
                        source_digest: f.source_digest.clone(),
                    },
                )
            })
            .collect();
        Ok(Self {
            kind: s.resource.kind.as_str(),
            id: s.resource.id,
            language: s.language.as_str().to_string(),
            fields: serde_json::to_string(&fields)
                .map_err(|e| corrupt(format!("translation: {e}")))?,
        })
    }
}

/// `write_stamps` for a translation, whose key is three columns.
async fn translation_stamps(
    tx: &impl Queries,
    v: &Values,
    s: &TranslationSnapshot,
) -> Result<(), RepoError> {
    let Some(stamps) = s.stamps else {
        return Ok(());
    };
    tx.query_typed(
        &format!(
            "UPDATE translations SET created_at = {}, updated_at = {}
             WHERE resource_kind = $1 AND resource_id = $2 AND language = $3",
            ts(4),
            ts(5)
        ),
        &[
            (&v.kind, Type::TEXT),
            (&v.id, Type::UUID),
            (&v.language, Type::TEXT),
            (&stamps.created_at.unix_micros(), Type::INT8),
            (&stamps.updated_at.unix_micros(), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

impl<C: PgConnection> TranslationRepo for PgStore<C> {
    async fn get(
        &self,
        resource: ResourceRef,
        language: &Language,
    ) -> Result<Option<Translation>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM translations
                     WHERE resource_kind = $1 AND resource_id = $2 AND language = $3",
                    columns()
                ),
                &[
                    (&resource.kind.as_str(), Type::TEXT),
                    (&resource.id, Type::UUID),
                    (&language.as_str(), Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(translation_from_row).transpose()
    }

    async fn for_resource(&self, resource: ResourceRef) -> Result<Vec<Translation>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM translations
                     WHERE resource_kind = $1 AND resource_id = $2 {ORDER}",
                    columns()
                ),
                &[
                    (&resource.kind.as_str(), Type::TEXT),
                    (&resource.id, Type::UUID),
                ],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(translation_from_row)
            .collect()
    }

    async fn load(
        &self,
        resources: &[ResourceRef],
        languages: &[Language],
    ) -> Result<Vec<Translation>, RepoError> {
        if resources.is_empty() || languages.is_empty() {
            return Ok(Vec::new());
        }
        let kinds: Vec<&str> = resources.iter().map(|r| r.kind.as_str()).collect();
        let ids: Vec<Uuid> = resources.iter().map(|r| r.id).collect();
        let languages: Vec<&str> = languages.iter().map(Language::as_str).collect();
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM translations
                     WHERE (resource_kind, resource_id) IN
                               (SELECT k, i FROM unnest($1::text[], $2::uuid[]) AS r (k, i))
                       AND language = ANY($3::text[])
                     {ORDER}",
                    columns()
                ),
                &[
                    (&kinds, Type::TEXT_ARRAY),
                    (&ids, Type::UUID_ARRAY),
                    (&languages, Type::TEXT_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(translation_from_row)
            .collect()
    }

    async fn list(
        &self,
        filter: &TranslationFilter,
        page: PageRequest<TranslationKey>,
    ) -> Result<Page<Translation>, RepoError> {
        let kinds: Option<Vec<&str>> = filter
            .kinds
            .as_ref()
            .map(|kinds| kinds.iter().map(|k| k.as_str()).collect());
        let language = filter.language.as_ref().map(Language::as_str);
        let after_kind = page.after.as_ref().map(|a| a.resource.kind.as_str());
        let after_id = page.after.as_ref().map(|a| a.resource.id);
        let after_language = page.after.as_ref().map(|a| a.language.as_str());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let found = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM translations
                     WHERE ($1::text[] IS NULL OR resource_kind = ANY($1))
                       AND ($2::text IS NULL OR language = $2)
                       AND ($3::text IS NULL
                            OR (resource_kind, resource_id, language) > ($3, $4::uuid, $5))
                     {ORDER} LIMIT $6",
                    columns()
                ),
                &[
                    (&kinds, Type::TEXT_ARRAY),
                    (&language, Type::TEXT),
                    (&after_kind, Type::TEXT),
                    (&after_id, Type::UUID),
                    (&after_language, Type::TEXT),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(translation_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn any_in(&self, languages: &[Language]) -> Result<bool, RepoError> {
        let languages: Vec<&str> = languages.iter().map(Language::as_str).collect();
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                "SELECT 1 FROM translations WHERE language = ANY($1::text[]) LIMIT 1",
                &[(&languages, Type::TEXT_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        Ok(row.is_some())
    }

    async fn insert(&self, translation: &mut Translation) -> Result<(), RepoError> {
        stamp_insert(translation, self.now());
        let s = translation.snapshot();
        let events = translation.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        let v = Values::of(&s)?;
        in_transaction!(self, |tx| {
            tx.query_typed(
                "INSERT INTO translations (resource_kind, resource_id, language, version, fields)
                 VALUES ($1, $2, $3, 1, $4::text::jsonb)",
                &[
                    (&v.kind, Type::TEXT),
                    (&v.id, Type::UUID),
                    (&v.language, Type::TEXT),
                    (&v.fields, Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
            translation_stamps(tx, &v, &s).await?;
            // No subject version: the subject is the resource, whose languages each
            // count their own versions, so they order nothing between them.
            insert_events(tx, &events, None, stamp).await
        })?;
        translation.set_version(version);
        Ok(())
    }

    async fn update(&self, translation: &mut Translation) -> Result<(), RepoError> {
        stamp_update(translation, self.now());
        let s = translation.snapshot();
        let events = translation.take_events();
        let stamp = self.event_stamp();
        let current = version_to_db(s.version)?;
        let v = Values::of(&s)?;
        let version = in_transaction!(self, |tx| {
            let row = tx
                .query_typed_opt(
                    "UPDATE translations SET version = version + 1, fields = $5::text::jsonb
                     WHERE resource_kind = $1 AND resource_id = $2 AND language = $3
                       AND version = $4
                     RETURNING version",
                    &[
                        (&v.kind, Type::TEXT),
                        (&v.id, Type::UUID),
                        (&v.language, Type::TEXT),
                        (&current, Type::INT8),
                        (&v.fields, Type::TEXT),
                    ],
                )
                .await
                .map_err(repo_error)?
                .ok_or(RepoError::StaleVersion)?;
            translation_stamps(tx, &v, &s).await?;
            let version = version_from_db(col(&row, "version")?)?;
            insert_events(tx, &events, None, stamp).await?;
            Ok(version)
        })?;
        translation.set_version(version);
        Ok(())
    }

    async fn delete(&self, translation: &mut Translation) -> Result<(), RepoError> {
        let resource = translation.resource();
        let language = translation.language().as_str().to_string();
        let version = version_to_db(translation.version())?;
        let events = translation.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            let deleted = tx
                .query_typed_opt(
                    "DELETE FROM translations
                     WHERE resource_kind = $1 AND resource_id = $2 AND language = $3
                       AND version = $4
                     RETURNING version",
                    &[
                        (&resource.kind.as_str(), Type::TEXT),
                        (&resource.id, Type::UUID),
                        (&language, Type::TEXT),
                        (&version, Type::INT8),
                    ],
                )
                .await
                .map_err(repo_error)?;
            if deleted.is_none() {
                return Err(RepoError::StaleVersion);
            }
            insert_events(tx, &events, None, stamp).await
        })
    }

    async fn delete_resources(&self, resources: &[ResourceRef]) -> Result<(), RepoError> {
        if resources.is_empty() {
            return Ok(());
        }
        let kinds: Vec<&str> = resources.iter().map(|r| r.kind.as_str()).collect();
        let ids: Vec<Uuid> = resources.iter().map(|r| r.id).collect();
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "DELETE FROM translations
                 WHERE (resource_kind, resource_id) IN
                           (SELECT k, i FROM unnest($1::text[], $2::uuid[]) AS r (k, i))",
                &[(&kinds, Type::TEXT_ARRAY), (&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }

    async fn retain_languages(&self, languages: &[Language]) -> Result<(), RepoError> {
        let languages: Vec<&str> = languages.iter().map(Language::as_str).collect();
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "DELETE FROM translations WHERE language <> ALL($1::text[])",
                &[(&languages, Type::TEXT_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }
}
