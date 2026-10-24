//! Markets, price lists and catalogs (all live in `hypha-markets`).

use std::collections::HashMap;

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{
    CatalogId, Clock, CountryCode, Ctx, Currency, MarketId, Money, PriceListId, ProductId, Stamped,
    Timestamp, VariantId,
};
use hypha_markets::{
    Catalog, CatalogContext, CompareAtMode, CreateCatalog, CreateMarket, CreatePriceList,
    HasCatalogs, HasMarkets, HasPriceLists, Market, MarketView, PriceList, PriceListKind,
    PriceListPhase, PriceListStatus, SetFixedPrice, UpdateMarket, add_catalog_products,
    add_market_country, assign_price_list, create_catalog, create_market, create_price_list,
    get_catalog, get_market, get_price_list, list_catalog_products, list_fixed_prices,
    list_markets, list_markets_only, list_price_lists, remove_catalog_products,
    remove_fixed_prices, remove_market_country, set_fixed_prices, update_market, update_price_list,
};
use hypha_products::{HasProducts, get_products, products_of_variants};
use serde::{Deserialize, Serialize};

use super::{
    page::{ListQuery, PageParams, PageResponse},
    products::StatusDto,
    rfc3339, stamp_times,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId, Storefront},
    patch,
    state::{SharedState, Storage},
};

// ---------- markets ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMarketRequest {
    name: String,
    countries: Vec<String>,
    currency: Currency,
    #[serde(default)]
    prices_include_tax: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMarketRequest {
    /// 1 to 255 characters, trimmed.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    // Optional, but `null` is refused (`patch::non_null`): not nullable in the spec.
    #[schema(value_type = bool, required = false)]
    prices_include_tax: Option<bool>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MarketResponse {
    id: MarketId,
    name: String,
    countries: Vec<CountryCode>,
    currency: Currency,
    prices_include_tax: bool,
    /// The catalog of what it sells, once one is created (`POST /admin/catalogs`).
    catalog: Option<CatalogId>,
}

impl MarketResponse {
    fn new(m: &Market, catalog: Option<CatalogId>) -> Self {
        Self {
            id: m.id(),
            name: m.name().to_string(),
            countries: m.countries().to_vec(),
            currency: m.currency(),
            prices_include_tax: m.prices_include_tax(),
            catalog,
        }
    }
}

impl From<&MarketView> for MarketResponse {
    fn from(v: &MarketView) -> Self {
        Self::new(&v.market, v.catalog)
    }
}

/// The response after a market changed: the market as stored, with its catalog read
/// back (a write scope covers reading it, `Scope::implied_by`).
async fn changed_market<S: HasMarkets + HasCatalogs>(
    ctx: &Ctx,
    ports: &S,
    market: &Market,
) -> Result<MarketResponse, ApiError> {
    let view = get_market(ctx, ports, market.id()).await?;
    Ok(MarketResponse::new(market, view.catalog))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MarketListResponse {
    /// Oldest first. A store has a handful, so they come in one list.
    items: Vec<MarketResponse>,
}

/// Every market, oldest first, with its catalog.
#[utoipa::path(
    get,
    path = "/admin/markets",
    responses((status = 200, description = "OK", body = MarketListResponse)),
    extensions(("x-hypha-scopes" = json!(["markets:read"]))),
)]
pub async fn list_markets_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<MarketListResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let views = list_markets(&ctx, &ports).await?;
    Ok(Json(MarketListResponse {
        items: views.iter().map(MarketResponse::from).collect(),
    }))
}

/// A market, with its catalog.
#[utoipa::path(
    get,
    path = "/admin/markets/{id}",
    params(("id" = MarketId, Path)),
    responses((status = 200, description = "OK", body = MarketResponse)),
    extensions(("x-hypha-scopes" = json!(["markets:read"]))),
)]
pub async fn fetch_market<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<MarketId>,
) -> Result<Json<MarketResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let view = get_market(&ctx, &ports, id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("market_not_found"))?;
    Ok(Json(MarketResponse::from(&view)))
}

