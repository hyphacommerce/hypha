use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{Money, ProductId, VariantId};
use hypha_products::{
    CreateSimpleProduct, HasProducts, Product, ProductStatus, create_simple_product, get_product,
    set_product_status,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId, Storefront},
    state::{SharedState, Storage},
};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateProductRequest {
    handle: String,
    title: String,
    price: Money,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SetStatusRequest {
    status: StatusDto,
}

/// `ProductStatus` on the wire: snake_case strings. A DTO of its own, so the domain
/// enum carries no serde and renaming a variant can't silently change the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = ProductStatus)]
#[serde(rename_all = "snake_case")]
pub(super) enum StatusDto {
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
pub(super) struct ProductResponse {
    id: ProductId,
    handle: String,
    title: String,
    status: StatusDto,
    variants: Vec<VariantResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
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
            status: p.status().into(),
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

/// Creates a product with one variant, as a draft.
#[utoipa::path(
    post,
    path = "/admin/products",
    request_body = CreateProductRequest,
    responses((status = 201, description = "Created", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub(super) async fn create<S: Storage>(
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
    Ok((StatusCode::CREATED, Json(ProductResponse::from(&product))))
}

/// A product, in any status.
#[utoipa::path(
    get,
    path = "/admin/products/{id}",
    params(("id" = ProductId, Path)),
    responses((status = 200, description = "OK", body = ProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let product = get_product(&ctx, ports.products(), id).await?;
    Ok(Json(ProductResponse::from(&product)))
}

/// A product as a storefront sees it (design note 0010, S5): active products only, without
/// admin fields. A draft or archived product is `404 product_not_found`, the same as one
/// that doesn't exist, so a storefront can't tell them apart.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct StorefrontProductResponse {
    id: ProductId,
    handle: String,
    title: String,
    variants: Vec<VariantResponse>,
}

impl From<&Product> for StorefrontProductResponse {
    fn from(p: &Product) -> Self {
        let admin = ProductResponse::from(p);
        Self {
            id: admin.id,
            handle: admin.handle,
            title: admin.title,
            variants: admin.variants,
        }
    }
}

/// An active product; drafts and archived products read as missing.
#[utoipa::path(
    get,
    path = "/storefront/products/{id}",
    params(("id" = ProductId, Path)),
    responses((status = 200, description = "OK", body = StorefrontProductResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub(super) async fn fetch_storefront<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<ProductId>,
) -> Result<Json<StorefrontProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let product = get_product(&ctx, ports.products(), id).await?;
    if product.status() != ProductStatus::Active {
        return Err(hypha_products::CommandError::ProductNotFound(id).into());
    }
    Ok(Json(StorefrontProductResponse::from(&product)))
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
pub(super) async fn set_status<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    body: JsonBody<SetStatusRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let product = set_product_status(&ctx, ports.products(), id, req.status.into()).await?;
    Ok(Json(ProductResponse::from(&product)))
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
