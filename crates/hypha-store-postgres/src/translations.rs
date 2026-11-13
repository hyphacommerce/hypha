//! Translations: one row per resource and language, the translated fields as JSON.

use std::collections::BTreeMap;

use hypha_core::{
    Language, Page, PageRequest, Recorded, RepoError, Version, Versioned, stamp_insert,
    stamp_update,
};
use hypha_translations::{
    FieldCounts, LanguageProgress, QueuedSource, ResourceKind, ResourceRef, SourceDigest,
    TranslatedField, Translation, TranslationFilter, TranslationKey, TranslationRepo,
    TranslationSnapshot, TranslationSourceRepo, WorkItem, WorklistFilter,
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

/// A kind's place in the catalog (`ResourceKind::ALL`): what a worklist is ordered by.
fn position(kind: ResourceKind) -> i32 {
    ResourceKind::ALL
        .iter()
        .position(|k| *k == kind)
        .and_then(|p| i32::try_from(p).ok())
        .unwrap_or(i32::MAX)
}

fn resource_from_row(row: &Row) -> Result<ResourceRef, RepoError> {
    let kind: String = col(row, "resource_kind")?;
    let kind = kind
        .parse::<ResourceKind>()
        .map_err(|e| corrupt(format!("translation source: {e}")))?;
    Ok(ResourceRef::new(kind, col(row, "resource_id")?))
}

fn count(row: &Row, name: &str) -> Result<u32, RepoError> {
    let n: i64 = col(row, name)?;
    u32::try_from(n).map_err(|_| corrupt(format!("translation source: {name} is {n}")))
}

fn total(row: &Row, name: &str) -> Result<u64, RepoError> {
    let n: i64 = col(row, name)?;
    u64::try_from(n).map_err(|_| corrupt(format!("translation progress: {name} is {n}")))
}

/// Whether the resource's translation lacks the source field `s.key` (`tr` is the
/// translation, or NULL without one).
const MISSING: &str = "count(*) FILTER (WHERE tr.fields IS NULL OR NOT tr.fields ? s.key)";
/// Whether it translates a source text other than `s.digest`.
const OUTDATED: &str = "count(*) FILTER (WHERE tr.fields -> s.key ->> 'source_digest' <> s.digest)";

impl<C: PgConnection> TranslationSourceRepo for PgStore<C> {
    async fn replace_sources(
        &self,
        resource: ResourceRef,
        digests: &[SourceDigest],
    ) -> Result<(), RepoError> {
        let keys: Vec<&str> = digests.iter().map(|d| d.key.as_str()).collect();
        let values: Vec<&str> = digests.iter().map(|d| d.digest.as_str()).collect();
        let kind = resource.kind.as_str();
        // One statement: the keys gone are deleted and the rest upserted together, so
        // two runs for the same resource can't trip over each other's rows.
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "WITH gone AS (
                     DELETE FROM translation_sources
                     WHERE resource_kind = $1 AND resource_id = $2 AND key <> ALL($3::text[])
                 )
                 INSERT INTO translation_sources (resource_kind, resource_id, key, digest)
                 SELECT $1, $2, k, d FROM unnest($3::text[], $4::text[]) AS n (k, d)
                 ON CONFLICT (resource_kind, resource_id, key)
                     DO UPDATE SET digest = EXCLUDED.digest
                     WHERE translation_sources.digest <> EXCLUDED.digest",
                &[
                    (&kind, Type::TEXT),
                    (&resource.id, Type::UUID),
                    (&keys, Type::TEXT_ARRAY),
                    (&values, Type::TEXT_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }

    async fn sources(
        &self,
        resources: &[ResourceRef],
    ) -> Result<Vec<(ResourceRef, SourceDigest)>, RepoError> {
        if resources.is_empty() {
            return Ok(Vec::new());
        }
        let kinds: Vec<&str> = resources.iter().map(|r| r.kind.as_str()).collect();
        let ids: Vec<Uuid> = resources.iter().map(|r| r.id).collect();
        let mut conn = self.conn().await;
        let mut found = conn
            .db()
            .query_typed(
                "SELECT resource_kind, resource_id, key, digest FROM translation_sources
                 WHERE (resource_kind, resource_id) IN
                           (SELECT k, i FROM unnest($1::text[], $2::uuid[]) AS r (k, i))",
                &[(&kinds, Type::TEXT_ARRAY), (&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|row| {
                Ok((
                    resource_from_row(row)?,
                    SourceDigest {
                        key: col(row, "key")?,
                        digest: col(row, "digest")?,
                    },
                ))
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        // `ResourceRef` order is the catalog's, which a column's isn't.
        found.sort();
        Ok(found)
    }

    async fn worklist(
        &self,
        filter: &WorklistFilter,
        page: PageRequest<ResourceRef>,
    ) -> Result<Page<WorkItem>, RepoError> {
        if filter.kinds.is_empty() {
            return Ok(Page::from_overfetch(Vec::new(), page.limit));
        }
        let kinds: Vec<&str> = filter.kinds.iter().map(|k| k.as_str()).collect();
        let positions: Vec<i32> = filter.kinds.iter().map(|k| position(*k)).collect();
        let language = filter.language.as_str();
        let after_position = page.after.map(|a| position(a.kind));
        let after_id = page.after.map(|a| a.id);
        let state = filter.state.map(|s| s.as_str());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        // Kind by kind in the catalog's order, each streamed by ID from the key's index
        // and cut at the page's length, so a page reads about as many rows as it shows
        // (and the ones the state passes over).
        let rows = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT w.resource_kind, w.resource_id, $2::text AS language,
                            w.total, w.missing, w.outdated,
                            t.version, t.fields::text AS fields, {stamps}
                     FROM unnest($1::text[], $3::int4[]) AS k (kind, pos)
                     CROSS JOIN LATERAL (
                         SELECT s.resource_kind, s.resource_id, count(*) AS total,
                                {MISSING} AS missing, {OUTDATED} AS outdated
                         FROM translation_sources s
                         LEFT JOIN translations tr
                                ON tr.resource_kind = s.resource_kind
                               AND tr.resource_id = s.resource_id AND tr.language = $2
                         WHERE s.resource_kind = k.kind
                           AND ($4::int4 IS NULL OR k.pos > $4 OR s.resource_id > $5::uuid)
                         GROUP BY s.resource_kind, s.resource_id
                         HAVING CASE $6::text
                             WHEN 'missing' THEN {MISSING} > 0
                             WHEN 'outdated' THEN {MISSING} = 0 AND {OUTDATED} > 0
                             WHEN 'complete' THEN {MISSING} = 0 AND {OUTDATED} = 0
                             ELSE true
                         END
                         ORDER BY s.resource_id
                         LIMIT $7
                     ) AS w
                     LEFT JOIN translations t
                            ON t.resource_kind = w.resource_kind
                           AND t.resource_id = w.resource_id AND t.language = $2
                     WHERE $4::int4 IS NULL OR k.pos >= $4
                     ORDER BY k.pos, w.resource_id
                     LIMIT $7",
                    stamps = stamp_columns("t."),
                ),
                &[
                    (&kinds, Type::TEXT_ARRAY),
                    (&language, Type::TEXT),
                    (&positions, Type::INT4_ARRAY),
                    (&after_position, Type::INT4),
                    (&after_id, Type::UUID),
                    (&state, Type::TEXT),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        let items = rows
            .iter()
            .map(|row| {
                let version: Option<i64> = col(row, "version")?;
                Ok(WorkItem {
                    resource: resource_from_row(row)?,
                    counts: FieldCounts {
                        total: count(row, "total")?,
                        missing: count(row, "missing")?,
                        outdated: count(row, "outdated")?,
                    },
                    translation: version
                        .is_some()
                        .then(|| translation_from_row(row))
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        Ok(Page::from_overfetch(items, page.limit))
    }

    async fn progress(
        &self,
        kinds: &[ResourceKind],
        languages: &[Language],
    ) -> Result<Vec<LanguageProgress>, RepoError> {
        if languages.is_empty() {
            return Ok(Vec::new());
        }
        let kinds: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
        let tags: Vec<&str> = languages.iter().map(Language::as_str).collect();
        let mut conn = self.conn().await;
        // The total from the sources' index alone; then the sources merged with the
        // languages' translations by resource, both in key order, each source field
        // looked up in its translation: the work follows the sources and what's
        // translated, not languages × fields.
        let rows = conn
            .db()
            .query_typed(
                "WITH total AS (
                     SELECT count(*) AS n FROM translation_sources
                     WHERE resource_kind = ANY($1::text[])
                 ),
                 done AS (
                     SELECT t.language,
                            count(*) FILTER (WHERE t.fields ? s.key) AS translated,
                            count(*) FILTER (WHERE t.fields -> s.key ->> 'source_digest' <> s.digest)
                                AS outdated
                     FROM translation_sources s
                     JOIN translations t
                       ON t.resource_kind = s.resource_kind AND t.resource_id = s.resource_id
                     WHERE s.resource_kind = ANY($1::text[]) AND t.language = ANY($2::text[])
                     GROUP BY t.language
                 )
                 SELECT l.language, total.n AS total,
                        coalesce(done.translated, 0) AS translated,
                        coalesce(done.outdated, 0) AS outdated
                 FROM unnest($2::text[]) WITH ORDINALITY AS l (language, pos)
                 CROSS JOIN total
                 LEFT JOIN done ON done.language = l.language
                 ORDER BY l.pos",
                &[(&kinds, Type::TEXT_ARRAY), (&tags, Type::TEXT_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter()
            .zip(languages)
            .map(|(row, language)| {
                Ok(LanguageProgress {
                    language: language.clone(),
                    total: total(row, "total")?,
                    translated: total(row, "translated")?,
                    outdated: total(row, "outdated")?,
                })
            })
            .collect()
    }

    async fn queue(&self, resources: &[ResourceRef]) -> Result<(), RepoError> {
        if resources.is_empty() {
            return Ok(());
        }
        let kinds: Vec<&str> = resources.iter().map(|r| r.kind.as_str()).collect();
        let ids: Vec<Uuid> = resources.iter().map(|r| r.id).collect();
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "INSERT INTO translation_source_queue (resource_kind, resource_id)
                 SELECT DISTINCT k, i FROM unnest($1::text[], $2::uuid[]) AS r (k, i)
                 ON CONFLICT (resource_kind, resource_id)
                     DO UPDATE SET mark = nextval('translation_source_marks')",
                &[(&kinds, Type::TEXT_ARRAY), (&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }

    async fn queue_kind(&self, kind: ResourceKind) -> Result<(), RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "INSERT INTO translation_source_queue (resource_kind, resource_id)
                 SELECT DISTINCT resource_kind, resource_id FROM translation_sources
                 WHERE resource_kind = $1
                 ON CONFLICT (resource_kind, resource_id)
                     DO UPDATE SET mark = nextval('translation_source_marks')",
                &[(&kind.as_str(), Type::TEXT)],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }

    async fn pending(&self, kinds: &[ResourceKind]) -> Result<u64, RepoError> {
        let kinds: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
        let mut conn = self.conn().await;
        // Counted from the key's index, which leads with the kind.
        let row = conn
            .db()
            .query_typed_one(
                "SELECT count(*) AS n FROM translation_source_queue
                 WHERE resource_kind = ANY($1::text[])",
                &[(&kinds, Type::TEXT_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        total(&row, "n")
    }

    async fn queued(&self, limit: u32) -> Result<Vec<QueuedSource>, RepoError> {
        let limit = i64::from(limit);
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "SELECT resource_kind, resource_id, mark FROM translation_source_queue
                 ORDER BY mark LIMIT $1",
                &[(&limit, Type::INT8)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|row| {
                Ok(QueuedSource {
                    resource: resource_from_row(row)?,
                    mark: col(row, "mark")?,
                })
            })
            .collect()
    }

    async fn unqueue(&self, done: &[QueuedSource]) -> Result<(), RepoError> {
        if done.is_empty() {
            return Ok(());
        }
        let kinds: Vec<&str> = done.iter().map(|q| q.resource.kind.as_str()).collect();
        let ids: Vec<Uuid> = done.iter().map(|q| q.resource.id).collect();
        let marks: Vec<i64> = done.iter().map(|q| q.mark).collect();
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "DELETE FROM translation_source_queue
                 WHERE (resource_kind, resource_id, mark) IN
                     (SELECT k, i, m FROM unnest($1::text[], $2::uuid[], $3::int8[]) AS d (k, i, m))",
                &[
                    (&kinds, Type::TEXT_ARRAY),
                    (&ids, Type::UUID_ARRAY),
                    (&marks, Type::INT8_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }

    async fn touch_queued(&self, resources: &[ResourceRef]) -> Result<(), RepoError> {
        if resources.is_empty() {
            return Ok(());
        }
        let kinds: Vec<&str> = resources.iter().map(|r| r.kind.as_str()).collect();
        let ids: Vec<Uuid> = resources.iter().map(|r| r.id).collect();
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "UPDATE translation_source_queue SET mark = nextval('translation_source_marks')
                 WHERE (resource_kind, resource_id) IN
                           (SELECT k, i FROM unnest($1::text[], $2::uuid[]) AS r (k, i))",
                &[(&kinds, Type::TEXT_ARRAY), (&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }
}