/// Creates a market: countries that share a currency.
#[utoipa::path(
    post,
    path = "/admin/markets",
    request_body = CreateMarketRequest,
    responses((status = 201, description = "Created", body = MarketResponse)),
    extensions(("x-hypha-scopes" = json!(["markets:write"]))),
)]
pub async fn create_market_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateMarketRequest>,
) -> Result<(StatusCode, Json<MarketResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateMarket {
        name: req.name,
        countries: req.countries,
        currency: req.currency,
        prices_include_tax: req.prices_include_tax,
    };
    let market = create_market(&ctx, ports.markets(), &state.ids, input).await?;
    // A new market has no catalog yet.
    Ok((
        StatusCode::CREATED,
        Json(MarketResponse::new(&market, None)),
    ))
}

/// Changes a market.
#[utoipa::path(
    patch,
    path = "/admin/markets/{id}",
    params(("id" = MarketId, Path)),
    request_body = UpdateMarketRequest,
    responses((status = 200, description = "OK", body = MarketResponse)),
    extensions(("x-hypha-scopes" = json!(["markets:write"]))),
)]
pub async fn update_market_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<MarketId>,
    body: JsonBody<UpdateMarketRequest>,
) -> Result<Json<MarketResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateMarket {
        name: req.name,
        prices_include_tax: req.prices_include_tax,
    };
    let market = update_market(&ctx, ports.markets(), id, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("market_not_found"))?;
    Ok(Json(changed_market(&ctx, &ports, &market).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddCountryRequest {
    country: String,
}

/// Adds a country to a market.
#[utoipa::path(
    post,
    path = "/admin/markets/{id}/countries",
    params(("id" = MarketId, Path)),
    request_body = AddCountryRequest,
    responses((status = 200, description = "OK", body = MarketResponse)),
    extensions(("x-hypha-scopes" = json!(["markets:write"]))),
)]
pub async fn add_country_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<MarketId>,
    body: JsonBody<AddCountryRequest>,
) -> Result<Json<MarketResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let market = add_market_country(&ctx, ports.markets(), id, &req.country)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("market_not_found"))?;
    Ok(Json(changed_market(&ctx, &ports, &market).await?))
}

/// Removes a country from a market.
#[utoipa::path(
    delete,
    path = "/admin/markets/{id}/countries/{country}",
    params(("id" = MarketId, Path), ("country" = CountryCode, Path)),
    responses((status = 200, description = "OK", body = MarketResponse)),
    extensions(("x-hypha-scopes" = json!(["markets:write"]))),
)]
pub async fn remove_country_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, country)): PathId<(MarketId, String)>,
) -> Result<Json<MarketResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let market = remove_market_country(&ctx, ports.markets(), id, &country)
        .await
        .map_err(|e| {
            ApiError::from(e)
                .not_found_if("market_not_found")
                .not_found_if("country_not_in_market")
        })?;
    Ok(Json(changed_market(&ctx, &ports, &market).await?))
}

// ---------- price lists ----------

/// What a list's prices mean: a `sale` is struck through against the regular price, an
/// `override` is the regular price. Either kind competes the same way: the lowest live
/// price wins.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PriceListKindDto {
    Sale,
    Override,
}

impl From<PriceListKindDto> for PriceListKind {
    fn from(k: PriceListKindDto) -> Self {
        match k {
            PriceListKindDto::Sale => PriceListKind::Sale,
            PriceListKindDto::Override => PriceListKind::Override,
        }
    }
}

impl From<PriceListKind> for PriceListKindDto {
    fn from(k: PriceListKind) -> Self {
        match k {
            PriceListKind::Sale => PriceListKindDto::Sale,
            PriceListKind::Override => PriceListKindDto::Override,
        }
    }
}

