//! Markets, price lists and catalogs (all live in `hypha-markets`).

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{
    CatalogId, CountryCode, Currency, MarketId, Money, PriceListId, ProductId, VariantId,
};
use hypha_markets::{
    Catalog, CatalogContext, CreateCatalog, CreateMarket, CreatePriceList, FixedPrice, HasCatalogs,
    HasMarkets, HasPriceLists, Market, PriceList, UpdateMarket, add_catalog_products,
    add_market_country, assign_price_list, create_catalog, create_market, create_price_list,
    remove_catalog_products, remove_fixed_prices, remove_market_country, set_fixed_prices,
    update_market,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

// ---------- markets ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateMarketRequest {
    name: String,
    countries: Vec<String>,
    currency: Currency,
    #[serde(default)]
    prices_include_tax: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateMarketRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    prices_include_tax: Option<bool>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct MarketResponse {
    id: MarketId,
    name: String,
    countries: Vec<CountryCode>,
    currency: Currency,
    prices_include_tax: bool,
}

impl From<&Market> for MarketResponse {
    fn from(m: &Market) -> Self {
        Self {
            id: m.id(),
            name: m.name().to_string(),
            countries: m.countries().to_vec(),
            currency: m.currency(),
            prices_include_tax: m.prices_include_tax(),
        }
    }
}

/// Creates a market: countries that share a currency.
#[utoipa::path(
    post,
    path = "/admin/markets",
    request_body = CreateMarketRequest,
    responses((status = 201, description = "Created", body = MarketResponse)),
    extensions(("x-hypha-scopes" = json!(["markets:write"]))),
)]
pub(super) async fn create_market_handler<S: Storage>(
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
    Ok((StatusCode::CREATED, Json(MarketResponse::from(&market))))
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
pub(super) async fn update_market_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<MarketId>,
    body: JsonBody<UpdateMarketRequest>,
) -> Result<Json<MarketResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateMarket {
        prices_include_tax: req.prices_include_tax,
    };
    let market = update_market(&ctx, ports.markets(), id, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("market_not_found"))?;
    Ok(Json(MarketResponse::from(&market)))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct AddCountryRequest {
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
pub(super) async fn add_country_handler<S: Storage>(
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
    Ok(Json(MarketResponse::from(&market)))
}

/// Removes a country from a market.
#[utoipa::path(
    delete,
    path = "/admin/markets/{id}/countries/{country}",
    params(("id" = MarketId, Path), ("country" = CountryCode, Path)),
    responses((status = 200, description = "OK", body = MarketResponse)),
    extensions(("x-hypha-scopes" = json!(["markets:write"]))),
)]
pub(super) async fn remove_country_handler<S: Storage>(
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
    Ok(Json(MarketResponse::from(&market)))
}

// ---------- price lists ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreatePriceListRequest {
    currency: Currency,
    /// Basis points: -1000 = 10% off. Defaults to 0.
    #[serde(default)]
    adjustment_bps: i32,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct PriceListResponse {
    id: PriceListId,
    currency: Currency,
    adjustment_bps: i32,
}

impl From<&PriceList> for PriceListResponse {
    fn from(l: &PriceList) -> Self {
        Self {
            id: l.id(),
            currency: l.currency(),
            adjustment_bps: l.adjustment().value(),
        }
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SetFixedPricesRequest {
    fixed_prices: Vec<FixedPriceDto>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FixedPriceDto {
    variant: VariantId,
    price: Money,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct RemoveFixedPricesRequest {
    variants: Vec<VariantId>,
}

/// Creates a price list.
#[utoipa::path(
    post,
    path = "/admin/price-lists",
    request_body = CreatePriceListRequest,
    responses((status = 201, description = "Created", body = PriceListResponse)),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub(super) async fn create_price_list_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreatePriceListRequest>,
) -> Result<(StatusCode, Json<PriceListResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreatePriceList {
        currency: req.currency,
        adjustment: req.adjustment_bps,
    };
    let list = create_price_list(&ctx, ports.price_lists(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(PriceListResponse::from(&list))))
}

/// Sets fixed prices for variants in a price list.
#[utoipa::path(
    put,
    path = "/admin/price-lists/{id}/fixed-prices",
    params(("id" = PriceListId, Path)),
    request_body = SetFixedPricesRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["pricing:write"]))),
)]
pub(super) async fn set_fixed_prices_handler<S: Storage>(
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
        .map(|f| FixedPrice {
            variant: f.variant,
            price: f.price,
        })
        .collect();
    set_fixed_prices(&ctx, ports.price_lists(), id, prices)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
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
pub(super) async fn remove_fixed_prices_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PriceListId>,
    body: JsonBody<RemoveFixedPricesRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    remove_fixed_prices(&ctx, ports.price_lists(), id, req.variants)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- catalogs ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateCatalogRequest {
    market: MarketId,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogProductsRequest {
    products: Vec<ProductId>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct AssignPriceListRequest {
    /// `null` detaches the current price list.
    price_list: Option<PriceListId>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct CatalogResponse {
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
pub(super) async fn create_catalog_handler<S: Storage>(
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
pub(super) async fn add_products_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CatalogId>,
    body: JsonBody<CatalogProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    add_catalog_products(&ctx, ports.catalogs(), id, req.products).await?;
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
pub(super) async fn remove_products_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CatalogId>,
    body: JsonBody<CatalogProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    remove_catalog_products(&ctx, ports.catalogs(), id, req.products).await?;
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
pub(super) async fn assign_price_list_handler<S: Storage>(
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
