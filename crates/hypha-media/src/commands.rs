//! The media library's commands (design note 0013). Files go straight between the client
//! and storage; these commands sign uploads, check what was stored and keep the metadata.

use hypha_core::{
    AssetId, AuthError, Clock, Ctx, ErrorCode, IdGenerator, Page, PageRequest, RepoError, Scope,
};
use thiserror::Error;

use crate::{
    Asset, AssetKind, AssetRepo, AssetStatus, BlobError, BlobStore, Checksum, HEAD_BYTES,
    MediaError, MimeType, NewUpload, PresignedUpload, UploadMismatch, UploadTerms, Visibility,
};

/// How long an upload URL works.
pub const UPLOAD_EXPIRES_IN_SECS: u32 = 15 * 60;

/// How long a private file's download URL works.
pub const DOWNLOAD_EXPIRES_IN_SECS: u32 = 15 * 60;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Blob(#[from] BlobError),
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error("asset {0} not found")]
    AssetNotFound(AssetId),
    /// Completing found nothing stored: the client hasn't uploaded (yet).
    #[error("nothing has been uploaded for asset {0}")]
    UploadMissing(AssetId),
    /// The stored file wasn't what was declared; the asset is now `failed`.
    #[error(transparent)]
    UploadMismatch(#[from] UploadMismatch),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Blob(e) => e.code(),
            CommandError::Media(e) => e.code(),
            CommandError::AssetNotFound(_) => "asset_not_found",
            CommandError::UploadMissing(_) => "upload_missing",
            CommandError::UploadMismatch(e) => e.code(),
        }
    }
}

/// A file a client is about to upload.
pub struct CreateUpload {
    pub filename: String,
    pub mime_type: String,
    pub byte_size: u64,
    /// SHA-256 as 64 hex characters: storage then refuses an upload with other contents.
    pub checksum: Option<String>,
    pub visibility: Visibility,
}

/// Creates a pending asset and the request that uploads its file straight to storage.
/// `complete_upload` makes it ready once the file is there.
pub async fn create_upload<R: AssetRepo, B: BlobStore>(
    ctx: &Ctx,
    assets: &R,
    blobs: &B,
    ids: &impl IdGenerator,
    clock: &impl Clock,
    input: CreateUpload,
) -> Result<(Asset, PresignedUpload), CommandError> {
    ctx.require(Scope::MediaWrite)?;
    let mime_type: MimeType = input.mime_type.parse()?;
    let checksum = input
        .checksum
        .as_deref()
        .map(Checksum::from_hex)
        .transpose()?;
    let now = clock.now();
    let mut asset = Asset::new_upload(NewUpload {
        id: AssetId::from_uuid(ids.new_uuid()),
        filename: input.filename,
        mime_type,
        byte_size: input.byte_size,
        checksum,
        visibility: input.visibility,
        now,
    })?;
    let terms = UploadTerms {
        content_type: mime_type,
        content_length: asset.byte_size(),
        checksum,
        cache_control: asset.visibility().cache_control(),
        expires_in_secs: UPLOAD_EXPIRES_IN_SECS,
    };
    let upload = blobs.presign_upload(asset.key(), &terms, now).await?;
    assets.insert(&mut asset).await?;
    Ok((asset, upload))
}

/// Checks the uploaded file against what `create_upload` declared and makes the asset
/// ready, with its dimensions read from the file. A file that doesn't match is deleted
/// and the asset becomes `failed` (`UploadMismatch`). Completing a ready asset again
/// returns it unchanged, so a retry is safe.
pub async fn complete_upload<R: AssetRepo, B: BlobStore>(
    ctx: &Ctx,
    assets: &R,
    blobs: &B,
    clock: &impl Clock,
    id: AssetId,
) -> Result<Asset, CommandError> {
    ctx.require(Scope::MediaWrite)?;
    let mut asset = load(assets, id).await?;
    match asset.status() {
        AssetStatus::Ready => return Ok(asset),
        AssetStatus::Failed => {
            return Err(MediaError::NotPending {
                status: AssetStatus::Failed,
            }
            .into());
        }
        AssetStatus::Pending => {}
    }
    let Some(stored) = blobs.head(asset.key()).await? else {
        return Err(CommandError::UploadMissing(id));
    };
    let head = blobs.read_range(asset.key(), 0, HEAD_BYTES).await?;
    match asset.verify_upload(stored.size, &head) {
        Ok(facts) => {
            asset.mark_ready(facts, clock.now())?;
            assets.update(&mut asset).await?;
            Ok(asset)
        }
        Err(mismatch) => {
            blobs.delete(asset.key()).await?;
            asset.mark_failed(clock.now())?;
            assets.update(&mut asset).await?;
            Err(mismatch.into())
        }
    }
}

pub async fn get_asset<R: AssetRepo>(
    ctx: &Ctx,
    assets: &R,
    id: AssetId,
) -> Result<Asset, CommandError> {
    ctx.require(Scope::MediaRead)?;
    load(assets, id).await
}

/// A page of the library: ready assets, newest first, only those of `kind` when given.
pub async fn list_assets<R: AssetRepo>(
    ctx: &Ctx,
    assets: &R,
    kind: Option<AssetKind>,
    page: PageRequest<AssetId>,
) -> Result<Page<Asset>, CommandError> {
    ctx.require(Scope::MediaRead)?;
    Ok(assets.list(kind, page).await?)
}

/// Changes only the fields that are `Some`, like a JSON Merge Patch: `Some(None)` (or a
/// blank string) clears one.
#[derive(Default)]
pub struct UpdateAsset {
    pub alt: Option<Option<String>>,
    pub title: Option<Option<String>>,
}

pub async fn update_asset<R: AssetRepo>(
    ctx: &Ctx,
    assets: &R,
    clock: &impl Clock,
    id: AssetId,
    input: UpdateAsset,
) -> Result<Asset, CommandError> {
    ctx.require(Scope::MediaWrite)?;
    let mut asset = load(assets, id).await?;
    if input.alt.is_none() && input.title.is_none() {
        return Ok(asset);
    }
    let now = clock.now();
    if let Some(alt) = input.alt {
        asset.set_alt(alt, now)?;
    }
    if let Some(title) = input.title {
        asset.set_title(title, now)?;
    }
    assets.update(&mut asset).await?;
    Ok(asset)
}

/// Deletes the asset and its file. The file goes first, so a failure halfway leaves an
/// asset whose delete can simply be retried, never a file nothing points at.
///
/// Assets in use can't be deleted once usages exist (design note 0013, phase 2).
pub async fn delete_asset<R: AssetRepo, B: BlobStore>(
    ctx: &Ctx,
    assets: &R,
    blobs: &B,
    id: AssetId,
) -> Result<(), CommandError> {
    ctx.require(Scope::MediaWrite)?;
    let asset = load(assets, id).await?;
    blobs.delete(asset.key()).await?;
    assets.delete(id).await?;
    Ok(())
}

async fn load<R: AssetRepo>(assets: &R, id: AssetId) -> Result<Asset, CommandError> {
    assets.get(id).await?.ok_or(CommandError::AssetNotFound(id))
}
