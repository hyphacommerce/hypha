use hypha_core::Stamped;

use super::stamp_times;
use axum::{Json, extract::State, http::StatusCode};
use hypha_cart::{ProductOffer, price_product};
use hypha_categories::categories_of_products;
use hypha_channels::HasChannels;
use hypha_channels::{channels_of_products, product_channels};
use hypha_core::{
    AssetId, CategoryId, ChannelId, Ctx, Currency, Keyset, Money, OptionId, OptionValueId,
    ProductId, SortKey, VariantId, Weight,
};
use hypha_inventory::{HasInventoryItems, HasLocations};
use hypha_markets::{HasCatalogs, HasMarkets, HasPriceLists};
use hypha_media::{Asset, HasAssets};
use hypha_merchandising::{
    ProductSummaryView, ProductView, delete_product, get_product_view, get_product_view_by_handle,
    list_product_views, set_product_media, set_variant_media,
};
use hypha_products::{
    AddVariant, CreateSimpleProduct, HasProducts, ListProducts, OptionDraft, OptionValueDraft,
    Product, ProductFilterInput, ProductOption, ProductSort, ProductStatus, SetProductOptions,
    SetVariant, UpdateProduct, UpdateVariant, add_product_variant, create_simple_product,
    remove_product_variant, set_product_options, set_product_status, set_product_variants,
    update_product, update_product_variant,
};
use serde::{Deserialize, Serialize};

use hypha_custom_fields::{
    CallerKind, FieldsRead, HasCustomFieldDefinitions, HasCustomFieldValues, OwnerType,
};

