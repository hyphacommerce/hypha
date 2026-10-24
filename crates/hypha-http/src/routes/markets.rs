//! Markets, price lists and catalogs (all live in `hypha-markets`).

use std::collections::HashMap;

use axum::{Json, extract::State, http::StatusCode};
use hypha_cart::explain_product_prices;
use hypha_core::{
    CatalogId, CategoryId, Clock, CountryCode, Ctx, Currency, CustomerGroupId, ErrorCode, Keyset,
    MarketId, Money, PriceListId, ProductId, Quantity, SortKey, Stamped, Timestamp, VariantId,
};
use hypha_customers::{CustomerGroupRepo, HasCustomerGroups};
use hypha_markets::{
    AdjustmentInput, AppliesTo, Catalog, CatalogContext, CatalogRepo, CompareAtMode, CreateCatalog,
    CreateMarket, CreatePriceList, FixedPriceFilter, HasCatalogs, HasMarkets, HasPriceLists,
    ListPriceLists, Market, MarketView, PriceBreak, PriceList, PriceListKind, PriceListPhase,
    PriceListRepo, PriceListSort, PriceListStatus, SetFixedPrice, UpdateMarket,
    add_catalog_price_list, add_catalog_products, add_market_country, create_catalog,
    create_market, create_price_list, delete_price_list, get_catalog, get_market, get_price_list,
    list_catalog_products, list_fixed_prices, list_markets, list_markets_only, list_price_lists,
    remove_catalog_price_list, remove_catalog_products, remove_fixed_prices, remove_market_country,
    set_catalog_price_lists, set_fixed_prices, update_market, update_price_list,
};
use hypha_pricing::CandidateResult;
use hypha_products::{HasProducts, get_products, products_of_variants};
use serde::{Deserialize, Serialize};

use super::{
    page::{ListQuery, PageParams, PageResponse, SortedFilters},
    products::StatusDto,
    rfc3339, stamp_times,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId, Query, Storefront},
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
    /// The catalog's price lists, by ID: the lowest live price among them wins.
    price_lists: Vec<PriceListId>,
}

impl MarketResponse {
    fn new(m: &Market, catalog: Option<CatalogId>, price_lists: Vec<PriceListId>) -> Self {
        Self {
            id: m.id(),
            name: m.name().to_string(),
            countries: m.countries().to_vec(),
            currency: m.currency(),
            prices_include_tax: m.prices_include_tax(),
            catalog,
            price_lists,
        }
    }
}

impl From<&MarketView> for MarketResponse {
    fn from(v: &MarketView) -> Self {
        Self::new(&v.market, v.catalog, v.price_lists.clone())
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
    Ok(MarketResponse::new(market, view.catalog, view.price_lists))
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
        Json(MarketResponse::new(&market, None, Vec::new())),
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

/// A percentage applied to base prices, for the products it applies to.
#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdjustmentDto {
    /// Basis points: -1000 = 10% off, 500 = 5% markup. At least -10000.
    percent_bps: i32,
    /// Defaults to every product of the catalog.
    #[serde(default)]
    applies_to: AppliesToDto,
}

/// Which products an adjustment reaches: `all` of the catalog's, listed `products`, or
/// the products in listed `categories` and the categories below them. Up to 1,000 IDs,
/// each once; they aren't checked (one that names nothing reaches nothing).
#[derive(Debug, Clone, Default, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AppliesToDto {
    #[default]
    All,
    Products {
        products: Vec<ProductId>,
    },
    Categories {
        categories: Vec<CategoryId>,
    },
}

impl From<AppliesToDto> for AppliesTo {
    fn from(a: AppliesToDto) -> Self {
        match a {
            AppliesToDto::All => AppliesTo::All,
            AppliesToDto::Products { products } => AppliesTo::Products(products),
            AppliesToDto::Categories { categories } => AppliesTo::Categories(categories),
        }
    }
}

