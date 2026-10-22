//! Sales channels: where products are sold, and publishing products to them.

use axum::{Json, extract::State, http::StatusCode};
use hypha_channels::{
    Channel, ChannelKind, CreateChannel, UpdateChannel, create_channel, get_channel, list_channels,
    publish_products, set_product_channels, unpublish_products, update_channel,
};
use hypha_core::{ChannelId, ProductId};
use hypha_products::get_product;
use serde::{Deserialize, Serialize};

use super::stamp_times;
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

/// What a channel is for. More kinds may come: show unknown ones as `custom`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = ChannelKind)]
#[serde(rename_all = "snake_case")]
pub enum KindDto {
    OnlineStore,
    Agentic,
    Custom,
}

impl From<KindDto> for ChannelKind {
    fn from(k: KindDto) -> Self {
        match k {
            KindDto::OnlineStore => ChannelKind::OnlineStore,
            KindDto::Agentic => ChannelKind::Agentic,
            KindDto::Custom => ChannelKind::Custom,
        }
    }
}

impl From<ChannelKind> for KindDto {
    fn from(k: ChannelKind) -> Self {
        match k {
            ChannelKind::OnlineStore => KindDto::OnlineStore,
            ChannelKind::Agentic => KindDto::Agentic,
            ChannelKind::Custom => KindDto::Custom,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ChannelResponse {
    id: ChannelId,
    name: String,
    kind: KindDto,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

impl ChannelResponse {
    fn new(c: &Channel) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(hypha_core::Stamped::stamps(c))?;
        Ok(Self {
            id: c.id(),
            name: c.name().to_string(),
            kind: c.kind().into(),
            created_at,
            updated_at,
        })
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ChannelListResponse {
    /// Oldest first. A store has a handful, so they come in one list.
    items: Vec<ChannelResponse>,
}

/// Every channel, oldest first.
#[utoipa::path(
    get,
    path = "/admin/channels",
    responses((status = 200, description = "OK", body = ChannelListResponse)),
    extensions(("x-hypha-scopes" = json!(["channels:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<ChannelListResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let channels = list_channels(&ctx, &ports).await?;
    Ok(Json(ChannelListResponse {
        items: channels
            .iter()
            .map(ChannelResponse::new)
            .collect::<Result<_, _>>()?,
    }))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateChannelRequest {
    /// Up to 255 characters, unique ignoring case.
    name: String,
    kind: KindDto,
}

/// Creates a channel. It sells nothing until products are published to it.
#[utoipa::path(
    post,
    path = "/admin/channels",
    request_body = CreateChannelRequest,
    responses((status = 201, description = "Created", body = ChannelResponse)),
    extensions(("x-hypha-scopes" = json!(["channels:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateChannelRequest>,
) -> Result<(StatusCode, Json<ChannelResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateChannel {
        name: req.name,
        kind: req.kind.into(),
    };
    let channel = create_channel(&ctx, &ports, &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(ChannelResponse::new(&channel)?)))
}

/// A channel.
#[utoipa::path(
    get,
    path = "/admin/channels/{id}",
    params(("id" = ChannelId, Path)),
    responses((status = 200, description = "OK", body = ChannelResponse)),
    extensions(("x-hypha-scopes" = json!(["channels:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ChannelId>,
) -> Result<Json<ChannelResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let channel = get_channel(&ctx, &ports, id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("channel_not_found"))?;
    Ok(Json(ChannelResponse::new(&channel)?))
}

/// A JSON Merge Patch of a channel.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateChannelRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
}

/// Renames a channel. Its kind can't change.
#[utoipa::path(
    patch,
    path = "/admin/channels/{id}",
    params(("id" = ChannelId, Path)),
    request_body = UpdateChannelRequest,
    responses((status = 200, description = "OK", body = ChannelResponse)),
    extensions(("x-hypha-scopes" = json!(["channels:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ChannelId>,
    body: JsonBody<UpdateChannelRequest>,
) -> Result<Json<ChannelResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let channel = update_channel(&ctx, &ports, id, UpdateChannel { name: req.name })
        .await
        .map_err(|e| ApiError::from(e).not_found_if("channel_not_found"))?;
    Ok(Json(ChannelResponse::new(&channel)?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelProductsRequest {
    /// Product IDs. They aren't checked: an unknown one is stored and never matches.
    products: Vec<ProductId>,
}

/// Publishes products to the channel; ones already published are left as they are.
#[utoipa::path(
    post,
    path = "/admin/channels/{id}/products",
    params(("id" = ChannelId, Path)),
    request_body = ChannelProductsRequest,
    responses((status = 204, description = "Published")),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn publish<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ChannelId>,
    body: JsonBody<ChannelProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    publish_products(&ctx, &ports, id, req.products)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("channel_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Unpublishes products from the channel; ones not published are left as they are.
#[utoipa::path(
    delete,
    path = "/admin/channels/{id}/products",
    params(("id" = ChannelId, Path)),
    request_body = ChannelProductsRequest,
    responses((status = 204, description = "Unpublished")),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn unpublish<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ChannelId>,
    body: JsonBody<ChannelProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    unpublish_products(&ctx, &ports, id, req.products)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("channel_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductChannelsRequest {
    /// Every channel the product is sold through; the others stop selling it.
    channels: Vec<ChannelId>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ProductChannelsResponse {
    /// Sorted.
    channels: Vec<ChannelId>,
}

/// Sets the channels a product is sold through (the product page's Publishing card).
#[utoipa::path(
    put,
    path = "/admin/products/{id}/channels",
    params(("id" = ProductId, Path)),
    request_body = ProductChannelsRequest,
    responses((status = 200, description = "OK", body = ProductChannelsResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn set_for_product<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    body: JsonBody<ProductChannelsRequest>,
) -> Result<Json<ProductChannelsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    // The product is in the URL: a 404 when it doesn't exist.
    get_product(&ctx, hypha_products::HasProducts::products(&ports), id).await?;
    let channels = set_product_channels(&ctx, &ports, id, req.channels).await?;
    Ok(Json(ProductChannelsResponse { channels }))
}
