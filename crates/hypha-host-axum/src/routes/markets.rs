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
    PriceList, assign_price_list, create_catalog, create_market, create_price_list,
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
        .route("/catalogs", post(create_catalog_handler))
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
    #[serde(default)]
    fixed_prices: Vec<FixedPriceDto>,
}

#[derive(Deserialize, Serialize)]
struct FixedPriceDto {
    variant: VariantId,
    price: Money,
}

#[derive(Serialize)]
struct PriceListResponse {
    id: PriceListId,
    currency: Currency,
    adjustment: i32,
    /// Sorted by variant ID so responses are stable.
    fixed_prices: Vec<FixedPriceDto>,
}

impl From<&PriceList> for PriceListResponse {
    fn from(l: &PriceList) -> Self {
        let mut fixed_prices: Vec<FixedPriceDto> = l
            .fixed_prices()
            .iter()
            .map(|(variant, price)| FixedPriceDto {
                variant: *variant,
                price: price.money(),
            })
            .collect();
        fixed_prices.sort_by_key(|f| f.variant.as_uuid());
        Self {
            id: l.id(),
            currency: l.currency(),
            adjustment: l.adjustment().value(),
            fixed_prices,
        }
    }
}

async fn create_price_list_handler(
    State(state): State<SharedState>,
    JsonBody(req): JsonBody<CreatePriceListRequest>,
) -> Result<(StatusCode, Json<PriceListResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let input = CreatePriceList {
        currency: req.currency,
        adjustment: req.adjustment,
        fixed_prices: req
            .fixed_prices
            .into_iter()
            .map(|f| FixedPrice {
                variant: f.variant,
                price: f.price,
            })
            .collect(),
    };
    let list = create_price_list(&ctx, &state.price_lists, &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(PriceListResponse::from(&list))))
}

// ---------- catalogs ----------

#[derive(Deserialize)]
struct CreateCatalogRequest {
    market: MarketId,
    #[serde(default)]
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
    /// Sorted by ID so responses are stable.
    products: Vec<ProductId>,
    price_list: Option<PriceListId>,
}

impl From<&Catalog> for CatalogResponse {
    fn from(c: &Catalog) -> Self {
        // Irrefutable while Market is the only context; a new variant makes this a compile error.
        let CatalogContext::Market(market) = c.context();
        let mut products: Vec<ProductId> = c.products().iter().copied().collect();
        products.sort_by_key(|p| p.as_uuid());
        Self {
            id: c.id(),
            market,
            products,
            price_list: c.price_list(),
        }
    }
}

async fn create_catalog_handler(
    State(state): State<SharedState>,
    JsonBody(req): JsonBody<CreateCatalogRequest>,
) -> Result<(StatusCode, Json<CatalogResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let input = CreateCatalog {
        market: req.market,
        products: req.products,
    };
    let catalog = create_catalog(&ctx, &state.markets, &state.catalogs, &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(CatalogResponse::from(&catalog))))
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