impl From<&AppliesTo> for AppliesToDto {
    fn from(a: &AppliesTo) -> Self {
        match a {
            AppliesTo::All => AppliesToDto::All,
            AppliesTo::Products(products) => AppliesToDto::Products {
                products: products.clone(),
            },
            AppliesTo::Categories(categories) => AppliesToDto::Categories {
                categories: categories.clone(),
            },
        }
    }
}

impl From<AdjustmentDto> for AdjustmentInput {
    fn from(a: AdjustmentDto) -> Self {
        AdjustmentInput {
            percent: a.percent_bps,
            applies_to: a.applies_to.into(),
        }
    }
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
    /// Who it prices for; absent for everyone.
    #[serde(default)]
    audience: Option<AudienceDto>,
}

/// Who a price list prices for: buyers signed in as a customer in any of `groups` (up to
/// 100, each existing, `422 customer_group_not_found`), or everyone when it's empty. A
/// list for groups never prices search documents or a shopper who isn't signed in.
#[derive(Debug, Clone, Default, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AudienceDto {
    groups: Vec<CustomerGroupId>,
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
    /// Its customer groups, sorted; none for everyone.
    audience: AudienceDto,
    /// The markets whose catalogs hold it.
    markets: Vec<MarketId>,
    /// How many fixed prices it has.
    #[schema(minimum = 0)]
    fixed_price_count: u64,
    created_at: String,
    updated_at: String,
}

/// `lists` as responses: each with the markets holding it and its count of fixed prices,
/// read once for them all.
async fn price_list_responses<P: HasCatalogs + HasPriceLists>(
    ports: &P,
    lists: &[PriceList],
    now: Timestamp,
) -> Result<Vec<PriceListResponse>, ApiError> {
    let catalogs = ports
        .catalogs()
        .list_all()
        .await
        .map_err(hypha_markets::CommandError::from)?;
    let ids: Vec<PriceListId> = lists.iter().map(|l| l.id()).collect();
    let counts = ports
        .price_lists()
        .fixed_price_counts(&ids)
        .await
        .map_err(hypha_markets::CommandError::from)?;
    lists
        .iter()
        .map(|l| {
            let markets = catalogs
                .iter()
                .filter(|c| c.price_lists().contains(&l.id()))
                .map(|c| {
                    let CatalogContext::Market(market) = c.context();
                    market
                })
                .collect();
            PriceListResponse::new(l, now, markets, counts.get(&l.id()).copied().unwrap_or(0))
        })
        .collect()
}

async fn price_list_response<P: HasCatalogs + HasPriceLists>(
    ports: &P,
    list: &PriceList,
    now: Timestamp,
) -> Result<PriceListResponse, ApiError> {
    let mut found = price_list_responses(ports, std::slice::from_ref(list), now).await?;
    found
        .pop()
        .ok_or_else(|| ApiError::internal("a price list without its response"))
}

impl PriceListResponse {
    fn new(
        l: &PriceList,
        now: Timestamp,
        markets: Vec<MarketId>,
        fixed_price_count: u64,
    ) -> Result<Self, ApiError> {
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
                applies_to: a.applies_to().into(),
            }),
            audience: AudienceDto {
                groups: l.audience().groups().to_vec(),
            },
            markets,
            fixed_price_count,
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
    /// Quantity prices: up to 10, from 2 up with quantities increasing, each price below
    /// the one before it (the first below `price`). Absent for none; setting a fixed price
    /// again replaces them.
    #[serde(default)]
    breaks: Vec<PriceBreakDto>,
}

/// From `min_quantity` of the variant in one cart line, each costs `price`.
#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PriceBreakDto {
    #[schema(minimum = 2)]
    min_quantity: u32,
    price: Money,
}

