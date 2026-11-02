use hypha_core::{
    AssetId, Direction, Keyset, Page, PageRequest, Recorded, RepoError, SortKey, Timestamp,
    UploadId, Version, Versioned,
};
use hypha_media::{
    Asset, AssetFilter, AssetRepo, AssetSnapshot, AssetSort, AssetUsage, AssetUsageRepo, BlobKey,
    Checksum, MimeType, TemporaryUpload, TemporaryUploadRepo, TemporaryUploadSnapshot,
    UploadPurpose, UsageOwner, UsageRole,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{col, micros, ts, version_from_db, version_to_db},
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::Queries,
    store::{PgConnection, PgStore, in_transaction},
};

/// `assets`' columns, as `asset_from_db` reads them.
fn columns() -> String {
    format!(
        "id, version, status, filename, blob_key, file_version, mime_type, byte_size, \
         checksum, width, height, alt, title, visibility, {}, {}",
        micros("created_at"),
        micros("updated_at"),
    )
}

// An asset is one row, so every read is one query and needs no snapshot transaction.
impl<C: PgConnection> AssetRepo for PgStore<C> {
    async fn get(&self, id: AssetId) -> Result<Option<Asset>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!("SELECT {} FROM assets WHERE id = $1", columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(asset_from_db).transpose()
    }

    async fn get_many(&self, ids: &[AssetId]) -> Result<Vec<Asset>, RepoError> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let ids: Vec<Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                &format!("SELECT {} FROM assets WHERE id = ANY($1)", columns()),
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter().map(asset_from_db).collect()
    }

    /// Names sort by `lower(filename) COLLATE "C"`, as `SortKey::text` does (index in
    /// migration 0045); the text filter is `strpos`, like products'.
    async fn list(
        &self,
        filter: &AssetFilter,
        sort: AssetSort,
        page: PageRequest<Keyset<AssetId>>,
    ) -> Result<Page<Asset>, RepoError> {
        let kind = filter.kind().map(|k| k.as_str());
        let (cmp, dir) = match sort.direction() {
            Direction::Ascending => (">", "ASC"),
            Direction::Descending => ("<", "DESC"),
        };
        let mut name_key = None::<String>;
        let after = match &page.after {
            None => None,
            Some(after) => {
                match (&after.key, sort) {
                    (SortKey::Id, AssetSort::Newest | AssetSort::Oldest) => {}
                    (SortKey::Text(t), AssetSort::NameAsc | AssetSort::NameDesc) => {
                        name_key = Some(t.clone());
                    }
                    (key, sort) => {
                        return Err(RepoError::Backend(format!(
                            "a {key:?} position for a {sort:?} list"
                        )));
                    }
                }
                Some(after.id.as_uuid())
            }
        };
        let (position, order) = match sort {
            AssetSort::Newest | AssetSort::Oldest => (format!("id {cmp} $2"), format!("id {dir}")),
            AssetSort::NameAsc | AssetSort::NameDesc => (
                format!(
                    r#"((lower(filename) COLLATE "C"), id) {cmp} (($5::text COLLATE "C"), $2)"#
                ),
                format!(r#"(lower(filename) COLLATE "C") {dir}, id {dir}"#),
            ),
        };
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM assets
                     WHERE status = 'ready' AND ($1::text IS NULL OR kind = $1)
                       AND ($2::uuid IS NULL OR {position})
                       AND ($4::text IS NULL
                            OR strpos(lower(filename), lower($4)) > 0
                            OR strpos(lower(alt), lower($4)) > 0
                            OR strpos(lower(title), lower($4)) > 0)
                     ORDER BY {order} LIMIT $3",
                    columns(),
                ),
                &[
                    (&kind, Type::TEXT),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
                    (&filter.q(), Type::TEXT),
                    (&name_key, Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        let assets = rows
            .iter()
            .map(asset_from_db)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(assets, page.limit))
    }

    async fn abandoned(&self, before: Timestamp, limit: u32) -> Result<Vec<Asset>, RepoError> {
        let before = before.unix_micros();
        let limit = i64::from(limit);
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM assets
                     WHERE (status = 'pending' AND created_at < {before_at})
                        OR (status = 'failed' AND updated_at < {before_at})
                     ORDER BY id LIMIT $2",
                    columns(),
                    before_at = ts(1),
                ),
                &[(&before, Type::INT8), (&limit, Type::INT8)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter().map(asset_from_db).collect()
    }

    async fn insert(&self, asset: &mut Asset) -> Result<(), RepoError> {
        let s = asset.snapshot();
        in_transaction!(self, |tx| insert_asset(tx, &s).await)?;
        asset.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, asset: &mut Asset) -> Result<(), RepoError> {
        let s = asset.snapshot();
        let version = in_transaction!(self, |tx| update_asset(tx, &s).await)?;
        asset.set_version(version);
        Ok(())
    }

    async fn delete(&self, asset: &mut Asset) -> Result<(), RepoError> {
        let id = asset.id();
        let events = asset.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            if delete_asset(tx, id).await? {
                insert_events(tx, &events, None, stamp).await?;
            }
            Ok(())
        })
    }
}