/// `draft` never prices anything, whatever its window says.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PriceListStatusDto {
    Draft,
    Active,
}

impl From<PriceListStatusDto> for PriceListStatus {
    fn from(s: PriceListStatusDto) -> Self {
        match s {
            PriceListStatusDto::Draft => PriceListStatus::Draft,
            PriceListStatusDto::Active => PriceListStatus::Active,
        }
    }
}

impl From<PriceListStatus> for PriceListStatusDto {
    fn from(s: PriceListStatus) -> Self {
        match s {
            PriceListStatus::Draft => PriceListStatusDto::Draft,
            PriceListStatus::Active => PriceListStatusDto::Active,
        }
    }
}

/// Where a list stands now, from its status and window: only `active` prices anything.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PriceListPhaseDto {
    Draft,
    /// Active, before its window opens.
    Scheduled,
    Active,
    /// Active, after its window closed.
    Expired,
}

impl From<PriceListPhase> for PriceListPhaseDto {
    fn from(p: PriceListPhase) -> Self {
        match p {
            PriceListPhase::Draft => PriceListPhaseDto::Draft,
            PriceListPhase::Scheduled => PriceListPhaseDto::Scheduled,
            PriceListPhase::Active => PriceListPhaseDto::Active,
            PriceListPhase::Expired => PriceListPhaseDto::Expired,
        }
    }
}

/// How an `override` list shows a variant's own compare-at price when its fixed price
/// has none: `adjusted` by the list's adjustment, or `hidden`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompareAtModeDto {
    Adjusted,
    Hidden,
}

impl From<CompareAtModeDto> for CompareAtMode {
    fn from(m: CompareAtModeDto) -> Self {
        match m {
            CompareAtModeDto::Adjusted => CompareAtMode::Adjusted,
            CompareAtModeDto::Hidden => CompareAtMode::Hidden,
        }
    }
}

impl From<CompareAtMode> for CompareAtModeDto {
    fn from(m: CompareAtMode) -> Self {
        match m {
            CompareAtMode::Adjusted => CompareAtModeDto::Adjusted,
            CompareAtMode::Hidden => CompareAtModeDto::Hidden,
        }
    }
}

/// A percentage applied to base prices.
#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdjustmentDto {
    /// Basis points: -1000 = 10% off, 500 = 5% markup. At least -10000.
    percent_bps: i32,
}

/// An RFC 3339 instant in a request body.
#[derive(Debug, Clone, Copy)]
pub struct Instant(Timestamp);

