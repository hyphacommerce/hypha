use hypha_core::{AssetId, Page, PageRequest, RepoError, Timestamp, Version, Versioned};
use hypha_media::{Asset, AssetKind, AssetRepo, AssetSnapshot, BlobKey, Checksum, MimeType};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{version_from_db, version_to_db},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore},
};

/// `assets`' columns in the order `asset_from_db` reads them.
const COLUMNS: &str = "id, version, status, filename, blob_key, file_version, mime_type, \
     byte_size, checksum, width, height, alt, title, visibility, \
     (extract(epoch FROM created_at) * 1000000)::bigint, \
     (extract(epoch FROM updated_at) * 1000000)::bigint";

// An asset is one row, so every read is one query and needs no snapshot transaction.
impl<C: PgConnection> AssetRepo for PgStore<C> {
    async fn get(&self, id: AssetId) -> Result<Option<Asset>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_opt(
                &format!("SELECT {COLUMNS} FROM assets WHERE id = $1"),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(asset_from_db).transpose()
    }

    async fn list(
        &self,
        kind: Option<AssetKind>,
        page: PageRequest<AssetId>,
    ) -> Result<Page<Asset>, RepoError> {
        let kind = kind.map(|k| k.as_str());
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let rows = conn
            .client()
            .query_typed(
                &format!(
                    "SELECT {COLUMNS} FROM assets
                     WHERE status = 'ready' AND ($1::text IS NULL OR kind = $1)
                       AND ($2::uuid IS NULL OR id < $2)
                     ORDER BY id DESC LIMIT $3"
                ),
                &[
                    (&kind, Type::TEXT),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
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

    async fn insert(&self, asset: &mut Asset) -> Result<(), RepoError> {
        let s = asset.snapshot();
        let mut conn = self.conn().await;
        conn.client()
            .query_typed(
                "INSERT INTO assets (id, version, status, kind, filename, blob_key, file_version,
                     mime_type, byte_size, checksum, width, height, alt, title, visibility,
                     created_at, updated_at)
                 VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                     'epoch'::timestamptz + $15 * interval '1 microsecond',
                     'epoch'::timestamptz + $16 * interval '1 microsecond')",
                &RowValues::new(&s)?.params(),
            )
            .await
            .map_err(repo_error)?;
        asset.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, asset: &mut Asset) -> Result<(), RepoError> {
        let s = asset.snapshot();
        let expected = version_to_db(s.version)?;
        let values = RowValues::new(&s)?;
        let mut params = values.params();
        params.push((&expected, Type::INT8));
        let mut conn = self.conn().await;
        let bumped = conn
            .client()
            .query_typed_opt(
                "UPDATE assets SET version = version + 1, status = $2, kind = $3, filename = $4,
                     blob_key = $5, file_version = $6, mime_type = $7, byte_size = $8,
                     checksum = $9, width = $10, height = $11, alt = $12, title = $13,
                     visibility = $14,
                     created_at = 'epoch'::timestamptz + $15 * interval '1 microsecond',
                     updated_at = 'epoch'::timestamptz + $16 * interval '1 microsecond'
                 WHERE id = $1 AND version = $17 RETURNING version",
                &params,
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = bumped else {
            return Err(RepoError::StaleVersion);
        };
        asset.set_version(version_from_db(row.get(0))?);
        Ok(())
    }

    async fn delete(&self, id: AssetId) -> Result<(), RepoError> {
        let mut conn = self.conn().await;
        conn.client()
            .query_typed(
                "DELETE FROM assets WHERE id = $1",
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }
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

/// An asset from a row of `COLUMNS`.
fn asset_from_db(row: &Row) -> Result<Asset, RepoError> {
    let id: Uuid = row.get(0);
    let status: String = row.get(2);
    let mime_type: String = row.get(6);
    let visibility: String = row.get(13);
    let checksum: Option<Vec<u8>> = row.get(8);
    let checksum = checksum
        .map(|bytes| {
            <[u8; 32]>::try_from(bytes.as_slice())
                .map(Checksum::from_bytes)
                .map_err(|_| corrupt(format!("asset {id} checksum")))
        })
        .transpose()?;
    let dimension = |column: usize| -> Result<Option<u32>, RepoError> {
        let value: Option<i64> = row.get(column);
        value
            .map(|v| u32::try_from(v).map_err(|_| corrupt(format!("asset {id} dimension {v}"))))
            .transpose()
    };
    let file_version: i32 = row.get(5);
    let byte_size: i64 = row.get(7);
    let snapshot = AssetSnapshot {
        id: AssetId::from_uuid(id),
        version: version_from_db(row.get(1))?,
        status: status
            .parse()
            .map_err(|s| corrupt(format!("asset status {s:?}")))?,
        filename: row.get(3),
        key: BlobKey::restore(row.get(4)),
        file_version: u32::try_from(file_version)
            .map_err(|_| corrupt(format!("asset {id} file version {file_version}")))?,
        mime_type: mime_type.parse::<MimeType>().map_err(corrupt)?,
        byte_size: u64::try_from(byte_size)
            .map_err(|_| corrupt(format!("asset {id} byte size {byte_size}")))?,
        checksum,
        width: dimension(9)?,
        height: dimension(10)?,
        alt: row.get(11),
        title: row.get(12),
        visibility: visibility
            .parse()
            .map_err(|v| corrupt(format!("asset visibility {v:?}")))?,
        created_at: Timestamp::from_unix_micros(row.get(14)),
        updated_at: Timestamp::from_unix_micros(row.get(15)),
    };
    Asset::restore(snapshot).map_err(|e| corrupt(format!("asset {id}: {e}")))
}
