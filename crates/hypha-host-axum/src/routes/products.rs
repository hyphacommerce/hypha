use hypha_core::Stamped;

use super::stamp_times;
use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{AssetId, Money, OptionId, OptionValueId, ProductId, VariantId, Weight};
use hypha_media::{Asset, HasAssets};
use hypha_merchandising::{
    ProductSummaryView, ProductView, get_product_view, list_product_views, set_product_media,
    set_variant_media,
};
use hypha_products::{
    AddVariant, CreateSimpleProduct, HasProducts, ListProducts, OptionDraft, OptionValueDraft,
    Product, ProductStatus, SetProductOptions, UpdateProduct, UpdateVariant, add_product_variant,
    create_simple_product, remove_product_variant, set_product_options, set_product_status,
    update_product, update_product_variant,
};
use serde::{Deserialize, Serialize};

use super::{
    assets::AssetKindDto,
    page::{ListQuery, PageParams, PageResponse},
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId, Storefront},
    media::Media,
    patch,
    state::{SharedState, Storage},
};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateProductRequest {
    handle: String,
    title: String,
    price: Money,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SetStatusRequest {
    status: StatusDto,
}

/// `ProductStatus` on the wire: snake_case strings. A DTO of its own, so the domain
/// enum carries no serde and renaming a variant can't silently change the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = ProductStatus)]
#[serde(rename_all = "snake_case")]
pub(super) enum StatusDto {
    Draft,
    Active,
    Archived,
}

impl From<StatusDto> for ProductStatus {
    fn from(s: StatusDto) -> Self {
        match s {
            StatusDto::Draft => ProductStatus::Draft,
            StatusDto::Active => ProductStatus::Active,
            StatusDto::Archived => ProductStatus::Archived,
        }
    }
}

impl From<ProductStatus> for StatusDto {
    fn from(s: ProductStatus) -> Self {
        match s {
            ProductStatus::Draft => StatusDto::Draft,
            ProductStatus::Active => StatusDto::Active,
            ProductStatus::Archived => StatusDto::Archived,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct ProductResponse {
    id: ProductId,
    handle: String,
    title: String,
    status: StatusDto,
    /// Sanitised HTML (scripts, styles and event attributes removed), safe to render.
    description: Option<String>,
    vendor: Option<String>,
    product_type: Option<String>,
    /// In the merchant's order.
    tags: Vec<String>,
    /// What variants differ by (Size, Color), in display order; empty for a product
    /// with a single variant.
    options: Vec<OptionResponse>,
    variants: Vec<VariantResponse>,
    /// In order; the first is the featured item. At most 250.
    media: Vec<ProductMediaResponse>,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

/// One item of a product's media list (design note 0014, section 7): what a page needs
/// to render it. The asset's own route has the rest.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct ProductMediaResponse {
    asset_id: AssetId,
    /// `image` or `video` today. More kinds (3D models) will come: show the ones you
    /// know and skip the others.
    kind: AssetKindDto,
    mime_type: String,
    /// The file. Public, so it doesn't expire when served from the media URL; without
    /// one (local development) it is signed and works for `media.download_expires` (15
    /// minutes by default).
    url: String,
    /// Images: a version at most 400 pixels wide when the media URLs can resize, else
    /// the original. `null` for videos.
    thumbnail_url: Option<String>,
    /// The asset's alt text.
    alt: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

impl ProductMediaResponse {
    async fn new(asset: &Asset, media: &Media) -> Result<Self, ApiError> {
        Ok(Self {
            asset_id: asset.id(),
            kind: asset.kind().into(),
            mime_type: asset.mime_type().to_string(),
            // Only ready assets can be attached, so there is always a file.
            url: media.url(asset).await?.ok_or_else(|| {
                ApiError::internal(format!("attached asset {} has no file", asset.id()))
            })?,
            thumbnail_url: media.thumbnail_url(asset).await?,
            alt: asset.alt().map(str::to_string),
            width: asset.width(),
            height: asset.height(),
        })
    }
}

/// The items for `assets`, in order. A deployment without file storage can still serve
/// products without media; one with media needs it (`503 media_not_configured`).
async fn media_items(
    assets: &[Asset],
    media: Option<&Media>,
) -> Result<Vec<ProductMediaResponse>, ApiError> {
    if assets.is_empty() {
        return Ok(Vec::new());
    }
    let media = media.ok_or_else(ApiError::media_not_configured)?;
    let mut items = Vec::with_capacity(assets.len());
    for asset in assets {
        items.push(ProductMediaResponse::new(asset, media).await?);
    }
    Ok(items)
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct OptionResponse {
    id: OptionId,
    name: String,
    values: Vec<OptionValueResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct OptionValueResponse {
    id: OptionValueId,
    label: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct VariantResponse {
    id: VariantId,
    /// The base price, before markets and price lists.
    price: Money,
    sku: Option<String>,
    /// Its option values ("M / Blue"); `null` for a product without options.
    title: Option<String>,
    /// One value per option, in the order of `options`.
    selections: Vec<OptionValueId>,
    /// The item of the product's `media` this variant shows (an image), if any.
    media_asset_id: Option<AssetId>,
    /// `null` when not entered; carriers and weight-tiered rates need it.
    weight_grams: Option<u32>,
    /// `false` for what is never shipped (downloads, gift cards).
    requires_shipping: bool,
}

impl ProductResponse {
    async fn new(view: &ProductView, media: Option<&Media>) -> Result<Self, ApiError> {
        let p = &view.product;
        let (created_at, updated_at) = stamp_times(p.stamps())?;
        Ok(Self {
            created_at,
            updated_at,
            id: p.id(),
            handle: p.handle().as_str().to_string(),
            title: p.title().to_string(),
            status: p.status().into(),
            description: p.description().map(|d| d.as_html().to_string()),
            vendor: p.vendor().map(str::to_string),
            product_type: p.product_type().map(str::to_string),
            tags: p.tags().to_vec(),
            options: p
                .options()
                .iter()
                .map(|o| OptionResponse {
                    id: o.id(),
                    name: o.name().to_string(),
                    values: o
                        .values()
                        .iter()
                        .map(|v| OptionValueResponse {
                            id: v.id(),
                            label: v.label().to_string(),
                        })
                        .collect(),
                })
                .collect(),
            variants: p
                .variants()
                .iter()
                .map(|v| VariantResponse {
                    id: v.id(),
                    price: v.base_price().money(),
                    sku: v.sku().map(str::to_string),
                    title: p.variant_title(v.id()),
                    selections: v.selections().to_vec(),
                    media_asset_id: v.media(),
                    weight_grams: v.weight().map(Weight::grams),
                    requires_shipping: v.requires_shipping(),
                })
                .collect(),
            media: media_items(&view.media, media).await?,
        })
    }
}

/// The response for a product a command just returned, with its media's assets.
async fn respond<S: Storage>(
    state: &SharedState<S>,
    ports: &S::Ports<'_>,
    product: Product,
) -> Result<Json<ProductResponse>, ApiError> {
    let view = ProductView::load(ports.assets(), product).await?;
    Ok(Json(
        ProductResponse::new(&view, state.media.as_ref()).await?,
    ))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(super) struct ProductFilters {
    /// Only products in this status.
    status: Option<StatusDto>,
    /// Only products whose title, handle or a variant's SKU contains this text, ignoring
    /// case. Up to 255 characters; blank filters nothing. Reads the products as they are
    /// now (drafts too); storefronts search with `/storefront/search`.
    q: Option<String>,
}

/// A product in a list: what a table of products shows, without options or variants
/// (design note 0012, L7). `GET /admin/products/{id}` has the rest.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct ProductSummaryResponse {
    id: ProductId,
    handle: String,
    title: String,
    status: StatusDto,
    vendor: Option<String>,
    product_type: Option<String>,
    variant_count: u64,
    /// The lowest and highest variant price in each currency the variants are priced
    /// in, ordered by currency code: usually one.
    price_ranges: Vec<PriceRangeResponse>,
    /// The first item of the product's media, if it has any.
    featured_media: Option<ProductMediaResponse>,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct PriceRangeResponse {
    min: Money,
    /// Equal to `min` when every variant in this currency costs the same.
    max: Money,
}

impl ProductSummaryResponse {
    async fn new(view: &ProductSummaryView, media: Option<&Media>) -> Result<Self, ApiError> {
        let featured_media = match &view.featured_media {
            Some(asset) => media_items(std::slice::from_ref(asset), media).await?.pop(),
            None => None,
        };
        let p = &view.summary;
        let (created_at, updated_at) = stamp_times(p.stamps)?;
        Ok(Self {
            created_at,
            updated_at,
            id: p.id,
            handle: p.handle.as_str().to_string(),
            title: p.title.clone(),
            status: p.status.into(),
            vendor: p.vendor.clone(),
            product_type: p.product_type.clone(),
            variant_count: p.variant_count as u64,
            price_ranges: p
                .price_ranges
                .iter()
                .map(|r| PriceRangeResponse {
                    min: r.min.money(),
                    max: r.max.money(),
                })
                .collect(),
            featured_media,
        })
    }
}

/// A page of products (named for the documents: utoipa's `body =` takes a plain name).
type ProductSummaryPage = PageResponse<ProductSummaryResponse>;

/// Products in any status, newest first, optionally of one status.
#[utoipa::path(
    get,
    path = "/admin/products",
    params(PageParams, ProductFilters),
    responses((status = 200, description = "OK", body = ProductSummaryPage)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub(super) async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<ProductFilters>,
) -> Result<Json<ProductSummaryPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse()?;
    let input = ListProducts {
        status: filters.status.map(Into::into),
        q: filters.q,
    };
    let page = list_product_views(&ctx, &ports, input, page).await?;
    let mut items = Vec::with_capacity(page.items.len());
    for view in &page.items {
        items.push(ProductSummaryResponse::new(view, state.media.as_ref()).await?);
    }
    Ok(Json(PageResponse::with_items(
        &page,
        &scope,
        |v| v.summary.id,
        items,
    )?))
}

/// Creates a product with one variant, as a draft.
#[utoipa::path(
    post,
    path = "/admin/products",
    request_body = CreateProductRequest,
    responses((status = 201, description = "Created", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateProductRequest>,
) -> Result<(StatusCode, Json<ProductResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateSimpleProduct {
        handle: req.handle,
        title: req.title,
        price: req.price,
    };
    let product = create_simple_product(&ctx, ports.products(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, respond(&state, &ports, product).await?))
}

/// A product, in any status.
#[utoipa::path(
    get,
    path = "/admin/products/{id}",
    params(("id" = ProductId, Path)),
    responses((status = 200, description = "OK", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let view = get_product_view(&ctx, &ports, id).await?;
    Ok(Json(
        ProductResponse::new(&view, state.media.as_ref()).await?,
    ))
}

/// A product as a storefront sees it (design note 0010, S5): active products only, without
/// admin fields. A draft or archived product is `404 product_not_found`, the same as one
/// that doesn't exist, so a storefront can't tell them apart.
///
/// No prices: a product's base price isn't what a shopper pays (their market's price
/// list and currency decide that), so showing it would mislead. Until product listing
/// brings prices per market, the cart is where a storefront gets them.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct StorefrontProductResponse {
    id: ProductId,
    handle: String,
    title: String,
    /// Sanitised HTML (scripts, styles and event attributes removed), safe to render.
    description: Option<String>,
    vendor: Option<String>,
    product_type: Option<String>,
    /// In the merchant's order.
    tags: Vec<String>,
    variants: Vec<StorefrontVariantResponse>,
    /// In order; the first is the featured item.
    media: Vec<ProductMediaResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct StorefrontVariantResponse {
    id: VariantId,
    /// The item of the product's `media` this variant shows (an image), if any.
    media_asset_id: Option<AssetId>,
}

impl StorefrontProductResponse {
    async fn new(view: &ProductView, media: Option<&Media>) -> Result<Self, ApiError> {
        let p = &view.product;
        Ok(Self {
            id: p.id(),
            handle: p.handle().as_str().to_string(),
            title: p.title().to_string(),
            description: p.description().map(|d| d.as_html().to_string()),
            vendor: p.vendor().map(str::to_string),
            product_type: p.product_type().map(str::to_string),
            tags: p.tags().to_vec(),
            variants: p
                .variants()
                .iter()
                .map(|v| StorefrontVariantResponse {
                    id: v.id(),
                    media_asset_id: v.media(),
                })
                .collect(),
            media: media_items(&view.media, media).await?,
        })
    }
}

/// An active product; drafts and archived products read as missing.
#[utoipa::path(
    get,
    path = "/storefront/products/{id}",
    params(("id" = ProductId, Path)),
    responses((status = 200, description = "OK", body = StorefrontProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub(super) async fn fetch_storefront<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<ProductId>,
) -> Result<Json<StorefrontProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let view = get_product_view(&ctx, &ports, id).await?;
    if view.product.status() != ProductStatus::Active {
        return Err(hypha_products::CommandError::ProductNotFound(id).into());
    }
    Ok(Json(
        StorefrontProductResponse::new(&view, state.media.as_ref()).await?,
    ))
}

/// Changes a product's status (draft, active, archived).
#[utoipa::path(
    patch,
    path = "/admin/products/{id}/status",
    params(("id" = ProductId, Path)),
    request_body = SetStatusRequest,
    responses((status = 200, description = "OK", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn set_status<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    body: JsonBody<SetStatusRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let product = set_product_status(&ctx, ports.products(), id, req.status.into()).await?;
    respond(&state, &ports, product).await
}

/// Title and handle (JSON Merge Patch; neither can be `null`).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateProductRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    title: Option<String>,
    /// Changing it changes the product's storefront URLs; old ones aren't redirected.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    handle: Option<String>,
    /// HTML, up to 64 KiB; it's sanitised (scripts, styles and event attributes are
    /// removed), and the response has what was kept. `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    description: Option<Option<String>>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    vendor: Option<Option<String>>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    product_type: Option<Option<String>>,
    /// Replaces every tag. Up to 250; blanks are dropped and duplicates (ignoring case)
    /// keep their first spelling.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<String>, required = false)]
    tags: Option<Vec<String>>,
}

/// Changes a product's title, handle, description, vendor, product type or tags.
#[utoipa::path(
    patch,
    path = "/admin/products/{id}",
    params(("id" = ProductId, Path)),
    request_body = UpdateProductRequest,
    responses((status = 200, description = "OK", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    body: JsonBody<UpdateProductRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateProduct {
        title: req.title,
        handle: req.handle,
        description: req.description,
        vendor: req.vendor,
        product_type: req.product_type,
        tags: req.tags,
    };
    let product = update_product(&ctx, ports.products(), id, input).await?;
    respond(&state, &ports, product).await
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SetOptionsRequest {
    /// Every option, in order: they replace the product's.
    options: Vec<OptionRequest>,
    /// Variants to remove in the same write: the ones that use a value this request
    /// removes (a `409 option_value_in_use` lists them).
    #[serde(default)]
    remove_variants: Vec<VariantId>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct OptionRequest {
    /// An existing option, kept (and renamed or moved); omitted for a new one.
    #[serde(default)]
    id: Option<OptionId>,
    name: String,
    /// In order.
    values: Vec<OptionValueRequest>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct OptionValueRequest {
    /// An existing value of this option; omitted for a new one.
    #[serde(default)]
    id: Option<OptionValueId>,
    label: String,
}

/// Replaces a product's options and their values (design note 0014, section 3).
///
/// Options and values sent with their `id` are kept; the rest are new. A variant that
/// uses a removed value must be listed in `remove_variants` (else `409
/// option_value_in_use`, with the variants in `details`). Removing an option drops it
/// from every variant (`409 duplicate_option_combination` if two would then be the same);
/// a new option gives every variant its first value.
#[utoipa::path(
    put,
    path = "/admin/products/{id}/options",
    params(("id" = ProductId, Path)),
    request_body = SetOptionsRequest,
    responses((status = 200, description = "OK", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn set_options<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    body: JsonBody<SetOptionsRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = SetProductOptions {
        options: req
            .options
            .into_iter()
            .map(|o| OptionDraft {
                id: o.id,
                name: o.name,
                values: o
                    .values
                    .into_iter()
                    .map(|v| OptionValueDraft {
                        id: v.id,
                        label: v.label,
                    })
                    .collect(),
            })
            .collect(),
        remove_variants: req.remove_variants,
    };
    let product = set_product_options(&ctx, ports.products(), &state.ids, id, input).await?;
    respond(&state, &ports, product).await
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct AddVariantRequest {
    /// Unique across the store when set.
    #[serde(default)]
    sku: Option<String>,
    /// The base price, before markets and price lists.
    price: Money,
    /// One value per option, in the order of the product's `options`; empty for a
    /// product without options.
    #[serde(default)]
    selections: Vec<OptionValueId>,
    #[serde(default)]
    weight_grams: Option<u32>,
    /// Defaults to `true`.
    #[serde(default = "shipped")]
    requires_shipping: bool,
}

fn shipped() -> bool {
    true
}

/// Adds a variant: one combination of option values that isn't taken yet.
#[utoipa::path(
    post,
    path = "/admin/products/{id}/variants",
    params(("id" = ProductId, Path)),
    request_body = AddVariantRequest,
    responses((status = 201, description = "Created", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn add_variant<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    body: JsonBody<AddVariantRequest>,
) -> Result<(StatusCode, Json<ProductResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = AddVariant {
        sku: req.sku,
        price: req.price,
        selections: req.selections,
        weight: req.weight_grams.map(Weight::from_grams),
        requires_shipping: req.requires_shipping,
    };
    let product = add_product_variant(&ctx, ports.products(), &state.ids, id, input).await?;
    Ok((StatusCode::CREATED, respond(&state, &ports, product).await?))
}

/// A variant's SKU, price and option values (JSON Merge Patch).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateVariantRequest {
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    sku: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Money, required = false)]
    price: Option<Money>,
    /// One value per option, in option order; another variant can't have them.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<OptionValueId>, required = false)]
    selections: Option<Vec<OptionValueId>>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<u32>, required = false)]
    weight_grams: Option<Option<u32>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    requires_shipping: Option<bool>,
}

/// Changes a variant.
#[utoipa::path(
    patch,
    path = "/admin/products/{id}/variants/{variant_id}",
    params(("id" = ProductId, Path), ("variant_id" = VariantId, Path)),
    request_body = UpdateVariantRequest,
    responses((status = 200, description = "OK", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn update_variant<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, variant)): PathId<(ProductId, VariantId)>,
    body: JsonBody<UpdateVariantRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateVariant {
        sku: req.sku,
        price: req.price,
        selections: req.selections,
        weight: req.weight_grams.map(|w| w.map(Weight::from_grams)),
        requires_shipping: req.requires_shipping,
    };
    let product = update_product_variant(&ctx, ports.products(), id, variant, input).await?;
    respond(&state, &ports, product).await
}

/// Removes a variant; a product keeps at least one (`409 no_variants`). Price lists and
/// inventory keep its ID until they're cleaned up; carts show it as unavailable.
#[utoipa::path(
    delete,
    path = "/admin/products/{id}/variants/{variant_id}",
    params(("id" = ProductId, Path), ("variant_id" = VariantId, Path)),
    responses((status = 200, description = "OK", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn remove_variant<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, variant)): PathId<(ProductId, VariantId)>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let product = remove_product_variant(&ctx, ports.products(), id, variant).await?;
    respond(&state, &ports, product).await
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SetMediaRequest {
    /// Every item, in order (the first is featured): ready, public images and videos,
    /// each at most once, up to 250.
    assets: Vec<AssetId>,
}

/// Replaces a product's media list: attach, reorder and detach in one request.
/// Detaching never deletes the asset; variants showing a detached item show none.
#[utoipa::path(
    put,
    path = "/admin/products/{id}/media",
    params(("id" = ProductId, Path)),
    request_body = SetMediaRequest,
    responses((status = 200, description = "OK", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn set_media<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    body: JsonBody<SetMediaRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let view = set_product_media(&ctx, &ports, id, req.assets).await?;
    Ok(Json(
        ProductResponse::new(&view, state.media.as_ref()).await?,
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SetVariantMediaRequest {
    /// An image of the product's media list, or `null` for none.
    asset: Option<AssetId>,
}

/// Which image of its product's media a variant shows.
#[utoipa::path(
    put,
    path = "/admin/products/{id}/variants/{variant_id}/media",
    params(("id" = ProductId, Path), ("variant_id" = VariantId, Path)),
    request_body = SetVariantMediaRequest,
    responses((status = 200, description = "OK", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn set_variant_media_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, variant)): PathId<(ProductId, VariantId)>,
    body: JsonBody<SetVariantMediaRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let view = set_variant_media(&ctx, &ports, id, variant, req.asset).await?;
    Ok(Json(
        ProductResponse::new(&view, state.media.as_ref()).await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_snake_case_on_the_wire_and_matches_display() {
        for status in [
            ProductStatus::Draft,
            ProductStatus::Active,
            ProductStatus::Archived,
        ] {
            let json = serde_json::to_string(&StatusDto::from(status)).unwrap();
            assert_eq!(json, format!("\"{status}\""));
            let back: StatusDto = serde_json::from_str(&json).unwrap();
            assert_eq!(ProductStatus::from(back), status);
        }
        assert!(serde_json::from_str::<StatusDto>("\"Active\"").is_err());
    }
}
