//! Markets, price lists and catalogs (all live in `hypha-markets`).

use axum::{Json, extract::State, http::StatusCode};
use hypha_cart::CartPorts;
use hypha_core::{
    CatalogId, CountryCode, Ctx, Currency, MarketId, Money, PriceListId, ProductId, VariantId,
};
use hypha_markets::{
    Catalog, CatalogContext, CreateCatalog, CreateMarket, CreatePriceList, FixedPrice, Market,
    PriceList, UpdateMarket, add_catalog_products, assign_price_list, create_catalog,
    create_market, create_price_list, remove_catalog_products, remove_fixed_prices,
    set_fixed_prices, update_market,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

// ---------- markets ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateMarketRequest {
    name: String,
    countries: Vec<String>,
    currency: Currency,
    #[serde(default)]
    prices_include_tax: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateMarketRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    prices_include_tax: Option<bool>,
}

#[derive(Serialize)]
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

pub(super) async fn create_market_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    JsonBody(req): JsonBody<CreateMarketRequest>,
) -> Result<(StatusCode, Json<MarketResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = CreateMarket {
        name: req.name,
        countries: req.countries,
        currency: req.currency,
        prices_include_tax: req.prices_include_tax,
    };
    let market = create_market(&ctx, ports.markets(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(MarketResponse::from(&market))))
}

pub(super) async fn update_market_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<MarketId>,
    JsonBody(req): JsonBody<UpdateMarketRequest>,
) -> Result<Json<MarketResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = UpdateMarket {
        prices_include_tax: req.prices_include_tax,
    };
    let market = update_market(&ctx, ports.markets(), id, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("market_not_found"))?;
    Ok(Json(MarketResponse::from(&market)))
}

// ---------- price lists ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreatePriceListRequest {
    currency: Currency,
    /// Basis points: -1000 = 10% off. Defaults to 0.
    #[serde(default)]
    adjustment_bps: i32,
}

#[derive(Serialize)]
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetFixedPricesRequest {
    fixed_prices: Vec<FixedPriceDto>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FixedPriceDto {
    variant: VariantId,
    price: Money,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RemoveFixedPricesRequest {
    variants: Vec<VariantId>,
}

pub(super) async fn create_price_list_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    JsonBody(req): JsonBody<CreatePriceListRequest>,
) -> Result<(StatusCode, Json<PriceListResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = CreatePriceList {
        currency: req.currency,
        adjustment: req.adjustment_bps,
    };
    let list = create_price_list(&ctx, ports.price_lists(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(PriceListResponse::from(&list))))
}

pub(super) async fn set_fixed_prices_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<PriceListId>,
    JsonBody(req): JsonBody<SetFixedPricesRequest>,
) -> Result<StatusCode, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
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

pub(super) async fn remove_fixed_prices_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<PriceListId>,
    JsonBody(req): JsonBody<RemoveFixedPricesRequest>,
) -> Result<StatusCode, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    remove_fixed_prices(&ctx, ports.price_lists(), id, req.variants)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- catalogs ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateCatalogRequest {
    market: MarketId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogProductsRequest {
    products: Vec<ProductId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AssignPriceListRequest {
    /// `null` detaches the current price list.
    price_list: Option<PriceListId>,
}

#[derive(Serialize)]
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

pub(super) async fn create_catalog_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    JsonBody(req): JsonBody<CreateCatalogRequest>,
) -> Result<(StatusCode, Json<CatalogResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = CreateCatalog { market: req.market };
    let catalog =
        create_catalog(&ctx, ports.markets(), ports.catalogs(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(CatalogResponse::from(&catalog))))
}

pub(super) async fn add_products_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<CatalogId>,
    JsonBody(req): JsonBody<CatalogProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    add_catalog_products(&ctx, ports.catalogs(), id, req.products).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn remove_products_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<CatalogId>,
    JsonBody(req): JsonBody<CatalogProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    remove_catalog_products(&ctx, ports.catalogs(), id, req.products).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn assign_price_list_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<CatalogId>,
    JsonBody(req): JsonBody<AssignPriceListRequest>,
) -> Result<Json<CatalogResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
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