impl<'de> Deserialize<'de> for Instant {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse::<jiff::Timestamp>()
            .map(|t| Instant(Timestamp::from_unix_micros(t.as_microsecond())))
            .map_err(|e| serde::de::Error::custom(format!("not an RFC 3339 instant: {e}")))
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePriceListRequest {
    /// Unique, ignoring case; 1 to 255 characters, trimmed.
    name: String,
    /// Up to 255 characters; for staff.
    #[serde(default)]
    description: Option<String>,
    /// Fixed for good: catalogs and fixed prices depend on it.
    currency: Currency,
    /// Defaults to `override`.
    #[serde(default)]
    kind: Option<PriceListKindDto>,
    /// Defaults to `active`.
    #[serde(default)]
    status: Option<PriceListStatusDto>,
    /// When it starts pricing (inclusive); `null` for at once.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = DateTime)]
    starts_at: Option<Instant>,
    /// When it stops (exclusive); `null` for never. After `starts_at`.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = DateTime)]
    ends_at: Option<Instant>,
    /// Defaults to `adjusted`.
    #[serde(default)]
    compare_at: Option<CompareAtModeDto>,
    /// `null` (the default) adjusts nothing: the list prices only the variants it has
    /// fixed prices for.
    #[serde(default)]
    adjustment: Option<AdjustmentDto>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PriceListResponse {
    id: PriceListId,
    name: String,
    description: Option<String>,
    currency: Currency,
    kind: PriceListKindDto,
    status: PriceListStatusDto,
    /// Where it stands now, from `status` and the window.
    phase: PriceListPhaseDto,
    /// RFC 3339; `null` for no start.
    starts_at: Option<String>,
    /// RFC 3339; `null` for no end.
    ends_at: Option<String>,
    compare_at: CompareAtModeDto,
    /// `null` without one: the list prices only its fixed variants.
    adjustment: Option<AdjustmentDto>,
    created_at: String,
    updated_at: String,
}

impl PriceListResponse {
    fn new(l: &PriceList, now: Timestamp) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(l.stamps())?;
        let window = l.window();
        Ok(Self {
            id: l.id(),
            name: l.name().to_string(),
            description: l.description().map(str::to_string),
            currency: l.currency(),
            kind: l.kind().into(),
            status: l.status().into(),
            phase: l.phase(now).into(),
            starts_at: window.starts_at().map(rfc3339).transpose()?,
            ends_at: window.ends_at().map(rfc3339).transpose()?,
            compare_at: l.compare_at().into(),
            adjustment: l.adjustment().map(|a| AdjustmentDto {
                percent_bps: a.percent().value(),
            }),
            created_at,
            updated_at,
        })
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetFixedPricesRequest {
    /// Up to 1,000 per request; bulk callers page through.
    fixed_prices: Vec<FixedPriceDto>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FixedPriceDto {
    variant: VariantId,
    /// In the list's currency.
    price: Money,
    /// Shown struck through beside `price`: above it, in its currency. Absent or `null`
    /// for none (setting a fixed price again without one clears it).
    #[serde(default)]
    compare_at: Option<Money>,
}

/// Creates a price list. Its name is unique, ignoring case (`409 price_list_name_taken`).
#[utoipa::path(
    post,
    path = "/admin/price-lists",
    request_body = CreatePriceListRequest,
    responses((status = 201, description = "Created", body = PriceListResponse)),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn create_price_list_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreatePriceListRequest>,
) -> Result<(StatusCode, Json<PriceListResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreatePriceList {
        name: req.name,
        description: req.description,
        currency: req.currency,
        kind: req.kind.map_or(PriceListKind::Override, Into::into),
        status: req.status.map_or(PriceListStatus::Active, Into::into),
        starts_at: req.starts_at.map(|i| i.0),
        ends_at: req.ends_at.map(|i| i.0),
        compare_at: req.compare_at.map_or(CompareAtMode::Adjusted, Into::into),
        adjustment: req.adjustment.map(|a| a.percent_bps),
    };
    let list = create_price_list(&ctx, ports.price_lists(), &state.ids, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(PriceListResponse::new(&list, state.clock.now())?),
    ))
}

/// Sets fixed prices for variants in a price list: up to 1,000 at once, every entry's
/// problem answered together (`422 fixed_prices_invalid`, `details.entries`).
#[utoipa::path(
    put,
    path = "/admin/price-lists/{id}/fixed-prices",
    params(("id" = PriceListId, Path)),
    request_body = SetFixedPricesRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn set_fixed_prices_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PriceListId>,
    body: JsonBody<SetFixedPricesRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let prices = req
        .fixed_prices
        .into_iter()
        .map(|f| SetFixedPrice {
            variant: f.variant,
            price: f.price,
            compare_at: f.compare_at,
        })
        .collect();
    set_fixed_prices(&ctx, &ports, id, prices)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveFixedPricesRequest {
    variants: Vec<VariantId>,
}

/// Removes fixed prices from a price list.
#[utoipa::path(
    delete,
    path = "/admin/price-lists/{id}/fixed-prices",
    params(("id" = PriceListId, Path)),
    request_body = RemoveFixedPricesRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn remove_fixed_prices_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PriceListId>,
    body: JsonBody<RemoveFixedPricesRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    remove_fixed_prices(&ctx, &ports, id, req.variants)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- catalogs ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCatalogRequest {
    market: MarketId,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogProductsRequest {
    products: Vec<ProductId>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignPriceListRequest {
    /// `null` detaches the current price list.
    price_list: Option<PriceListId>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CatalogResponse {
    id: CatalogId,
    market: MarketId,
    price_list: Option<PriceListId>,
}

impl From<&Catalog> for CatalogResponse {
    fn from(c: &Catalog) -> Self {
        // Irrefutable while Market is the only context; a new variant makes this a compile error.
        let CatalogContext::Market(market) = c.context();
        Self {
            id: c.id(),
            market,
            price_list: c.price_list(),
        }
    }
}

/// Creates a catalog: which products a market (or all markets) sells.
#[utoipa::path(
    post,
    path = "/admin/catalogs",
    request_body = CreateCatalogRequest,
    responses((status = 201, description = "Created", body = CatalogResponse)),
    extensions(("x-hypha-scopes" = json!(["markets:write"]))),
)]
pub async fn create_catalog_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateCatalogRequest>,
) -> Result<(StatusCode, Json<CatalogResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateCatalog { market: req.market };
    let catalog =
        create_catalog(&ctx, ports.markets(), ports.catalogs(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(CatalogResponse::from(&catalog))))
}

/// Adds products to a catalog.
#[utoipa::path(
    post,
    path = "/admin/catalogs/{id}/products",
    params(("id" = CatalogId, Path)),
    request_body = CatalogProductsRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["markets:write"]))),
)]
pub async fn add_products_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CatalogId>,
    body: JsonBody<CatalogProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    add_catalog_products(&ctx, &ports, id, req.products).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes products from a catalog.
