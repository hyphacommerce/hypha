use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, patch, post},
};
use hypha_core::{Ctx, Money, ProductId, VariantId};
use hypha_products::{
    CreateSimpleProduct, Product, ProductStatus, create_simple_product, get_product,
    set_product_status,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{JsonBody, PathId},
    state::SharedState,
};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/products", post(create))
        .route("/products/{id}", get(fetch))
        .route("/products/{id}/status", patch(set_status))
}

#[derive(Deserialize)]
struct CreateProductRequest {
    handle: String,
    title: String,
    price: Money,
}

#[derive(Deserialize)]
struct SetStatusRequest {
    status: ProductStatus,
}

#[derive(Serialize)]
struct ProductResponse {
    id: ProductId,
    handle: String,
    title: String,
    status: ProductStatus,
    variants: Vec<VariantResponse>,
}

#[derive(Serialize)]
struct VariantResponse {
    id: VariantId,
    price: Money,
}

impl From<&Product> for ProductResponse {
    fn from(p: &Product) -> Self {
        Self {
            id: p.id(),
            handle: p.handle().as_str().to_string(),
            title: p.title().to_string(),
            status: p.status(),
            variants: p
                .variants()
                .iter()
                .map(|v| VariantResponse {
                    id: v.id(),
                    price: v.base_price().money(),
                })
                .collect(),
        }
    }
}

async fn create(
    State(state): State<SharedState>,
    JsonBody(req): JsonBody<CreateProductRequest>,
) -> Result<(StatusCode, Json<ProductResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth (API keys / tokens)
    let input = CreateSimpleProduct {
        handle: req.handle,
        title: req.title,
        price: req.price,
    };
    let product = create_simple_product(&ctx, &state.products, &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(ProductResponse::from(&product))))
}

async fn fetch(
    State(state): State<SharedState>,
    PathId(id): PathId<ProductId>,
) -> Result<Json<ProductResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let product = get_product(&ctx, &state.products, id).await?;
    Ok(Json(ProductResponse::from(&product)))
}

async fn set_status(
    State(state): State<SharedState>,
    PathId(id): PathId<ProductId>,
    JsonBody(req): JsonBody<SetStatusRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let product = set_product_status(&ctx, &state.products, id, req.status).await?;
    Ok(Json(ProductResponse::from(&product)))
}