impl From<&PriceBreak> for PriceBreakDto {
    fn from(b: &PriceBreak) -> Self {
        Self {
            min_quantity: b.min_quantity,
            price: b.price.money(),
        }
    }
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
        adjustment: req.adjustment.map(Into::into),
        groups: req.audience.unwrap_or_default().groups,
    };
    let list = create_price_list(&ctx, ports.price_lists(), &state.ids, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(price_list_response(&ports, &list, state.clock.now()).await?),
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
            breaks: f
                .breaks
                .into_iter()
                .map(|b| (b.min_quantity, b.price))
                .collect(),
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
pub struct SetCatalogPriceListsRequest {
    /// The catalog's lists after the change, each once, at most 50, all in the market's
    /// currency; `[]` takes them all off.
    price_lists: Vec<PriceListId>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddCatalogPriceListRequest {
    /// In the market's currency.
    price_list: PriceListId,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CatalogResponse {
    id: CatalogId,
    market: MarketId,
    /// Its price lists, by ID: the lowest live price among them wins.
    price_lists: Vec<PriceListId>,
}

impl From<&Catalog> for CatalogResponse {
    fn from(c: &Catalog) -> Self {
        // Irrefutable while Market is the only context; a new variant makes this a compile error.
        let CatalogContext::Market(market) = c.context();
        Self {
            id: c.id(),
            market,
            price_lists: c.price_lists().to_vec(),
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

/// Sets a catalog's price lists, which compete to price its products: the lowest live
/// price wins, a tie going to the oldest list. Every list is in the market's currency
/// (`422 price_list_currency_mismatch`).
#[utoipa::path(
    put,
    path = "/admin/catalogs/{id}/price-lists",
    params(("id" = CatalogId, Path)),
    request_body = SetCatalogPriceListsRequest,
    responses((status = 200, description = "OK", body = CatalogResponse)),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn set_catalog_price_lists_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CatalogId>,
    body: JsonBody<SetCatalogPriceListsRequest>,
) -> Result<Json<CatalogResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let catalog = set_catalog_price_lists(&ctx, &ports, id, req.price_lists)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("catalog_not_found"))?;
    Ok(Json(CatalogResponse::from(&catalog)))
}

/// Adds a price list to a catalog's; one already there changes nothing.
#[utoipa::path(
    post,
    path = "/admin/catalogs/{id}/price-lists",
    params(("id" = CatalogId, Path)),
    request_body = AddCatalogPriceListRequest,
    responses((status = 200, description = "OK", body = CatalogResponse)),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn add_catalog_price_list_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CatalogId>,
    body: JsonBody<AddCatalogPriceListRequest>,
) -> Result<Json<CatalogResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let catalog = add_catalog_price_list(&ctx, &ports, id, req.price_list)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("catalog_not_found"))?;
    Ok(Json(CatalogResponse::from(&catalog)))
}

/// Takes a price list off a catalog.
#[utoipa::path(
    delete,
    path = "/admin/catalogs/{id}/price-lists/{price_list}",
    params(("id" = CatalogId, Path), ("price_list" = PriceListId, Path)),
    responses((status = 200, description = "OK", body = CatalogResponse)),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn remove_catalog_price_list_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, list)): PathId<(CatalogId, PriceListId)>,
) -> Result<Json<CatalogResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let catalog = remove_catalog_price_list(&ctx, &ports, id, list)
        .await
        .map_err(|e| {
            ApiError::from(e)
                .not_found_if("catalog_not_found")
                .not_found_if("price_list_not_in_catalog")
        })?;
    Ok(Json(CatalogResponse::from(&catalog)))
}

// ---------- reads ----------

/// What the price lists are narrowed to.
#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct PriceListFilters {
    #[param(inline)]
    kind: Option<PriceListKindDto>,
    /// Where they stand now: `draft`, `scheduled` (active, not started), `active`, or
    /// `expired` (active, ended).
    #[param(inline)]
    status: Option<PriceListPhaseDto>,
    /// Only those on this market's catalog.
    market: Option<MarketId>,
    /// Only those whose audience names this customer group.
    #[param(value_type = Option<CustomerGroupId>)]
    group: Option<CustomerGroupId>,
    #[param(inline)]
    sort: Option<PriceListSortDto>,
}