/// `AssetRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_asset(tx: &impl Queries, s: &AssetSnapshot) -> Result<(), RepoError> {
    tx.query_typed(
        &format!(
            "INSERT INTO assets (id, version, status, kind, filename, blob_key,
                 file_version, mime_type, byte_size, checksum, width, height, alt,
                 title, visibility, created_at, updated_at)
             VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                 {}, {})",
            ts(15),
            ts(16),
        ),
        &RowValues::new(s)?.params(),
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// `AssetRepo::update` inside the caller's transaction; the new version.
pub(crate) async fn update_asset(
    tx: &impl Queries,
    s: &AssetSnapshot,
) -> Result<Version, RepoError> {
    let expected = version_to_db(s.version)?;
    let values = RowValues::new(s)?;
    let mut params = values.params();
    params.push((&expected, Type::INT8));
    let bumped = tx
        .query_typed_opt(
            &format!(
                "UPDATE assets SET version = version + 1, status = $2, kind = $3,
                     filename = $4, blob_key = $5, file_version = $6, mime_type = $7,
                     byte_size = $8, checksum = $9, width = $10, height = $11, alt = $12,
                     title = $13, visibility = $14, created_at = {}, updated_at = {}
                 WHERE id = $1 AND version = $17 RETURNING version",
                ts(15),
                ts(16),
            ),
            &params,
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    version_from_db(col(&row, "version")?)
}

/// `AssetRepo::delete` inside the caller's transaction. An asset in use is refused by
/// the usage reference (`RepoError::Reference`).
/// Whether there was a row to delete.
pub(crate) async fn delete_asset(tx: &impl Queries, id: AssetId) -> Result<bool, RepoError> {
    let gone = tx
        .query_typed_opt(
            "DELETE FROM assets WHERE id = $1 RETURNING id",
            &[(&id.as_uuid(), Type::UUID)],
        )
        .await
        .map_err(repo_error)?;
    Ok(gone.is_some())
}

impl<C: PgConnection> AssetUsageRepo for PgStore<C> {
    async fn list_usages(
        &self,
        asset: AssetId,
        page: PageRequest<UsageOwner>,
    ) -> Result<Page<AssetUsage>, RepoError> {
        let (after_kind, after_id) = match &page.after {
            Some(owner) => (Some(owner.kind()), Some(owner.id())),
            None => (None, None),
        };
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT owner_kind, owner_id, role FROM asset_usages
                 WHERE asset_id = $1 AND ($2::text IS NULL OR (owner_kind, owner_id) < ($2, $3))
                 ORDER BY owner_kind DESC, owner_id DESC LIMIT $4",
                &[
                    (&asset.as_uuid(), Type::UUID),
                    (&after_kind, Type::TEXT),
                    (&after_id, Type::TEXT),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        let usages = rows
            .iter()
            .map(|row| {
                let owner = UsageOwner::new(col(row, "owner_kind")?, col(row, "owner_id")?)
                    .map_err(corrupt)?;
                let role = UsageRole::new(col(row, "role")?).map_err(corrupt)?;
                Ok(AssetUsage { asset, owner, role })
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        Ok(Page::from_overfetch(usages, page.limit))
    }
}

/// `StageAssetUsages::replace_asset_usages` inside a batch's transaction.
pub(crate) async fn replace_usages(
    tx: &impl Queries,
    owner: &UsageOwner,
    usages: &[(AssetId, UsageRole)],
) -> Result<(), RepoError> {
    tx.query_typed(
        "DELETE FROM asset_usages WHERE owner_kind = $1 AND owner_id = $2",
        &[(&owner.kind(), Type::TEXT), (&owner.id(), Type::TEXT)],
    )
    .await
    .map_err(repo_error)?;
    let assets: Vec<Uuid> = usages.iter().map(|(asset, _)| asset.as_uuid()).collect();
    let roles: Vec<&str> = usages.iter().map(|(_, role)| role.as_str()).collect();
    tx.query_typed(
        "INSERT INTO asset_usages (asset_id, owner_kind, owner_id, role)
         SELECT u.asset, $1, $2, u.role FROM unnest($3::uuid[], $4::text[]) AS u(asset, role)",
        &[
            (&owner.kind(), Type::TEXT),
            (&owner.id(), Type::TEXT),
            (&assets, Type::UUID_ARRAY),
            (&roles, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

type Param<'a> = (&'a (dyn tokio_postgres::types::ToSql + Sync), Type);

/// One asset's column values, in the types Postgres takes: what `insert` and `update`
/// write as `$1` to `$16` (`params` borrows them, so they must outlive the query).
struct RowValues {
    id: Uuid,
    status: &'static str,
    kind: &'static str,
    filename: String,
    blob_key: String,
    file_version: i32,
    mime_type: &'static str,
    byte_size: i64,
    checksum: Option<Vec<u8>>,
    width: Option<i64>,
    height: Option<i64>,
    alt: Option<String>,
    title: Option<String>,
    visibility: &'static str,
    created_at: i64,
    updated_at: i64,
}

impl RowValues {
    fn new(s: &AssetSnapshot) -> Result<Self, RepoError> {
        Ok(Self {
            id: s.id.as_uuid(),
            status: s.status.as_str(),
            kind: s.mime_type.kind().as_str(),
            filename: s.filename.clone(),
            blob_key: s.key.as_str().to_string(),
            file_version: i32::try_from(s.file_version)
                .map_err(|_| corrupt(format!("file version {}", s.file_version)))?,
            mime_type: s.mime_type.as_str(),
            byte_size: i64::try_from(s.byte_size)
                .map_err(|_| corrupt(format!("byte size {}", s.byte_size)))?,
            checksum: s.checksum.map(|c| c.as_bytes().to_vec()),
            width: s.width.map(i64::from),
            height: s.height.map(i64::from),
            alt: s.alt.clone(),
            title: s.title.clone(),
            visibility: s.visibility.as_str(),
            created_at: s.created_at.unix_micros(),
            updated_at: s.updated_at.unix_micros(),
        })
    }

    fn params(&self) -> Vec<Param<'_>> {
        vec![
            (&self.id, Type::UUID),
            (&self.status, Type::TEXT),
            (&self.kind, Type::TEXT),
            (&self.filename, Type::TEXT),
            (&self.blob_key, Type::TEXT),
            (&self.file_version, Type::INT4),
            (&self.mime_type, Type::TEXT),
            (&self.byte_size, Type::INT8),
            (&self.checksum, Type::BYTEA),
            (&self.width, Type::INT8),
            (&self.height, Type::INT8),
            (&self.alt, Type::TEXT),
            (&self.title, Type::TEXT),
            (&self.visibility, Type::TEXT),
            (&self.created_at, Type::INT8),
            (&self.updated_at, Type::INT8),
        ]
    }
}

/// An asset from a row of `columns()`.
fn asset_from_db(row: &Row) -> Result<Asset, RepoError> {
    let id: Uuid = col(row, "id")?;
    let status: String = col(row, "status")?;
    let mime_type: String = col(row, "mime_type")?;
    let visibility: String = col(row, "visibility")?;
    let checksum: Option<Vec<u8>> = col(row, "checksum")?;
    let checksum = checksum
        .map(|bytes| {
            <[u8; 32]>::try_from(bytes.as_slice())
                .map(Checksum::from_bytes)
                .map_err(|_| corrupt(format!("asset {id} checksum")))
        })
        .transpose()?;
    let dimension = |column: &str| -> Result<Option<u32>, RepoError> {
        let value: Option<i64> = col(row, column)?;
        value
            .map(|v| u32::try_from(v).map_err(|_| corrupt(format!("asset {id} dimension {v}"))))
            .transpose()
    };
    let file_version: i32 = col(row, "file_version")?;
    let byte_size: i64 = col(row, "byte_size")?;
    let snapshot = AssetSnapshot {
        id: AssetId::from_uuid(id),
        version: version_from_db(col(row, "version")?)?,
        status: status
            .parse()
            .map_err(|s| corrupt(format!("asset status {s:?}")))?,
        filename: col(row, "filename")?,
        key: BlobKey::restore(col(row, "blob_key")?),
        file_version: u32::try_from(file_version)
            .map_err(|_| corrupt(format!("asset {id} file version {file_version}")))?,
        mime_type: mime_type.parse::<MimeType>().map_err(corrupt)?,
        byte_size: u64::try_from(byte_size)
            .map_err(|_| corrupt(format!("asset {id} byte size {byte_size}")))?,
        checksum,
        width: dimension("width")?,
        height: dimension("height")?,
        alt: col(row, "alt")?,
        title: col(row, "title")?,
        visibility: visibility
            .parse()
            .map_err(|v| corrupt(format!("asset visibility {v:?}")))?,
        created_at: Timestamp::from_unix_micros(col(row, "created_at")?),
        updated_at: Timestamp::from_unix_micros(col(row, "updated_at")?),
    };
    Asset::restore(snapshot).map_err(|e| corrupt(format!("asset {id}: {e}")))
}

// ---------- temporary uploads ----------

fn upload_columns() -> String {
    format!(
        "id, purpose, filename, byte_size, checksum, {}, {}",
        micros("created_at"),
        micros("expires_at"),
    )
}

fn upload_from_db(row: &Row) -> Result<TemporaryUpload, RepoError> {
    let id: Uuid = col(row, "id")?;
    let purpose: String = col(row, "purpose")?;
    let byte_size: i64 = col(row, "byte_size")?;
    let checksum: Option<Vec<u8>> = col(row, "checksum")?;
    let checksum = checksum
        .map(|bytes| {
            <[u8; 32]>::try_from(bytes.as_slice())
                .map(Checksum::from_bytes)
                .map_err(|_| corrupt(format!("temporary upload {id}: a checksum isn't 32 bytes")))
        })
        .transpose()?;
    Ok(TemporaryUpload::restore(TemporaryUploadSnapshot {
        id: UploadId::from_uuid(id),
        purpose: purpose
            .parse::<UploadPurpose>()
            .map_err(|e| corrupt(format!("temporary upload {id}: {e}")))?,
        filename: col(row, "filename")?,
        byte_size: u64::try_from(byte_size).map_err(corrupt)?,
        checksum,
        created_at: Timestamp::from_unix_micros(col(row, "created_at")?),
        expires_at: Timestamp::from_unix_micros(col(row, "expires_at")?),
    }))
}

impl<C: PgConnection> TemporaryUploadRepo for PgStore<C> {
    async fn get(&self, id: UploadId) -> Result<Option<TemporaryUpload>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM temporary_uploads WHERE id = $1",
                    upload_columns()
                ),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(upload_from_db).transpose()
    }

    async fn insert(&self, upload: &TemporaryUpload) -> Result<(), RepoError> {
        let s = upload.snapshot();
        let byte_size = i64::try_from(s.byte_size).map_err(corrupt)?;
        let checksum = s.checksum.map(|c| c.as_bytes().to_vec());
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "INSERT INTO temporary_uploads
                         (id, purpose, filename, byte_size, checksum, created_at, expires_at)
                     VALUES ($1, $2, $3, $4, $5, {}, {})",
                    ts(6),
                    ts(7),
                ),
                &[
                    (&s.id.as_uuid(), Type::UUID),
                    (&s.purpose.as_str(), Type::TEXT),
                    (&s.filename, Type::TEXT),
                    (&byte_size, Type::INT8),
                    (&checksum, Type::BYTEA),
                    (&s.created_at.unix_micros(), Type::INT8),
                    (&s.expires_at.unix_micros(), Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }

    async fn delete(&self, id: UploadId) -> Result<bool, RepoError> {
        let mut conn = self.conn().await;
        let gone = conn
            .db()
            .query_typed_opt(
                "DELETE FROM temporary_uploads WHERE id = $1 RETURNING id",
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        Ok(gone.is_some())
    }

    async fn expired(&self, at: Timestamp, limit: u32) -> Result<Vec<TemporaryUpload>, RepoError> {
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM temporary_uploads WHERE expires_at <= {}
                     ORDER BY id LIMIT $2",
                    upload_columns(),
                    ts(1),
                ),
                &[
                    (&at.unix_micros(), Type::INT8),
                    (&i64::from(limit), Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        rows.iter().map(upload_from_db).collect()
    }
}
