//! Search (design note 0019): the storefront's search and product listing, and the
//! admin's settings, reindex and status.

use std::collections::BTreeMap;

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{AssetId, CategoryId, Money, ProductId, VariantId};
use hypha_media::{AssetRepo, HasAssets};
use hypha_search::{SearchInput, search};
use hypha_search_index::{
    Group, Sort, UpdateSearchSettings, get_search_settings, reindex, search_status,
    update_search_settings,
};
use serde::{Deserialize, Serialize};

use super::{
    page::{ListQuery, PageParams, cursor_at},
    products::{ProductMediaResponse, media_items},
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, Storefront},
    patch,
    state::{SharedState, Storage},
};

// ---------- storefront ----------

#[derive(Debug, Clone, Copy, Default, Deserialize, utoipa::ToSchema)]
#[schema(as = SearchSort)]
#[serde(rename_all = "snake_case")]
pub(super) enum SortDto {
    /// Best match first; without `q`, newest.
    #[default]
    Relevance,
    Newest,
    PriceAsc,
    PriceDesc,
    Title,
}

impl From<SortDto> for Sort {
    fn from(s: SortDto) -> Self {
        match s {
            SortDto::Relevance => Sort::Relevance,
            SortDto::Newest => Sort::Newest,
            SortDto::PriceAsc => Sort::PriceAsc,
            SortDto::PriceDesc => Sort::PriceDesc,
            SortDto::Title => Sort::Title,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, utoipa::ToSchema)]
#[schema(as = SearchGroup)]
#[serde(rename_all = "snake_case")]
pub(super) enum GroupDto {
    /// A product, or a product split by the options the merchant chose
    /// (`split_by` in the search settings).
    #[default]
    Listing,
    Product,
    /// Every variant on its own.
    Variant,
}

impl From<GroupDto> for Group {
    fn from(g: GroupDto) -> Self {
        match g {
            GroupDto::Listing => Group::Listing,
            GroupDto::Product => Group::Product,
            GroupDto::Variant => Group::Variant,
        }
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(super) struct SearchParams {
    /// Where the shopper is (ISO 3166-1 alpha-2): the market selling there prices and
    /// stocks the results.
    country: String,
    /// Words to find in titles, vendors, types, tags, SKUs, option values and
    /// descriptions; each must start a word. Absent lists every product.
    q: Option<String>,
    /// Repeatable `key:value`, key `option.<name>` (any case), `vendor`,
    /// `product_type` or `tag`: values of one key match any, different keys all.
    #[serde(default)]
    #[param(value_type = Vec<String>)]
    filter: Vec<String>,
    /// Inclusive, in the market currency's minor units.
    price_min: Option<i64>,
    price_max: Option<i64>,
    /// Only what can (`true`) or can't (`false`) be sold now.
    available: Option<bool>,
    /// Only products in this category or one below it (design note 0022); a hidden
    /// category is `404 category_not_found`.
    #[param(value_type = Option<CategoryId>)]
    category: Option<CategoryId>,
    #[param(inline)]
    sort: Option<SortDto>,
    /// What one result stands for. Default `listing`.
    #[param(inline)]
    group: Option<GroupDto>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct SearchPriceRange {
    min: Money,
    /// Equal to `min` when every price is the same.
    max: Money,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct SearchOption {
    name: String,
    value: String,
}

/// One result: a listing, a product or a variant (the request's `group`), shown by
/// its best match.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct SearchHitResponse {
    product: ProductId,
    /// The variant shown: the best match of its group.
    variant: VariantId,
    /// The listing it belongs to: the product, split by the merchant's options.
    listing: String,
    handle: String,
    title: String,
    /// "M / Blue"; `null` for a product without options.
    variant_title: Option<String>,
    vendor: Option<String>,
    product_type: Option<String>,
    /// The variant's options, in order.
    options: Vec<SearchOption>,
    /// In the market's currency.
    price: Money,
    /// Over the product's variants in this market.
    product_price: SearchPriceRange,
    /// Over its listing's variants.
    listing_price: SearchPriceRange,
    /// Whether the variant shown can be sold now.
    available: bool,
    /// The variant's image, else the product's featured one.
    image: Option<ProductMediaResponse>,
    /// How many of the group's variants matched.
    matched_variants: u32,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct FacetValueResponse {
    value: String,
    /// Results (as grouped) with this value, ignoring this facet's own filter.
    count: u64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct FacetResponse {
    /// As `filter` takes it: `option.color`, `vendor`, `product_type`, `tag`.
    key: String,
    /// The merchant's label, if they set one.
    label: Option<String>,
    /// Most results first, then by value; at most 100.
    values: Vec<FacetValueResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct SearchResponse {
    items: Vec<SearchHitResponse>,
    /// Results over every page.
    total: u64,
    /// The facets the merchant shows (or the defaults), counted over these results.
    facets: Vec<FacetResponse>,
    /// The lowest and highest price among the results, ignoring `price_min` and
    /// `price_max` (for a price slider); `null` when nothing matches.
    price: Option<SearchPriceRange>,
    has_more: bool,
    /// Pass as `cursor` for the next page; `null` on the last. Opaque.
    next_cursor: Option<String>,
}

/// Searches the products the shopper's market sells, priced and stocked there; without
/// `q`, the market's product listing. Pages go at most 1,000 results deep.
#[utoipa::path(
    get,
    path = "/storefront/search",
    params(PageParams, SearchParams),
    responses((status = 200, description = "OK", body = SearchResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub(super) async fn storefront_search<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    query: ListQuery<SearchParams>,
) -> Result<Json<SearchResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, params, scope) = query.parse::<u32>()?;
    let offset = page.after.unwrap_or(0);
    let limit = page.limit.get();
    let input = SearchInput {
        country: params.country,
        q: params.q,
        filters: params.filter,
        price_min: params.price_min,
        price_max: params.price_max,
        available: params.available,
        category: params.category,
        sort: params.sort.unwrap_or_default().into(),
        group: params.group.unwrap_or_default().into(),
        offset,
        limit,
    };
    let found = search(&ctx, &ports, &state.storage.search(&ports), input).await?;
    let results = found.results;

    // Each image once, in one query.
    let mut wanted: Vec<AssetId> = results
        .hits
        .iter()
        .filter_map(|h| h.document.image)
        .collect();
    wanted.sort();
    wanted.dedup();
    let assets = ports.assets().get_many(&wanted).await?;
    let mut images: BTreeMap<AssetId, ProductMediaResponse> = BTreeMap::new();
    for item in media_items(&assets, state.media.as_ref()).await? {
        images.insert(item.asset_id(), item);
    }

    let range = |r: hypha_search_index::PriceRange| SearchPriceRange {
        min: r.min,
        max: r.max,
    };
    // Pages stop at `MAX_DEPTH`: past it, there is no next page to offer.
    let shown = u64::from(offset) + results.hits.len() as u64;
    let has_more = shown < results.total.min(u64::from(hypha_search_index::MAX_DEPTH));
    let items = results
        .hits
        .into_iter()
        .map(|hit| {
            let d = hit.document;
            SearchHitResponse {
                // Variants of one product often share it: cloned, not taken.
                image: d.image.and_then(|id| images.get(&id).cloned()),
                product: d.product,
                variant: d.variant,
                listing: d.listing,
                handle: d.handle,
                title: d.title,
                variant_title: d.variant_title,
                vendor: d.vendor,
                product_type: d.product_type,
                options: d
                    .options
                    .into_iter()
                    .map(|(name, value)| SearchOption { name, value })
                    .collect(),
                price: d.price,
                product_price: range(d.product_price),
                listing_price: range(d.listing_price),
                available: d.available,
                matched_variants: hit.matched,
            }
        })
        .collect();
    Ok(Json(SearchResponse {
        items,
        total: results.total,
        facets: results
            .facets
            .into_iter()
            .map(|f| FacetResponse {
                label: found.labels.get(&f.key).cloned(),
                key: f.key.to_string(),
                values: f
                    .values
                    .into_iter()
                    .map(|v| FacetValueResponse {
                        value: v.value,
                        count: v.count,
                    })
                    .collect(),
            })
            .collect(),
        price: results
            .price
            .map(|(min, max)| SearchPriceRange { min, max }),
        has_more,
        next_cursor: if has_more {
            Some(cursor_at(&scope, shown)?)
        } else {
            None
        },
    }))
}

// ---------- admin ----------

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct FacetSettingResponse {
    /// `option.<name>`, `vendor`, `product_type` or `tag`.
    key: String,
    label: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct SearchSettingsResponse {
    /// Option names whose values split listings, in order ("show each Color
    /// separately"). Empty: one listing per product.
    split_by: Vec<String>,
    /// The facets storefronts show, in order. Empty: the defaults (product type,
    /// vendor, the market's most used options, tags).
    facets: Vec<FacetSettingResponse>,
}

impl From<&hypha_search_index::SearchSettings> for SearchSettingsResponse {
    fn from(s: &hypha_search_index::SearchSettings) -> Self {
        Self {
            split_by: s.split_by().to_vec(),
            facets: s
                .facets()
                .iter()
                .map(|f| FacetSettingResponse {
                    key: f.key.to_string(),
                    label: f.label.clone(),
                })
                .collect(),
        }
    }
}

/// The store's search settings (the defaults until first saved).
#[utoipa::path(
    get,
    path = "/admin/search/settings",
    responses((status = 200, description = "OK", body = SearchSettingsResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub(super) async fn fetch_settings<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<SearchSettingsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let settings = get_search_settings(&ctx, &ports).await?;
    Ok(Json(SearchSettingsResponse::from(&settings)))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FacetSettingRequest {
    /// `option.<name>`, `vendor`, `product_type` or `tag`.
    key: String,
    /// Up to 255 characters; absent or `null` lets the storefront label it.
    #[serde(default)]
    label: Option<String>,
}

/// A JSON Merge Patch of the settings.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateSearchSettingsRequest {
    /// At most 3 option names, unique ignoring case. Changing them rebuilds every
    /// search document in the background.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<String>, required = false)]
    split_by: Option<Vec<String>>,
    /// At most 20, unique; empty for the defaults.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<FacetSettingRequest>, required = false)]
    facets: Option<Vec<FacetSettingRequest>>,
}

/// Changes which options split listings and which facets storefronts show.
#[utoipa::path(
    patch,
    path = "/admin/search/settings",
    request_body = UpdateSearchSettingsRequest,
    responses((status = 200, description = "OK", body = SearchSettingsResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn update_settings<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<UpdateSearchSettingsRequest>,
) -> Result<Json<SearchSettingsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateSearchSettings {
        split_by: req.split_by,
        facets: req
            .facets
            .map(|facets| facets.into_iter().map(|f| (f.key, f.label)).collect()),
    };
    let settings = update_search_settings(&ctx, &ports, input).await?;
    Ok(Json(SearchSettingsResponse::from(&settings)))
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct SearchStatusResponse {
    /// Products (or everything) waiting to be rebuilt. 0 when the index is up to date.
    queued: u64,
    /// Documents in the index: one per variant per market it's sold in.
    documents: u64,
}

/// How far behind the index is.
#[utoipa::path(
    get,
    path = "/admin/search/status",
    responses((status = 200, description = "OK", body = SearchStatusResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub(super) async fn status<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<SearchStatusResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let status = search_status(&ctx, &ports, &state.storage.search(&ports)).await?;
    Ok(Json(SearchStatusResponse {
        queued: status.queued,
        documents: status.documents,
    }))
}

/// Rebuilds every search document in the background (after restoring a database, say);
/// `status` shows the progress.
#[utoipa::path(
    post,
    path = "/admin/search/reindex",
    responses((status = 202, description = "Queued", body = SearchStatusResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn start_reindex<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<(StatusCode, Json<SearchStatusResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    reindex(&ctx, &ports).await?;
    let status = search_status(&ctx, &ports, &state.storage.search(&ports)).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(SearchStatusResponse {
            queued: status.queued,
            documents: status.documents,
        }),
    ))
}