#[utoipa::path(
    delete,
    path = "/admin/catalogs/{id}/products",
    params(("id" = CatalogId, Path)),
    request_body = CatalogProductsRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["markets:write"]))),
)]
pub async fn remove_products_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CatalogId>,
    body: JsonBody<CatalogProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    remove_catalog_products(&ctx, &ports, id, req.products).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Sets or clears a catalog's price list.
#[utoipa::path(
    put,
    path = "/admin/catalogs/{id}/price-list",
    params(("id" = CatalogId, Path)),
    request_body = AssignPriceListRequest,
    responses((status = 200, description = "OK", body = CatalogResponse)),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn assign_price_list_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CatalogId>,
    body: JsonBody<AssignPriceListRequest>,
) -> Result<Json<CatalogResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let catalog = assign_price_list(
        &ctx,
        ports.markets(),
        ports.catalogs(),
        ports.price_lists(),
        id,
        req.price_list,
    )
    .await?;
    Ok(Json(CatalogResponse::from(&catalog)))
}

// ---------- reads ----------

#[derive(Serialize, utoipa::ToSchema)]
pub struct PriceListListResponse {
    /// Oldest first. A store has a handful, so they come in one list.
    items: Vec<PriceListResponse>,
}

/// Every price list, oldest first.
#[utoipa::path(
    get,
    path = "/admin/price-lists",
    responses((status = 200, description = "OK", body = PriceListListResponse)),
    extensions(("x-hypha-scopes" = json!(["pricing:read"]))),
)]
pub async fn list_price_lists_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<PriceListListResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let lists = list_price_lists(&ctx, ports.price_lists()).await?;
    let now = state.clock.now();
    Ok(Json(PriceListListResponse {
        items: lists
            .iter()
            .map(|l| PriceListResponse::new(l, now))
            .collect::<Result<_, _>>()?,
    }))
}