use super::{
    assets::AssetKindDto,
    custom_fields::{self, CustomFieldValues, FieldParams},
    page::{ListQuery, PageParams, PageResponse, SortedFilters},
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId, Query, Storefront},
    media::Media,
    patch,
    state::{AppState, SharedState, Storage},
};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProductRequest {
    handle: String,
    title: String,
    price: Money,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetStatusRequest {
    status: StatusDto,
}

/// `ProductStatus` on the wire: snake_case strings. A DTO of its own, so the domain
/// enum carries no serde and renaming a variant can't silently change the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = ProductStatus)]
#[serde(rename_all = "snake_case")]
pub enum StatusDto {
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
pub struct ProductResponse {
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
    /// The sales channels it is published to, sorted (design note 0020); a new product
    /// is in none.
    channels: Vec<ChannelId>,
    /// The categories it's directly in, sorted (design note 0022).
    categories: Vec<CategoryId>,
    /// Its custom field values the caller may read, `{"custom.material": "wool", …}`
    /// (design note 0033); `PATCH /admin/products/{id}/custom-fields` writes them.
    #[schema(value_type = std::collections::HashMap<String, serde_json::Value>)]
    custom_fields: CustomFieldValues,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

/// One item of a product's media list (design note 0014, section 7): what a page needs
/// to render it. The asset's own route has the rest.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct ProductMediaResponse {
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
    pub(super) fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    pub async fn new(asset: &Asset, media: &Media) -> Result<Self, ApiError> {
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
pub async fn media_items(
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
pub struct OptionResponse {
    id: OptionId,
    name: String,
    values: Vec<OptionValueResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OptionValueResponse {
    id: OptionValueId,
    label: String,
}

impl From<&ProductOption> for OptionResponse {
    fn from(o: &ProductOption) -> Self {
        Self {
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
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct VariantResponse {
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
    /// Its own custom field values the caller may read (variant fields, design note
    /// 0033); `PATCH /admin/variants/{id}/custom-fields` writes them.
    #[schema(value_type = std::collections::HashMap<String, serde_json::Value>)]
    custom_fields: CustomFieldValues,
}

impl ProductResponse {
    async fn new(
        view: &ProductView,
        channels: Vec<ChannelId>,
        categories: Vec<CategoryId>,
        fields: &FieldsRead,
        media: Option<&Media>,
    ) -> Result<Self, ApiError> {
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
            options: p.options().iter().map(OptionResponse::from).collect(),
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
                    custom_fields: fields.json_of(v.id().as_uuid()),
                })
                .collect(),
            media: media_items(&view.media, media).await?,
            channels,
            categories,
            custom_fields: fields.json_of(p.id().as_uuid()),
        })
    }
}

/// The product's and its variants' custom field values `kind` may read, in one read.
async fn product_fields<P>(
    ctx: &Ctx,
    kind: &CallerKind,
    ports: &P,
    product: &Product,
) -> Result<FieldsRead, ApiError>
where
    P: HasCustomFieldDefinitions + HasCustomFieldValues,
{
    let owners: Vec<uuid::Uuid> = std::iter::once(product.id().as_uuid())
        .chain(product.variants().iter().map(|v| v.id().as_uuid()))
        .collect();
    custom_fields::read(
        ctx,
        kind,
        ports,
        &[OwnerType::Product, OwnerType::Variant],
        &owners,
        None,
    )
    .await
}

/// The response for a product a command just returned, with its media's assets.
async fn respond<S: Storage>(
    state: &SharedState<S>,
    ports: &S::Ports<'_>,
    ctx: &Ctx,
    product: Product,
) -> Result<Json<ProductResponse>, ApiError> {
    let view = ProductView::load(ports.assets(), product).await?;
    respond_view(state, ports, ctx, &view).await
}

/// The response for `view`, with the channels its product is published to.
async fn respond_view<S: Storage>(
    state: &SharedState<S>,
    ports: &S::Ports<'_>,
    ctx: &Ctx,
    view: &ProductView,
) -> Result<Json<ProductResponse>, ApiError> {
    let id = view.product.id();
    let channels = product_channels(ctx, ports, id).await?;
    let categories = categories_of_products(ctx, ports, &[id])
        .await?
        .remove(&id)
        .unwrap_or_default();
    let kind = custom_fields::caller_kind(ctx, ports).await?;
    let fields = product_fields(ctx, &kind, ports, &view.product).await?;
    Ok(Json(
        ProductResponse::new(view, channels, categories, &fields, state.media.as_deref()).await?,
    ))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ProductFilters {
    /// Only products in this status; repeat for any of several (`status=draft&status=active`).
    #[serde(default)]
    status: Vec<StatusDto>,
    /// Only products whose title, handle or a variant's SKU contains this text, ignoring
    /// case. Up to 255 characters; blank filters nothing. Reads the products as they are
    /// now (drafts too); storefronts search with `/storefront/search`.
    q: Option<String>,
    /// Only products of this vendor, matched whole, ignoring case.
    vendor: Option<String>,
    /// Only products of this product type, matched whole, ignoring case.
    product_type: Option<String>,
    /// Only products with this tag, matched whole, ignoring case.
    tag: Option<String>,
    /// Only products directly in this category (not those below it). An ID that names
    /// no category matches nothing.
    #[param(value_type = Option<CategoryId>)]
    category: Option<CategoryId>,
    /// Only products published to this sales channel. An ID that names no channel
    /// matches nothing.
    #[param(value_type = Option<ChannelId>)]
    channel: Option<ChannelId>,
    #[param(inline)]
    sort: Option<ProductSortDto>,
}

/// The order of a product list (design note 0024, AU1). Default `newest`. Titles
/// compare lowercased, character by character (accented letters after `z`); ties go by
/// creation, in the same direction.
#[derive(Debug, Clone, Copy, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProductSortDto {
    Newest,
    Oldest,
    TitleAsc,
    TitleDesc,
    UpdatedDesc,
    UpdatedAsc,
}

impl From<ProductSortDto> for ProductSort {
    fn from(s: ProductSortDto) -> Self {
        match s {
            ProductSortDto::Newest => ProductSort::Newest,
            ProductSortDto::Oldest => ProductSort::Oldest,
            ProductSortDto::TitleAsc => ProductSort::TitleAsc,
            ProductSortDto::TitleDesc => ProductSort::TitleDesc,
            ProductSortDto::UpdatedDesc => ProductSort::UpdatedDesc,
            ProductSortDto::UpdatedAsc => ProductSort::UpdatedAsc,
        }
    }
}

impl ProductFilters {
    fn sort(&self) -> ProductSort {
        self.sort.map(Into::into).unwrap_or_default()
    }
}

impl SortedFilters for ProductFilters {
    fn fits(&self, key: &SortKey) -> bool {
        self.sort().fits(key)
    }
}

/// A product in a list: what a table of products shows, without options or variants
/// (design note 0012, L7). `GET /admin/products/{id}` has the rest.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ProductSummaryResponse {
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
    /// The sales channels it is published to, sorted (design note 0020).
    channels: Vec<ChannelId>,
    /// The categories it's directly in, sorted (design note 0022).
    categories: Vec<CategoryId>,
    /// The custom field values `?custom_fields=` asked for (design note 0033); absent
    /// without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<std::collections::HashMap<String, serde_json::Value>>, required = false)]
    custom_fields: Option<CustomFieldValues>,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PriceRangeResponse {
    min: Money,
    /// Equal to `min` when every variant in this currency costs the same.
    max: Money,
}

impl ProductSummaryResponse {
    async fn new(
        view: &ProductSummaryView,
        channels: Vec<ChannelId>,
        categories: Vec<CategoryId>,
        custom_fields: Option<CustomFieldValues>,
        media: Option<&Media>,
    ) -> Result<Self, ApiError> {
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
            channels,
            categories,
            custom_fields,
        })
    }
}

/// A page of products (named for the documents: utoipa's `body =` takes a plain name).
type ProductSummaryPage = PageResponse<ProductSummaryResponse>;

/// Products in any status, newest first or in `sort`'s order, narrowed by the filters
/// (every one given must match), custom fields' among them.
#[utoipa::path(
    get,
    path = "/admin/products",
    params(PageParams, ProductFilters, FieldParams),
    responses((status = 200, description = "OK", body = ProductSummaryPage)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<ProductFilters>,
) -> Result<Json<ProductSummaryPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope, field_query) = query.parse_sorted_with_fields()?;
    let fields = custom_fields::list_fields(&ctx, &ports, OwnerType::Product, &field_query).await?;
    let sort = filters.sort();
    let input = ListProducts {
        filter: ProductFilterInput {
            statuses: filters.status.into_iter().map(Into::into).collect(),
            q: filters.q,
            vendor: filters.vendor,
            product_type: filters.product_type,
            tag: filters.tag,
            category: filters.category,
            channel: filters.channel,
        },
        sort,
        fields: fields.filters,
    };
    let page = list_product_views(&ctx, &ports, input, page).await?;
    let ids: Vec<ProductId> = page.items.iter().map(|v| v.summary.id).collect();
    let mut channels = channels_of_products(&ctx, &ports, &ids).await?;
    let mut categories = categories_of_products(&ctx, &ports, &ids).await?;
    let values = match &fields.keys {
        Some(keys) => {
            let owners: Vec<uuid::Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
            let read = custom_fields::read(
                &ctx,
                &fields.kind,
                &ports,
                &[OwnerType::Product],
                &owners,
                Some(keys),
            )
            .await?;
            Some(read)
        }
        None => None,
    };
    let mut items = Vec::with_capacity(page.items.len());
    for view in &page.items {
        let id = view.summary.id;
        let of = channels.remove(&id).unwrap_or_default();
        let in_categories = categories.remove(&id).unwrap_or_default();
        let custom = values.as_ref().map(|v| v.json_of(id.as_uuid()));
        items.push(
            ProductSummaryResponse::new(view, of, in_categories, custom, state.media.as_deref())
                .await?,
        );
    }
    Ok(Json(PageResponse::sorted(
        &page,
        &scope,
        |v| Keyset {
            key: sort.key(&v.summary),
            id: v.summary.id,
        },
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
pub async fn create<S: Storage>(
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
    Ok((
        StatusCode::CREATED,
        respond(&state, &ports, &ctx, product).await?,
    ))
}

/// A product, in any status.
#[utoipa::path(
    get,
    path = "/admin/products/{id}",
    params(("id" = ProductId, Path)),
    responses((status = 200, description = "OK", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let view = get_product_view(&ctx, &ports, id).await?;
    respond_view(&state, &ports, &ctx, &view).await
}

/// A product as a storefront sees it (design note 0010, S5): active products only, without
/// admin fields. A draft or archived product is `404 product_not_found`, the same as one
/// that doesn't exist, so a storefront can't tell them apart.
///
/// Prices and stock are the shopper's: with `country`, each variant carries its price
/// in that country's market and whether it can be bought now (design note 0021, SU7),
/// priced as a cart prices it. Without `country`, there are none: a product's base price
/// isn't what any shopper pays.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StorefrontProductResponse {
    id: ProductId,
    handle: String,
    title: String,
    /// Sanitised HTML (scripts, styles and event attributes removed), safe to render.
    description: Option<String>,
    vendor: Option<String>,
    product_type: Option<String>,
    /// In the merchant's order.
    tags: Vec<String>,
    /// What a shopper picks a variant by, in order.
    options: Vec<OptionResponse>,
    variants: Vec<StorefrontVariantResponse>,
    /// In order; the first is the featured item.
    media: Vec<ProductMediaResponse>,
    /// Whether `country`'s market sells the product; `null` without `country`. A
    /// product not sold there has no prices: show it as unavailable in the country.
    sold_in_country: Option<bool>,
    /// The currency of the prices; `null` when there are none.
    currency: Option<Currency>,
    /// Whether the prices include tax (the market's rule); `null` when there are none.
    prices_include_tax: Option<bool>,
    /// The custom field values the merchant shows storefronts (`access.storefront:
    /// read`, design note 0033), `{"custom.material": "wool", …}`.
    #[schema(value_type = std::collections::HashMap<String, serde_json::Value>)]
    custom_fields: CustomFieldValues,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct StorefrontVariantResponse {
    id: VariantId,
    /// Its option values ("M / Blue"); `null` for a product without options.
    title: Option<String>,
    /// One value per option, in the order of `options`.
    selections: Vec<OptionValueId>,
    /// The item of the product's `media` this variant shows (an image), if any.
    media_asset_id: Option<AssetId>,
    /// In `country`'s market; `null` without `country` or where it isn't sold.
    price: Option<Money>,
    /// Whether one can be bought now (in stock, or stock isn't tracked); `null` where
    /// there's no price.
    available: Option<bool>,
    /// How many can be bought now: the stock left in the market's locations, or `null`
    /// when there's no limit (stock isn't tracked, or it sells past zero). `0` where
    /// there's no price.
    available_quantity: Option<u64>,
    /// The variant's own custom field values storefronts are shown.
    #[schema(value_type = std::collections::HashMap<String, serde_json::Value>)]
    custom_fields: CustomFieldValues,
}

impl StorefrontProductResponse {
    async fn new(
        view: &ProductView,
        offer: Option<&ProductOffer>,
        fields: &FieldsRead,
        media: Option<&Media>,
    ) -> Result<Self, ApiError> {
        let p = &view.product;
        let sold = offer.map(ProductOffer::sold);
        let market = offer.filter(|o| o.sold()).and_then(|o| o.market.as_ref());
        Ok(Self {
            id: p.id(),
            handle: p.handle().as_str().to_string(),
            title: p.title().to_string(),
            description: p.description().map(|d| d.as_html().to_string()),
            vendor: p.vendor().map(str::to_string),
            product_type: p.product_type().map(str::to_string),
            tags: p.tags().to_vec(),
            options: p.options().iter().map(OptionResponse::from).collect(),
            variants: p
                .variants()
                .iter()
                .map(|v| {
                    let priced = market
                        .and(offer)
                        .and_then(|o| o.variants.iter().find(|offered| offered.variant == v.id()));
                    StorefrontVariantResponse {
                        id: v.id(),
                        title: p.variant_title(v.id()),
                        selections: v.selections().to_vec(),
                        media_asset_id: v.media(),
                        price: priced.and_then(|o| o.price).map(|price| price.money()),
                        available: priced.filter(|o| o.price.is_some()).map(|o| o.available),
                        available_quantity: match priced {
                            Some(o) => o.sellable,
                            None => Some(0),
                        },
                        custom_fields: fields.json_of(v.id().as_uuid()),
                    }
                })
                .collect(),
            media: media_items(&view.media, media).await?,
            sold_in_country: sold,
            currency: market.map(|m| m.currency()),
            prices_include_tax: market.map(|m| m.prices_include_tax()),
            custom_fields: fields.json_of(p.id().as_uuid()),
        })
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ProductCountryParams {
    /// Where the shopper is (ISO 3166-1 alpha-2): its market prices and stocks the
    /// variants. Without it, the product comes without prices.
    country: Option<String>,
}

/// The storefront's response for `view`, after the checks every storefront read makes:
/// a draft, archived or unpublished product reads as missing (0020, CH6).
async fn storefront_product<P>(
    state: &AppState<impl Storage>,
    ports: &P,
    ctx: &Ctx,
    view: ProductView,
    country: Option<String>,
) -> Result<Json<StorefrontProductResponse>, ApiError>
where
    P: HasChannels
        + HasMarkets
        + HasCatalogs
        + HasPriceLists
        + HasInventoryItems
        + HasLocations
        + HasCustomFieldDefinitions
        + HasCustomFieldValues,
{
    let id = view.product.id();
    let published = match ctx.channel() {
        Some(channel) => hypha_channels::is_published(ports, channel, id).await?,
        None => false,
    };
    if view.product.status() != ProductStatus::Active || !published {
        return Err(hypha_products::CommandError::ProductNotFound(id).into());
    }
    let offer = match country {
        Some(country) => Some(price_product(ctx, ports, &view.product, &country).await?),
        None => None,
    };
    let fields = product_fields(ctx, &CallerKind::Storefront, ports, &view.product).await?;
    Ok(Json(
        StorefrontProductResponse::new(&view, offer.as_ref(), &fields, state.media.as_deref())
            .await?,
    ))
}

/// An active product sold through the caller's channel, priced in `country`; drafts,
/// archived products and products the channel doesn't sell read as missing.
#[utoipa::path(
    get,
    path = "/storefront/products/{id}",
    params(("id" = ProductId, Path), ProductCountryParams),
    responses((status = 200, description = "OK", body = StorefrontProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub async fn fetch_storefront<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<ProductId>,
    query: Query<ProductCountryParams>,
) -> Result<Json<StorefrontProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let params = query.parse()?;
    let view = get_product_view(&ctx, &ports, id).await?;
    storefront_product(&state, &ports, &ctx, view, params.country).await
}

/// A product page in one read (design note 0021, SU7): the product by its handle,
/// priced in `country`, with the same rules as the read by ID.
#[utoipa::path(
    get,
    path = "/storefront/products/by-handle/{handle}",
    params(
        ("handle" = String, Path, description = "The product's handle.", example = "ceramic-mug"),
        ProductCountryParams
    ),
    responses((status = 200, description = "OK", body = StorefrontProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub async fn fetch_by_handle_storefront<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(handle): PathId<String>,
    query: Query<ProductCountryParams>,
) -> Result<Json<StorefrontProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let params = query.parse()?;
    let view = get_product_view_by_handle(&ctx, &ports, &handle).await?;
    storefront_product(&state, &ports, &ctx, view, params.country).await
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
pub async fn set_status<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    body: JsonBody<SetStatusRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let product = set_product_status(&ctx, ports.products(), id, req.status.into()).await?;
    respond(&state, &ports, &ctx, product).await
}

/// Title and handle (JSON Merge Patch; neither can be `null`).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProductRequest {
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
pub async fn update<S: Storage>(
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
    respond(&state, &ports, &ctx, product).await
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetOptionsRequest {
    /// Every option, in order: they replace the product's.
    options: Vec<OptionRequest>,
    /// Variants to remove in the same write: the ones that use a value this request
    /// removes (a `409 option_value_in_use` lists them).
    #[serde(default)]
    remove_variants: Vec<VariantId>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OptionRequest {
    /// An existing option, kept (and renamed or moved); omitted for a new one.
    #[serde(default)]
    id: Option<OptionId>,
    name: String,
    /// In order.
    values: Vec<OptionValueRequest>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OptionValueRequest {
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
pub async fn set_options<S: Storage>(
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
    respond(&state, &ports, &ctx, product).await
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddVariantRequest {
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
pub async fn add_variant<S: Storage>(
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
    Ok((
        StatusCode::CREATED,
        respond(&state, &ports, &ctx, product).await?,
    ))
}

/// A variant in a whole list. Every key is required, `null` where allowed: a `PUT`
/// replaces the list, so a missing key would silently clear what's stored.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetVariantRequest {
    /// The variant to keep; `null` adds one.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schema(required = true)]
    id: Option<VariantId>,
    /// Unique across the store; `null` for none.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schema(required = true)]
    sku: Option<String>,
    /// The base price, before markets and price lists.
    price: Money,
    /// One value per option, in the order of the product's `options`; empty for a
    /// product without options.
    selections: Vec<OptionValueId>,
    /// `null` when not entered.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schema(required = true)]
    weight_grams: Option<u32>,
    requires_shipping: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetVariantsRequest {
    /// The product's variants after the change, in order: listed variants with an `id`
    /// are updated, those without are added, and stored ones left out are removed.
    /// 1 to 2,048; options are never expanded into variants here.
    variants: Vec<SetVariantRequest>,
}

/// Sets a product's whole variant list in one write (design note 0021, Q1): all or
/// nothing, with one set of events. A variant keeps its media.
#[utoipa::path(
    put,
    path = "/admin/products/{id}/variants",
    params(("id" = ProductId, Path)),
    request_body = SetVariantsRequest,
    responses((status = 200, description = "OK", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn set_variants<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    body: JsonBody<SetVariantsRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let variants = req
        .variants
        .into_iter()
        .map(|v| SetVariant {
            id: v.id,
            sku: v.sku,
            price: v.price,
            selections: v.selections,
            weight: v.weight_grams.map(Weight::from_grams),
            requires_shipping: v.requires_shipping,
        })
        .collect();
    let product = set_product_variants(&ctx, ports.products(), &state.ids, id, variants).await?;
    respond(&state, &ports, &ctx, product).await
}

/// A variant's SKU, price and option values (JSON Merge Patch).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateVariantRequest {
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
pub async fn update_variant<S: Storage>(
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
    respond(&state, &ports, &ctx, product).await
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
pub async fn remove_variant<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, variant)): PathId<(ProductId, VariantId)>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let product = remove_product_variant(&ctx, ports.products(), id, variant).await?;
    respond(&state, &ports, &ctx, product).await
}

/// Deletes a product with its variants and options, whatever its status (design note
/// 0033, section 6.3). Its media stay in the library; its catalog, channel and category
/// membership go with it, and its fixed prices, stock records and search listings
/// shortly after (`product.deleted`). Orders keep their lines; carts show its lines as
/// unavailable until they're removed. Archive instead to keep it.
#[utoipa::path(
    delete,
    path = "/admin/products/{id}",
    params(("id" = ProductId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn remove<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_product(&ctx, &ports, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetMediaRequest {
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
pub async fn set_media<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    body: JsonBody<SetMediaRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let view = set_product_media(&ctx, &ports, id, req.assets).await?;
    respond_view(&state, &ports, &ctx, &view).await
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetVariantMediaRequest {
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
pub async fn set_variant_media_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, variant)): PathId<(ProductId, VariantId)>,
    body: JsonBody<SetVariantMediaRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let view = set_variant_media(&ctx, &ports, id, variant, req.asset).await?;
    respond_view(&state, &ports, &ctx, &view).await
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