/// The order of the list. Default `newest`. Names compare lowercased, character by
/// character; ties go by creation, in the same direction.
#[derive(Debug, Clone, Copy, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PriceListSortDto {
    Newest,
    Oldest,
    NameAsc,
    NameDesc,
}

impl PriceListFilters {
    fn sort(&self) -> PriceListSort {
        match self.sort {
            None | Some(PriceListSortDto::Newest) => PriceListSort::Newest,
            Some(PriceListSortDto::Oldest) => PriceListSort::Oldest,
            Some(PriceListSortDto::NameAsc) => PriceListSort::NameAsc,
            Some(PriceListSortDto::NameDesc) => PriceListSort::NameDesc,
        }
    }
}

impl SortedFilters for PriceListFilters {
    fn fits(&self, key: &SortKey) -> bool {
        self.sort().fits(key)
    }
}

impl From<PriceListPhaseDto> for PriceListPhase {
    fn from(p: PriceListPhaseDto) -> Self {
        match p {
            PriceListPhaseDto::Draft => PriceListPhase::Draft,
            PriceListPhaseDto::Scheduled => PriceListPhase::Scheduled,
            PriceListPhaseDto::Active => PriceListPhase::Active,
            PriceListPhaseDto::Expired => PriceListPhase::Expired,
        }
    }
}

type PriceListPage = PageResponse<PriceListResponse>;

/// Price lists, newest first or in `sort`'s order, narrowed by kind, status and market.
#[utoipa::path(
    get,
    path = "/admin/price-lists",
    params(PageParams, PriceListFilters),
    responses((status = 200, description = "OK", body = PriceListPage)),
    extensions(("x-hypha-scopes" = json!(["pricing:read"]))),
)]
pub async fn list_price_lists_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<PriceListFilters>,
) -> Result<Json<PriceListPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse_sorted()?;
    let sort = filters.sort();
    let input = ListPriceLists {
        kind: filters.kind.map(Into::into),
        phase: filters.status.map(Into::into),
        market: filters.market,
        group: filters.group,
        sort,
    };
    let now = state.clock.now();
    let page = list_price_lists(&ctx, &ports, input, now, page).await?;
    let items = price_list_responses(&ports, &page.items, now).await?;
    Ok(Json(PageResponse::sorted(
        &page,
        &scope,
        |l| Keyset {
            key: sort.key(l),
            id: l.id(),
        },
        items,
    )?))
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
    Ok(Json(
        price_list_response(&ports, &list, state.clock.now()).await?,
    ))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct FixedPriceResponse {
    variant: VariantId,
    price: Money,
    /// Shown struck through beside `price`; `null` for none.
    compare_at: Option<Money>,
    /// Its quantity prices, by quantity.
    breaks: Vec<PriceBreakDto>,
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
    /// Replaces the audience; `{"groups": []}` for everyone.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = AudienceDto, required = false)]
    audience: Option<AudienceDto>,
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
        adjustment: req.adjustment.map(|a| a.map(Into::into)),
        groups: req.audience.map(|a| a.groups),
    };
    let list = update_price_list(&ctx, ports.price_lists(), id, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    Ok(Json(
        price_list_response(&ports, &list, state.clock.now()).await?,
    ))
}