/// A price list.
#[utoipa::path(
    get,
    path = "/admin/price-lists/{id}",
    params(("id" = PriceListId, Path)),
    responses((status = 200, description = "OK", body = PriceListResponse)),
    extensions(("x-hypha-scopes" = json!(["pricing:read"]))),
)]
pub async fn fetch_price_list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PriceListId>,
) -> Result<Json<PriceListResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let list = get_price_list(&ctx, ports.price_lists(), id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    Ok(Json(PriceListResponse::new(&list, state.clock.now())?))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct FixedPriceResponse {
    variant: VariantId,
    price: Money,
    /// Shown struck through beside `price`; `null` for none.
    compare_at: Option<Money>,
    /// The variant's product; `null` when the variant no longer exists (fixed prices
    /// store variant IDs as given).
    product: Option<FixedPriceProduct>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct FixedPriceProduct {
    id: ProductId,
    title: String,
    /// The variant's option values ("M / Blue"); `null` for a product without options.
    variant_title: Option<String>,
}

/// A price list's changes (JSON Merge Patch). Its currency is fixed.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePriceListRequest {
    /// Unique, ignoring case.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = PriceListKindDto, required = false)]
    kind: Option<PriceListKindDto>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = PriceListStatusDto, required = false)]
    status: Option<PriceListStatusDto>,
    /// RFC 3339; `null` clears it. The window is checked as it ends up.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, format = DateTime, required = false)]
    starts_at: Option<Option<Instant>>,
    /// RFC 3339; `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, format = DateTime, required = false)]
    ends_at: Option<Option<Instant>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = CompareAtModeDto, required = false)]
    compare_at: Option<CompareAtModeDto>,
    /// `null` removes it: the list then prices only its fixed variants.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<AdjustmentDto>, required = false)]
    adjustment: Option<Option<AdjustmentDto>>,
}

/// Changes a price list; its currency is fixed.
#[utoipa::path(
    patch,
    path = "/admin/price-lists/{id}",
    params(("id" = PriceListId, Path)),
    request_body = UpdatePriceListRequest,
    responses((status = 200, description = "OK", body = PriceListResponse)),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn update_price_list_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PriceListId>,
    body: JsonBody<UpdatePriceListRequest>,
) -> Result<Json<PriceListResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = hypha_markets::UpdatePriceList {
        name: req.name,
        description: req.description,
        kind: req.kind.map(Into::into),
        status: req.status.map(Into::into),
        starts_at: req.starts_at.map(|t| t.map(|i| i.0)),
        ends_at: req.ends_at.map(|t| t.map(|i| i.0)),
        compare_at: req.compare_at.map(Into::into),
        adjustment: req.adjustment.map(|a| a.map(|a| a.percent_bps)),
    };
    let list = update_price_list(&ctx, ports.price_lists(), id, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    Ok(Json(PriceListResponse::new(&list, state.clock.now())?))
}

type FixedPricePage = PageResponse<FixedPriceResponse>;

/// A price list's fixed prices, newest variant first.
#[utoipa::path(
    get,
    path = "/admin/price-lists/{id}/fixed-prices",
    params(("id" = PriceListId, Path), PageParams),
    responses((status = 200, description = "OK", body = FixedPricePage)),
    extensions(("x-hypha-scopes" = json!(["pricing:read", "products:read"]))),
)]
pub async fn list_fixed_prices_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PriceListId>,
    query: ListQuery,
) -> Result<Json<FixedPricePage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, _, scope) = query.parse()?;
    let page = list_fixed_prices(&ctx, ports.price_lists(), id, page)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    let variants: Vec<VariantId> = page.items.iter().map(|(v, _)| *v).collect();
    let products = products_of_variants(&ctx, ports.products(), &variants).await?;
    // Each variant's product, looked up once per row rather than scanned for.
    let owners: HashMap<VariantId, &hypha_products::Product> = products
        .iter()
        .flat_map(|p| p.variants().iter().map(move |v| (v.id(), p)))
        .collect();
    let product_of = |variant: VariantId| {
        owners.get(&variant).map(|p| FixedPriceProduct {
            id: p.id(),
            title: p.title().to_string(),
            variant_title: p.variant_title(variant),
        })
    };
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |(variant, _)| *variant,
        |(variant, fixed)| {
            Ok(FixedPriceResponse {
                variant,
                price: fixed.price().money(),
                compare_at: fixed.compare_at().map(|p| p.money()),
                product: product_of(variant),
            })
        },
    )?))
}

