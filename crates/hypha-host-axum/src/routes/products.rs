use axum::{Json, extract::State, http::StatusCode};
use hypha_cart::CartPorts;
use hypha_core::{Ctx, Money, ProductId, VariantId};
use hypha_products::{
    CreateSimpleProduct, Product, ProductStatus, create_simple_product, get_product,
    set_product_status,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{JsonBody, PathId},
    state::{SharedState, Storage},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateProductRequest {
    handle: String,
    title: String,
    price: Money,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetStatusRequest {
    status: ProductStatus,
}

#[derive(Serialize)]
pub(super) struct ProductResponse {
    id: ProductId,
    handle: String,
    title: String,
    status: ProductStatus,
    variants: Vec<VariantResponse>,
}

#[derive(Serialize)]
pub(super) struct VariantResponse {
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

pub(super) async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    JsonBody(req): JsonBody<CreateProductRequest>,
) -> Result<(StatusCode, Json<ProductResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth (API keys / tokens)
    let ports = state.ports().await?;
    let input = CreateSimpleProduct {
        handle: req.handle,
        title: req.title,
        price: req.price,
    };
    let product = create_simple_product(&ctx, ports.products(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(ProductResponse::from(&product))))
}

pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<ProductId>,
) -> Result<Json<ProductResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let product = get_product(&ctx, ports.products(), id).await?;
    Ok(Json(ProductResponse::from(&product)))
}

pub(super) async fn set_status<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<ProductId>,
    JsonBody(req): JsonBody<SetStatusRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let product = set_product_status(&ctx, ports.products(), id, req.status).await?;
    Ok(Json(ProductResponse::from(&product)))
}