/// Deletes a price list with its fixed prices. One a catalog holds is refused
/// (`409 price_list_in_use`, `details.catalogs`): take it off them first.
#[utoipa::path(
    delete,
    path = "/admin/price-lists/{id}",
    params(("id" = PriceListId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub async fn delete_price_list_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PriceListId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_price_list(&ctx, &ports, id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

type FixedPricePage = PageResponse<FixedPriceResponse>;

/// What a list's fixed prices are narrowed to.
#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct FixedPriceFilters {
    /// Only this product's variants.
    product: Option<ProductId>,
    /// Only variants of products in this category or any category below it, as a price
    /// list's adjustment for a category reaches them.
    category: Option<CategoryId>,
    /// Only variants whose product's title or handle, or whose SKU, contains this text,
    /// ignoring case. Up to 255 characters; blank filters nothing.
    q: Option<String>,
}

/// A price list's fixed prices, newest variant first, narrowed by product, category or
/// text: what a price grid pages through.
#[utoipa::path(
    get,
    path = "/admin/price-lists/{id}/fixed-prices",
    params(("id" = PriceListId, Path), PageParams, FixedPriceFilters),
    responses((status = 200, description = "OK", body = FixedPricePage)),
    extensions(("x-hypha-scopes" = json!(["pricing:read", "products:read"]))),
)]
pub async fn list_fixed_prices_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PriceListId>,
    query: ListQuery<FixedPriceFilters>,
) -> Result<Json<FixedPricePage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse()?;
    let filter = FixedPriceFilter::new(filters.product, filters.category, filters.q)
        .map_err(hypha_markets::CommandError::from)?;
    let page = list_fixed_prices(&ctx, ports.price_lists(), id, &filter, page)
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
                breaks: fixed.breaks().iter().map(PriceBreakDto::from).collect(),
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

// ---------- a product's prices ----------

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ProductPricesParams {
    /// Only this market; every market without it.
    market: Option<MarketId>,
    /// How many of each variant, for quantity prices; 1 without it.
    #[param(minimum = 1)]
    quantity: Option<u32>,
    /// Priced for a customer in this group (`404 customer_group_not_found`); without it,
    /// for a shopper who isn't signed in.
    #[param(value_type = Option<CustomerGroupId>)]
    group: Option<CustomerGroupId>,
}

