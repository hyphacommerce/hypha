//! The media library's commands (design note 0013). Files go straight between the client
//! and storage; these commands sign uploads, check what was stored and keep the metadata.

use hypha_core::{
    AssetId, AuthError, Clock, Ctx, ErrorCode, IdGenerator, Keyset, Page, PageLimit, PageRequest,
    RepoError, Scope,
};
use thiserror::Error;

use crate::{
    Asset, AssetFilter, AssetKind, AssetRepo, AssetSort, AssetStatus, AssetUsage, AssetUsageRepo,
    BlobError, BlobStore, Checksum, HEAD_BYTES, HasAssetUsages, HasAssets, MediaError, MimeType,
    NewUpload, PresignedUpload, UploadMismatch, UploadTerms, UsageOwner, Visibility, repo::keys,
    settings::MediaSettings,
};

/// How many usages `AssetInUse` carries; `list_asset_usages` pages through all of them.
pub const IN_USE_LISTED: u32 = 25;

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
    /// The first `IN_USE_LISTED` usages, and whether there are more.
    #[error("asset {asset} is in use")]
    AssetInUse {
        asset: AssetId,
        usages: Vec<AssetUsage>,
        has_more: bool,
    },
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
            CommandError::AssetInUse { .. } => "asset_in_use",
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
    settings: &MediaSettings,
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
        expires_in_secs: MediaSettings::secs(settings.upload_expires),
    };
    // To the upload key: the URL stays valid for a while, so it must never be able to
    // write where the checked file is served from (`complete_upload` copies it there).
    let upload = blobs
        .presign_upload(&asset.upload_key(), &terms, now)
        .await?;
    assets.insert(&mut asset).await?;
    Ok((asset, upload))
}

/// Checks the uploaded file against what `create_upload` declared, copies it to the key
/// it is served from and makes the asset ready, with its dimensions read from the file.
/// A file that doesn't match is deleted and the asset becomes `failed`
/// (`UploadMismatch`).
///
/// Safe to retry, even while an earlier call still runs: completing a ready asset
/// returns it unchanged, and a call that loses the race to another (its write is stale,
/// or the upload was already moved) returns the asset the winner made ready.
pub async fn complete_upload<R: AssetRepo, B: BlobStore>(
    ctx: &Ctx,
    assets: &R,
    blobs: &B,
    clock: &impl Clock,
    id: AssetId,
) -> Result<Asset, CommandError> {
    ctx.require(Scope::MediaWrite)?;
    let asset = load(assets, id).await?;
    match check_and_store(assets, blobs, clock, asset).await {
        Ok(asset) => Ok(asset),
        Err(error) => match assets.get(id).await? {
            Some(asset) if asset.status() == AssetStatus::Ready => Ok(asset),
            _ => Err(error),
        },
    }
}

