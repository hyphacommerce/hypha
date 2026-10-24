//! Temporary uploads: a file the admin's browser uploads straight to storage for its server
//! to read back (a CSV of prices to import), never through a form post to Hypha. Kept for
//! a day, then swept with the media.

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::Response,
};
use hypha_core::UploadId;
use hypha_media::{
    CreateTemporaryUpload, HasTemporaryUploads, TemporaryUpload, UploadPurpose,
    create_temporary_upload, delete_temporary_upload, read_temporary_upload,
};
use serde::{Deserialize, Serialize};

use super::{
    assets::{UploadRequestResponse, media},
    rfc3339,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    state::{SharedState, Storage},
};

/// What an upload is for, which decides who may make and read it, its file's type and its
/// largest size: `price_import` (`pricing:write`, `text/csv`, up to 20 MB).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UploadPurposeDto {
    PriceImport,
}

impl From<UploadPurpose> for UploadPurposeDto {
    fn from(p: UploadPurpose) -> Self {
        match p {
            UploadPurpose::PriceImport => UploadPurposeDto::PriceImport,
        }
    }
}

impl UploadPurposeDto {
    fn as_str(self) -> &'static str {
        match self {
            UploadPurposeDto::PriceImport => UploadPurpose::PriceImport.as_str(),
        }
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateUploadRequest {
    purpose: UploadPurposeDto,
    /// The file's name, kept for people; up to 255 characters.
    filename: String,
    /// Exactly the file's size: storage refuses another.
    byte_size: u64,
    /// SHA-256 as 64 hex characters: storage then refuses other contents.
    #[serde(default)]
    checksum: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct UploadResponse {
    id: UploadId,
    purpose: UploadPurposeDto,
    filename: String,
    byte_size: u64,
    /// RFC 3339, UTC: the upload and its file are deleted after then.
    expires_at: String,
    /// The request that sends the file straight to storage, with the file as its body
    /// (the purpose's type as `Content-Type`).
    upload: UploadRequestResponse,
}

/// Starts a temporary upload: the request that sends a file straight to storage, to read
/// back with `GET /admin/uploads/{id}/content` until `expires_at`. The scope is the
/// purpose's: `pricing:write` for `price_import`.
#[utoipa::path(
    post,
    path = "/admin/uploads",
    request_body = CreateUploadRequest,
    responses((status = 201, description = "Created", body = UploadResponse)),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateUploadRequest>,
) -> Result<(StatusCode, Json<UploadResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let media = media(&state)?;
    let (upload, request) = create_temporary_upload(
        &ctx,
        ports.temporary_uploads(),
        &media.blobs,
        &state.ids,
        &state.clock,
        &media.settings,
        CreateTemporaryUpload {
            purpose: req.purpose.as_str().to_string(),
            filename: req.filename,
            byte_size: req.byte_size,
            checksum: req.checksum,
        },
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(UploadResponse::new(
            &upload,
            UploadRequestResponse::new(request)?,
        )?),
    ))
}

impl UploadResponse {
    fn new(upload: &TemporaryUpload, request: UploadRequestResponse) -> Result<Self, ApiError> {
        Ok(Self {
            id: upload.id(),
            purpose: upload.purpose().into(),
            filename: upload.filename().to_string(),
            byte_size: upload.byte_size(),
            expires_at: rfc3339(upload.expires_at())?,
            upload: request,
        })
    }
}

/// The uploaded file's bytes, with the purpose's type. `404 upload_not_found` once it
/// expired, `409 upload_missing` until the file has been uploaded. The scope is the
/// purpose's.
#[utoipa::path(
    get,
    path = "/admin/uploads/{id}/content",
    params(("id" = UploadId, Path)),
    responses((status = 200, description = "The file", content_type = "text/csv", body = String)),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn content<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<UploadId>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let media = media(&state)?;
    let (upload, bytes) = read_temporary_upload(
        &ctx,
        ports.temporary_uploads(),
        &media.blobs,
        &state.clock,
        id,
    )
    .await?;
    Response::builder()
        .header(header::CONTENT_TYPE, upload.purpose().content_type())
        .header(header::CACHE_CONTROL, "private, no-store")
        .body(Body::from(bytes))
        .map_err(ApiError::internal)
}

/// Deletes the upload and its file before it expires. The scope is the purpose's.
#[utoipa::path(
    delete,
    path = "/admin/uploads/{id}",
    params(("id" = UploadId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn remove<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<UploadId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let media = media(&state)?;
    delete_temporary_upload(
        &ctx,
        ports.temporary_uploads(),
        &media.blobs,
        &state.clock,
        id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