/// A catalog.
#[utoipa::path(
    get,
    path = "/admin/catalogs/{id}",
    params(("id" = CatalogId, Path)),
    responses((status = 200, description = "OK", body = CatalogResponse)),
    extensions(("x-hypha-scopes" = json!(["markets:read"]))),
)]
pub async fn fetch_catalog<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CatalogId>,
) -> Result<Json<CatalogResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let catalog = get_catalog(&ctx, ports.catalogs(), id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("catalog_not_found"))?;
    Ok(Json(CatalogResponse::from(&catalog)))
}

/// A product in a catalog. Catalogs store product IDs as given, so the product's fields
/// are `null` for an ID that names no product.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CatalogProductResponse {
    id: ProductId,
    title: Option<String>,
    handle: Option<String>,
    status: Option<StatusDto>,
}

type CatalogProductPage = PageResponse<CatalogProductResponse>;

/// A catalog's products, newest first.
#[utoipa::path(
    get,
    path = "/admin/catalogs/{id}/products",
    params(("id" = CatalogId, Path), PageParams),
    responses((status = 200, description = "OK", body = CatalogProductPage)),
    extensions(("x-hypha-scopes" = json!(["markets:read", "products:read"]))),
)]
pub async fn list_catalog_products_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CatalogId>,
    query: ListQuery,
) -> Result<Json<CatalogProductPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, _, scope) = query.parse()?;
    let page = list_catalog_products(&ctx, ports.catalogs(), id, page)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("catalog_not_found"))?;
    let mut products: HashMap<ProductId, _> = get_products(&ctx, ports.products(), &page.items)
        .await?
        .into_iter()
        .map(|p| (p.id(), p))
        .collect();
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |id| *id,
        |id| {
            let product = products.remove(&id);
            Ok(CatalogProductResponse {
                id,
                title: product.as_ref().map(|p| p.title().to_string()),
                handle: product.as_ref().map(|p| p.handle().as_str().to_string()),
                status: product.as_ref().map(|p| p.status().into()),
            })
        },
    )?))
}

// ---------- storefront ----------

/// A country a market sells to, with what a storefront needs to show prices there.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StorefrontCountryResponse {
    country: CountryCode,
    /// The English name; UIs show local names (`Intl.DisplayNames`).
    name: &'static str,
    market: MarketId,
    currency: Currency,
    /// Digits after the decimal point of the currency (2 for EUR, 0 for JPY).
    #[schema(minimum = 0)]
    minor_units: u8,
    prices_include_tax: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct StorefrontCountryListResponse {
    /// Sorted by country code: every country some market sells to.
    items: Vec<StorefrontCountryResponse>,
}

/// Where the store sells: each country with its market's currency. What a country picker
/// and a storefront's routing need.
#[utoipa::path(
    get,
    path = "/storefront/countries",
    responses((status = 200, description = "OK", body = StorefrontCountryListResponse)),
    extensions(("x-hypha-scopes" = json!(["markets:read"]))),
)]
pub async fn storefront_countries<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
) -> Result<Json<StorefrontCountryListResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let markets = list_markets_only(&ctx, ports.markets()).await?;
    let mut items: Vec<StorefrontCountryResponse> = markets
        .iter()
        .flat_map(|m| {
            m.countries().iter().map(|c| StorefrontCountryResponse {
                country: *c,
                name: c.name(),
                market: m.id(),
                currency: m.currency(),
                minor_units: m.currency().minor_units(),
                prices_include_tax: m.prices_include_tax(),
            })
        })
        .collect();
    items.sort_by_key(|i| i.country.as_str());
    Ok(Json(StorefrontCountryListResponse { items }))
}