/// What became of one price list for a variant.
#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CandidateResultDto {
    /// Its price is the one paid.
    Won,
    /// Another list's price was lower.
    Higher,
    /// Another list had the same price and is older.
    TiedWithOlder,
    /// Draft, not started yet, or ended (`phase` says which).
    NotLive,
    /// Live, but for customer groups the buyer (the `group` asked about) isn't in.
    NotInAudience,
    /// No fixed price for the variant, and no adjustment reaching its product.
    NotPricing,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PriceCandidateResponse {
    price_list: PriceListId,
    name: String,
    kind: PriceListKindDto,
    phase: PriceListPhaseDto,
    /// What it would charge; `null` unless it's live and prices the variant.
    price: Option<Money>,
    result: CandidateResultDto,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PriceProblemResponse {
    /// Stable `ErrorCode`, e.g. `not_available_in_market`.
    code: &'static str,
    message: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct VariantPricesResponse {
    variant: VariantId,
    /// What a shopper pays for one at `quantity`; `null` when it can't be priced there
    /// (`problem` says why).
    price: Option<Money>,
    /// Shown struck through beside `price`; only ever above it.
    compare_at_price: Option<Money>,
    /// Whether a compare-at price is shown.
    on_sale: bool,
    /// The list whose price won; `null` for the base price.
    price_list: Option<PriceListId>,
    /// What lines larger than `quantity` cost each, across every list: by quantity, each
    /// below `price` and the one before it, a line of `min_quantity` paying its `price`.
    price_breaks: Vec<PriceBreakDto>,
    problem: Option<PriceProblemResponse>,
    /// Every list of the market's catalog, in ID order (oldest first).
    candidates: Vec<PriceCandidateResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MarketPricesResponse {
    market: MarketId,
    currency: Currency,
    prices_include_tax: bool,
    /// In the product's variant order.
    variants: Vec<VariantPricesResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ProductPricesResponse {
    product: ProductId,
    quantity: u32,
    /// Oldest market first.
    markets: Vec<MarketPricesResponse>,
}

/// A product's prices in each market (or one), each variant's with every price list of
/// the market's catalog and why it won or lost: the lowest live price wins, a tie going
/// to the oldest list, and the base price only when no live list prices the variant. Lists
/// for customer groups price only for a buyer in one (`group`). A product that isn't
/// active is priced too.
#[utoipa::path(
    get,
    path = "/admin/products/{id}/prices",
    params(("id" = ProductId, Path), ProductPricesParams),
    responses((status = 200, description = "OK", body = ProductPricesResponse)),
    extensions(("x-hypha-scopes" = json!(["pricing:read", "products:read"]))),
)]
pub async fn product_prices_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    query: Query<ProductPricesParams>,
) -> Result<Json<ProductPricesResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let params = query.parse()?;
    let quantity =
        Quantity::new(params.quantity.unwrap_or(1)).map_err(hypha_cart::CommandError::from)?;
    let product = hypha_products::get_product(&ctx, ports.products(), id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("product_not_found"))?;
    let groups = match params.group {
        Some(group) => {
            let found = ports
                .customer_groups()
                .get(group)
                .await
                .map_err(hypha_customers::CommandError::from)?;
            if found.is_none() {
                return Err(
                    ApiError::from(hypha_customers::CommandError::GroupNotFound(group))
                        .not_found_if("customer_group_not_found"),
                );
            }
            vec![group]
        }
        None => Vec::new(),
    };
    let markets = explain_product_prices(&ctx, &ports, &product, params.market, quantity, &groups)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("market_not_found"))?;
    let now = state.clock.now();
    let markets = markets
        .iter()
        .map(|m| MarketPricesResponse {
            market: m.market.id(),
            currency: m.market.currency(),
            prices_include_tax: m.market.prices_include_tax(),
            variants: m
                .variants
                .iter()
                .map(|(variant, explained)| {
                    let resolved = explained.resolved.as_ref().ok();
                    VariantPricesResponse {
                        variant: *variant,
                        price: resolved.map(|r| r.price.money()),
                        compare_at_price: resolved.and_then(|r| r.compare_at).map(|p| p.money()),
                        on_sale: resolved.is_some_and(|r| r.on_sale()),
                        price_list: resolved.and_then(|r| r.price_list),
                        price_breaks: resolved
                            .map(|r| r.breaks.iter().map(PriceBreakDto::from).collect())
                            .unwrap_or_default(),
                        problem: explained
                            .resolved
                            .as_ref()
                            .err()
                            .map(|e| PriceProblemResponse {
                                code: e.code(),
                                message: e.to_string(),
                            }),
                        candidates: explained
                            .candidates
                            .iter()
                            .filter_map(|c| {
                                let list = m.price_lists.iter().find(|l| l.id() == c.price_list)?;
                                Some(PriceCandidateResponse {
                                    price_list: c.price_list,
                                    name: list.name().to_string(),
                                    kind: list.kind().into(),
                                    phase: list.phase(now).into(),
                                    price: c.price.map(|p| p.money()),
                                    result: match c.result {
                                        CandidateResult::Won => CandidateResultDto::Won,
                                        CandidateResult::Higher => CandidateResultDto::Higher,
                                        CandidateResult::TiedWithOlder => {
                                            CandidateResultDto::TiedWithOlder
                                        }
                                        CandidateResult::NotLive(_) => CandidateResultDto::NotLive,
                                        CandidateResult::NotInAudience => {
                                            CandidateResultDto::NotInAudience
                                        }
                                        CandidateResult::NotPricing => {
                                            CandidateResultDto::NotPricing
                                        }
                                    },
                                })
                            })
                            .collect(),
                    }
                })
                .collect(),
        })
        .collect();
    Ok(Json(ProductPricesResponse {
        product: id,
        quantity: quantity.value(),
        markets,
    }))
}