/// `complete_upload` on the asset as loaded.
async fn check_and_store<R: AssetRepo, B: BlobStore>(
    assets: &R,
    blobs: &B,
    clock: &impl Clock,
    mut asset: Asset,
) -> Result<Asset, CommandError> {
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
    let upload_key = asset.upload_key();
    let Some(stored) = blobs.head(&upload_key).await? else {
        return Err(CommandError::UploadMissing(asset.id()));
    };
    let head = blobs.read_range(&upload_key, 0, HEAD_BYTES).await?;
    match asset.verify_upload(stored.size, &head) {
        Ok(facts) => {
            // The checked bytes move where they are served from; a later PUT with the
            // still-valid upload URL only writes the upload key again.
            blobs.copy(&upload_key, asset.key()).await?;
            blobs.delete(&upload_key).await?;
            asset.mark_ready(facts, clock.now())?;
            assets.update(&mut asset).await?;
            Ok(asset)
        }
        Err(mismatch) => {
            blobs.delete(&upload_key).await?;
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

/// A page of the library: ready assets in `sort`'s order, narrowed to `kind` and text
/// `q` (file name, alt text, title) when given. `page.after`'s key must fit the sort
/// (`AssetSort::fits`; hosts check cursors).
pub async fn list_assets<R: AssetRepo>(
    ctx: &Ctx,
    assets: &R,
    kind: Option<AssetKind>,
    q: Option<String>,
    sort: AssetSort,
    page: PageRequest<Keyset<AssetId>>,
) -> Result<Page<Asset>, CommandError> {
    ctx.require(Scope::MediaRead)?;
    let filter = AssetFilter::new(kind, q)?;
    Ok(assets.list(&filter, sort, page).await?)
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

/// Deletes the asset and its file, unless something uses it (`AssetInUse`, design note
/// 0014, E10). The row goes first: the repository refuses it while usages exist, even
/// ones attached after the check here, and a file must never disappear from under an
/// asset that is still in use. If deleting the file then fails, it stays in storage
/// with nothing pointing at it. Records `asset.deleted` with the row.
pub async fn delete_asset<S: HasAssets + HasAssetUsages, B: BlobStore>(
    ctx: &Ctx,
    ports: &S,
    blobs: &B,
    id: AssetId,
) -> Result<(), CommandError> {
    ctx.require(Scope::MediaWrite)?;
    let mut asset = load(ports.assets(), id).await?;
    refuse_if_used(ports.asset_usages(), id).await?;
    asset.delete();
    match ports.assets().delete(&mut asset).await {
        Err(RepoError::Reference(key)) if key == keys::USAGE_ASSET => {
            // Attached since the check: report it like the check would have.
            refuse_if_used(ports.asset_usages(), id).await?;
            return Err(RepoError::Reference(key).into());
        }
        deleted => deleted?,
    }
    blobs.delete(asset.key()).await?;
    // A pending asset's file, or one uploaded again after completing.
    blobs.delete(&asset.upload_key()).await?;
    Ok(())
}

/// `AssetInUse` with the first usages, if there are any.
async fn refuse_if_used<U: AssetUsageRepo>(usages: &U, asset: AssetId) -> Result<(), CommandError> {
    let first = PageRequest {
        after: None,
        limit: PageLimit::new(IN_USE_LISTED).unwrap_or_default(),
    };
    let page = usages.list_usages(asset, first).await?;
    if page.items.is_empty() {
        return Ok(());
    }
    Err(CommandError::AssetInUse {
        asset,
        usages: page.items,
        has_more: page.has_more,
    })
}

/// Where an asset is used ("used by"), a page at a time.
pub async fn list_asset_usages<S: HasAssets + HasAssetUsages>(
    ctx: &Ctx,
    ports: &S,
    id: AssetId,
    page: PageRequest<UsageOwner>,
) -> Result<Page<AssetUsage>, CommandError> {
    ctx.require(Scope::MediaRead)?;
    load(ports.assets(), id).await?;
    Ok(ports.asset_usages().list_usages(id, page).await?)
}

/// Deletes assets whose upload was abandoned (pending for over `abandoned_after`, a day
/// by default: the tab closed, the upload failed) or that failed their check that long
/// ago, with any file under their keys. Hosts run it regularly. Returns how many it
/// deleted.
pub async fn sweep_abandoned_uploads<R: AssetRepo, B: BlobStore>(
    ctx: &Ctx,
    assets: &R,
    blobs: &B,
    clock: &impl Clock,
    settings: &MediaSettings,
) -> Result<usize, CommandError> {
    ctx.require(Scope::MediaWrite)?;
    const BATCH: u32 = 100;
    let before = clock.now().minus(settings.abandoned_after);
    let mut deleted = 0;
    loop {
        let batch = assets.abandoned(before, BATCH).await?;
        for mut asset in batch.iter().cloned() {
            // Files first, as in `delete_asset`: a failure leaves a row to retry.
            blobs.delete(&asset.upload_key()).await?;
            blobs.delete(asset.key()).await?;
            asset.delete();
            assets.delete(&mut asset).await?;
        }
        deleted += batch.len();
        if batch.len() < BATCH as usize {
            return Ok(deleted);
        }
    }
}

async fn load<R: AssetRepo>(assets: &R, id: AssetId) -> Result<Asset, CommandError> {
    assets.get(id).await?.ok_or(CommandError::AssetNotFound(id))
}
