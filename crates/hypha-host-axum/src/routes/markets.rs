//! Markets, price lists and catalogs (all live in `hypha-markets`).

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{post, put},
};
use hypha_core::{
    CatalogId, CountryCode, Ctx, Currency, MarketId, Money, PriceListId, ProductId, VariantId,
};
use hypha_markets::{
    Catalog, CatalogContext, CreateCatalog, CreateMarket, CreatePriceList, FixedPrice, Market,
    PriceList, add_catalog_products, assign_price_list, create_catalog, create_market,
    create_price_list, remove_catalog_products, remove_fixed_prices, set_fixed_prices,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{JsonBody, PathId},
    state::SharedState,
};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/markets", post(create_market_handler))
        .route("/price-lists", post(create_price_list_handler))
        .route(
            "/price-lists/{id}/fixed-prices",
            put(set_fixed_prices_handler).delete(remove_fixed_prices_handler),
        )
        .route("/catalogs", post(create_catalog_handler))
        .route(
            "/catalogs/{id}/products",
            post(add_products_handler).delete(remove_products_handler),
        )
        .route("/catalogs/{id}/price-list", put(assign_price_list_handler))
}

// ---------- markets ----------

#[derive(Deserialize)]
struct CreateMarketRequest {
    name: String,
    countries: Vec<String>,
    currency: Currency,
}

#[derive(Serialize)]
struct MarketResponse {
    id: MarketId,
    name: String,
    countries: Vec<CountryCode>,
    currency: Currency,
}

impl From<&Market> for MarketResponse {
    fn from(m: &Market) -> Self {
        Self {
            id: m.id(),
            name: m.name().to_string(),
            countries: m.countries().to_vec(),
            currency: m.currency(),
        }
    }
}

async fn create_market_handler(
    State(state): State<SharedState>,
    JsonBody(req): JsonBody<CreateMarketRequest>,
) -> Result<(StatusCode, Json<MarketResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let input = CreateMarket {
        name: req.name,
        countries: req.countries,
        currency: req.currency,
    };
    let market = create_market(&ctx, &state.markets, &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(MarketResponse::from(&market))))
}

// ---------- price lists ----------

#[derive(Deserialize)]
struct CreatePriceListRequest {
    currency: Currency,
    /// Basis points: -1000 = 10% off. Defaults to 0.
    #[serde(default)]
    adjustment: i32,
}

#[derive(Serialize)]
struct PriceListResponse {
    id: PriceListId,
    currency: Currency,
    adjustment: i32,
}

impl From<&PriceList> for PriceListResponse {
    fn from(l: &PriceList) -> Self {
        Self {
            id: l.id(),
            currency: l.currency(),
            adjustment: l.adjustment().value(),
        }
    }
}

#[derive(Deserialize)]
struct SetFixedPricesRequest {
    fixed_prices: Vec<FixedPriceDto>,
}

#[derive(Deserialize)]
struct FixedPriceDto {
    variant: VariantId,
    price: Money,
}

#[derive(Deserialize)]
struct RemoveFixedPricesRequest {
    variants: Vec<VariantId>,
}

async fn create_price_list_handler(
    State(state): State<SharedState>,
    JsonBody(req): JsonBody<CreatePriceListRequest>,
) -> Result<(StatusCode, Json<PriceListResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let input = CreatePriceList {
        currency: req.currency,
        adjustment: req.adjustment,
    };
    let list = create_price_list(&ctx, &state.price_lists, &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(PriceListResponse::from(&list))))
}

async fn set_fixed_prices_handler(
    State(state): State<SharedState>,
    PathId(id): PathId<PriceListId>,
    JsonBody(req): JsonBody<SetFixedPricesRequest>,
) -> Result<StatusCode, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let prices = req
        .fixed_prices
        .into_iter()
        .map(|f| FixedPrice {
            variant: f.variant,
            price: f.price,
        })
        .collect();
    set_fixed_prices(&ctx, &state.price_lists, id, prices)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_fixed_prices_handler(
    State(state): State<SharedState>,
    PathId(id): PathId<PriceListId>,
    JsonBody(req): JsonBody<RemoveFixedPricesRequest>,
) -> Result<StatusCode, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    remove_fixed_prices(&ctx, &state.price_lists, id, req.variants)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("price_list_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- catalogs ----------

#[derive(Deserialize)]
struct CreateCatalogRequest {
    market: MarketId,
}

#[derive(Deserialize)]
struct CatalogProductsRequest {
    products: Vec<ProductId>,
}

#[derive(Deserialize)]
struct AssignPriceListRequest {
    /// `null` detaches the current price list.
    price_list: Option<PriceListId>,
}

#[derive(Serialize)]
struct CatalogResponse {
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

async fn create_catalog_handler(
    State(state): State<SharedState>,
    JsonBody(req): JsonBody<CreateCatalogRequest>,
) -> Result<(StatusCode, Json<CatalogResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let input = CreateCatalog { market: req.market };
    let catalog = create_catalog(&ctx, &state.markets, &state.catalogs, &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(CatalogResponse::from(&catalog))))
}

async fn add_products_handler(
    State(state): State<SharedState>,
    PathId(id): PathId<CatalogId>,
    JsonBody(req): JsonBody<CatalogProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    add_catalog_products(&ctx, &state.catalogs, id, req.products).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_products_handler(
    State(state): State<SharedState>,
    PathId(id): PathId<CatalogId>,
    JsonBody(req): JsonBody<CatalogProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    remove_catalog_products(&ctx, &state.catalogs, id, req.products).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn assign_price_list_handler(
    State(state): State<SharedState>,
    PathId(id): PathId<CatalogId>,
    JsonBody(req): JsonBody<AssignPriceListRequest>,
) -> Result<Json<CatalogResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let catalog = assign_price_list(
        &ctx,
        &state.markets,
        &state.catalogs,
        &state.price_lists,
        id,
        req.price_list,
    )
    .await?;
    Ok(Json(CatalogResponse::from(&catalog)))
}
