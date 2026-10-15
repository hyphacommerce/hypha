//! The media library (design note 0013): assets, their uploads and their URLs. Files go
//! straight between the client and storage; these routes sign and check.

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::AssetId;
use hypha_media::{
    Asset, AssetKind, AssetStatus, CreateUpload, HasAssets, UpdateAsset, Visibility,
    complete_upload, create_upload, delete_asset, get_asset, list_assets, update_asset,
};
use serde::{Deserialize, Serialize};

use super::{
    page::{ListQuery, PageParams, PageResponse},
    rfc3339,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    media::Media,
    patch,
    state::{SharedState, Storage},
};

/// `AssetKind` on the wire.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum AssetKindDto {
    Image,
    Video,
    File,
}

impl From<AssetKind> for AssetKindDto {
    fn from(k: AssetKind) -> Self {
        match k {
            AssetKind::Image => AssetKindDto::Image,
            AssetKind::Video => AssetKindDto::Video,
            AssetKind::File => AssetKindDto::File,
        }
    }
}

impl From<AssetKindDto> for AssetKind {
    fn from(k: AssetKindDto) -> Self {
        match k {
            AssetKindDto::Image => AssetKind::Image,
            AssetKindDto::Video => AssetKind::Video,
            AssetKindDto::File => AssetKind::File,
        }
    }
}

/// `AssetStatus` on the wire.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum AssetStatusDto {
    Pending,
    Ready,
    Failed,
}

impl From<AssetStatus> for AssetStatusDto {
    fn from(s: AssetStatus) -> Self {
        match s {
            AssetStatus::Pending => AssetStatusDto::Pending,
            AssetStatus::Ready => AssetStatusDto::Ready,
            AssetStatus::Failed => AssetStatusDto::Failed,
        }
    }
}

/// `Visibility` on the wire.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum VisibilityDto {
    /// Served from the public media URL (a CDN).
    #[default]
    Public,
    /// Only through short-lived signed URLs.
    Private,
}

impl From<Visibility> for VisibilityDto {
    fn from(v: Visibility) -> Self {
        match v {
            Visibility::Public => VisibilityDto::Public,
            Visibility::Private => VisibilityDto::Private,
        }
    }
}

impl From<VisibilityDto> for Visibility {
    fn from(v: VisibilityDto) -> Self {
        match v {
            VisibilityDto::Public => Visibility::Public,
            VisibilityDto::Private => Visibility::Private,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct AssetResponse {
    id: AssetId,
    /// Only `ready` assets are listed and can be attached.
    status: AssetStatusDto,
    kind: AssetKindDto,
    /// As uploaded, for people.
    filename: String,
    mime_type: String,
    byte_size: u64,
    /// SHA-256 as hex, when the uploader declared it.
    checksum: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    alt: Option<String>,
    title: Option<String>,
    visibility: VisibilityDto,
    /// The file, once ready. A public file's URL never changes content; a private
    /// file's is signed and works for 15 minutes.
    url: Option<String>,
    /// Images only: a version at most 400 pixels wide when the media URLs can resize,
    /// else the original.
    thumbnail_url: Option<String>,
    /// RFC 3339, UTC.
    created_at: String,
    /// RFC 3339, UTC.
    updated_at: String,
}

impl AssetResponse {
    async fn new(asset: &Asset, media: &Media) -> Result<Self, ApiError> {
        Ok(Self {
            id: asset.id(),
            status: asset.status().into(),
            kind: asset.kind().into(),
            filename: asset.filename().to_string(),
            mime_type: asset.mime_type().to_string(),
            byte_size: asset.byte_size(),
            checksum: asset.checksum().map(|c| c.to_string()),
            width: asset.width(),
            height: asset.height(),
            alt: asset.alt().map(str::to_string),
            title: asset.title().map(str::to_string),
            visibility: asset.visibility().into(),
            url: media.url(asset).await?,
            thumbnail_url: media.thumbnail_url(asset).await?,
            created_at: rfc3339(asset.created_at())?,
            updated_at: rfc3339(asset.updated_at())?,
        })
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateAssetRequest {
    /// The file's name, kept for people; up to 255 characters.
    filename: String,
    /// One of `image/jpeg`, `image/png`, `image/webp`, `image/avif`, `image/gif`,
    /// `video/mp4`, `video/webm`, `video/quicktime`, `application/pdf` (no SVG yet).
    mime_type: String,
    /// Exactly the file's size: storage refuses another. Images up to 20 MB, videos up
    /// to 1 GB, PDFs up to 100 MB.
    byte_size: u64,
    /// SHA-256 as 64 hex characters: storage then refuses other contents.
    #[serde(default)]
    checksum: Option<String>,
    #[serde(default)]
    visibility: VisibilityDto,
}

/// A request for the client to send itself, with the file as its body.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct UploadRequestResponse {
    /// `PUT`.
    method: &'static str,
    url: String,
    /// Send exactly these, besides `Content-Length`.
    headers: Vec<HeaderResponse>,
    /// RFC 3339, UTC: the URL stops working then.
    expires_at: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct HeaderResponse {
    name: String,
    value: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct CreateAssetResponse {
    asset: AssetResponse,
    upload: UploadRequestResponse,
}

/// The `Media` or `503 media_not_configured`.
fn media<S>(state: &SharedState<S>) -> Result<&Media, ApiError> {
    state
        .media
        .as_ref()
        .ok_or_else(ApiError::media_not_configured)
}

/// Starts an upload: a pending asset, and the request that sends its file straight to
/// storage. Then `POST /admin/assets/{id}/complete`.
#[utoipa::path(
    post,
    path = "/admin/assets",
    request_body = CreateAssetRequest,
    responses((status = 201, description = "Created", body = CreateAssetResponse)),
    extensions(("x-hypha-scopes" = json!(["media:write"]))),
)]
pub(super) async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateAssetRequest>,
) -> Result<(StatusCode, Json<CreateAssetResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let media = media(&state)?;
    let input = CreateUpload {
        filename: req.filename,
        mime_type: req.mime_type,
        byte_size: req.byte_size,
        checksum: req.checksum,
        visibility: req.visibility.into(),
    };
    let (asset, upload) = create_upload(
        &ctx,
        ports.assets(),
        &media.blobs,
        &state.ids,
        &state.clock,
        input,
    )
    .await?;
    let response = CreateAssetResponse {
        asset: AssetResponse::new(&asset, media).await?,
        upload: UploadRequestResponse {
            method: upload.method,
            url: upload.url,
            headers: upload
                .headers
                .into_iter()
                .map(|(name, value)| HeaderResponse { name, value })
                .collect(),
            expires_at: rfc3339(upload.expires_at)?,
        },
    };
    Ok((StatusCode::CREATED, Json(response)))
}

/// Checks the uploaded file and makes the asset ready, with its real type and image
/// dimensions read from the file. A file that isn't what was declared is deleted and
/// the asset fails (`422 upload_mismatch`). Completing a ready asset again is fine.
#[utoipa::path(
    post,
    path = "/admin/assets/{id}/complete",
    params(("id" = AssetId, Path)),
    responses((status = 200, description = "OK", body = AssetResponse)),
    extensions(("x-hypha-scopes" = json!(["media:write"]))),
)]
pub(super) async fn complete<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<AssetId>,
) -> Result<Json<AssetResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let media = media(&state)?;
    let asset = complete_upload(&ctx, ports.assets(), &media.blobs, &state.clock, id).await?;
    Ok(Json(AssetResponse::new(&asset, media).await?))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(super) struct AssetFilters {
    /// Only assets of this kind.
    kind: Option<AssetKindDto>,
}

/// A page of assets (named for the documents: utoipa's `body =` takes a plain name).
type AssetPage = PageResponse<AssetResponse>;

/// The library: ready assets, newest first, optionally of one kind.
#[utoipa::path(
    get,
    path = "/admin/assets",
    params(PageParams, AssetFilters),
    responses((status = 200, description = "OK", body = AssetPage)),
    extensions(("x-hypha-scopes" = json!(["media:read"]))),
)]
pub(super) async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<AssetFilters>,
) -> Result<Json<AssetPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters) = query.parse()?;
    let media = media(&state)?;
    let page = list_assets(&ctx, ports.assets(), filters.kind.map(Into::into), page).await?;
    let mut items = Vec::with_capacity(page.items.len());
    for asset in &page.items {
        items.push(AssetResponse::new(asset, media).await?);
    }
    Ok(Json(PageResponse::with_items(&page, Asset::id, items)))
}

/// An asset, in any status.
#[utoipa::path(
    get,
    path = "/admin/assets/{id}",
    params(("id" = AssetId, Path)),
    responses((status = 200, description = "OK", body = AssetResponse)),
    extensions(("x-hypha-scopes" = json!(["media:read"]))),
)]
pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<AssetId>,
) -> Result<Json<AssetResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let media = media(&state)?;
    let asset = get_asset(&ctx, ports.assets(), id).await?;
    Ok(Json(AssetResponse::new(&asset, media).await?))
}

/// A JSON Merge Patch of an asset's metadata: absent fields stay, `null` clears.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateAssetRequest {
    /// Describes the image for screen readers and search engines; up to 255 characters.
    #[serde(default, deserialize_with = "patch::nullable")]
    alt: Option<Option<String>>,
    /// The library's label; up to 255 characters.
    #[serde(default, deserialize_with = "patch::nullable")]
    title: Option<Option<String>>,
}

/// Changes an asset's alt text or title.
#[utoipa::path(
    patch,
    path = "/admin/assets/{id}",
    params(("id" = AssetId, Path)),
    request_body = UpdateAssetRequest,
    responses((status = 200, description = "OK", body = AssetResponse)),
    extensions(("x-hypha-scopes" = json!(["media:write"]))),
)]
pub(super) async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<AssetId>,
    body: JsonBody<UpdateAssetRequest>,
) -> Result<Json<AssetResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let media = media(&state)?;
    let input = UpdateAsset {
        alt: req.alt,
        title: req.title,
    };
    let asset = update_asset(&ctx, ports.assets(), &state.clock, id, input).await?;
    Ok(Json(AssetResponse::new(&asset, media).await?))
}

/// Deletes an asset and its file.
#[utoipa::path(
    delete,
    path = "/admin/assets/{id}",
    params(("id" = AssetId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["media:write"]))),
)]
pub(super) async fn remove<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<AssetId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let media = media(&state)?;
    delete_asset(&ctx, ports.assets(), &media.blobs, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
