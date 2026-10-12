//! Tax classes, their products, and tax regions (all live in `hypha-tax`).

use axum::{Json, extract::State, http::StatusCode};
use hypha_cart::CartPorts;
use hypha_core::{CountryCode, Ctx, ProductId, TaxClassId, TaxRate, TaxRegionId};
use hypha_tax::{
    AssignTaxClass, CreateTaxClass, CreateTaxRegion, TaxClass, TaxRegion, assign_tax_class,
    create_tax_class, create_tax_region, get_tax_region, remove_class_rate, set_class_rate,
    set_default_rate,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{JsonBody, PathId},
    state::{SharedState, Storage},
};

// ---------- classes ----------

#[derive(Deserialize)]
pub(super) struct CreateTaxClassRequest {
    name: String,
}

#[derive(Serialize)]
pub(super) struct TaxClassResponse {
    id: TaxClassId,
    name: String,
}

impl From<&TaxClass> for TaxClassResponse {
    fn from(c: &TaxClass) -> Self {
        Self {
            id: c.id(),
            name: c.name().to_string(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct AssignTaxClassRequest {
    products: Vec<ProductId>,
    /// `null` puts the products back on each country's default rate.
    tax_class: Option<TaxClassId>,
}

pub(super) async fn create_class<S: Storage>(
    State(state): State<SharedState<S>>,
    JsonBody(req): JsonBody<CreateTaxClassRequest>,
) -> Result<(StatusCode, Json<TaxClassResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = CreateTaxClass { name: req.name };
    let class = create_tax_class(&ctx, ports.tax_classes(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(TaxClassResponse::from(&class))))
}

pub(super) async fn assign_class<S: Storage>(
    State(state): State<SharedState<S>>,
    JsonBody(req): JsonBody<AssignTaxClassRequest>,
) -> Result<StatusCode, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = AssignTaxClass {
        products: req.products,
        class: req.tax_class,
    };
    assign_tax_class(&ctx, ports.tax_classes(), input).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- regions ----------

#[derive(Deserialize)]
pub(super) struct CreateTaxRegionRequest {
    country: String,
    /// Parts per million: 250000 = 25%.
    default_rate: u32,
}

#[derive(Deserialize)]
pub(super) struct UpdateTaxRegionRequest {
    default_rate: u32,
}

#[derive(Deserialize)]
pub(super) struct SetClassRateRequest {
    rate: u32,
}

#[derive(Serialize)]
pub(super) struct TaxRegionResponse {
    id: TaxRegionId,
    country: CountryCode,
    default_rate: TaxRate,
    class_rates: Vec<ClassRateResponse>,
}

#[derive(Serialize)]
pub(super) struct ClassRateResponse {
    tax_class: TaxClassId,
    rate: TaxRate,
}

impl From<&TaxRegion> for TaxRegionResponse {
    fn from(r: &TaxRegion) -> Self {
        Self {
            id: r.id(),
            country: r.country(),
            default_rate: r.default_rate(),
            class_rates: r
                .class_rates()
                .iter()
                .map(|c| ClassRateResponse {
                    tax_class: c.class,
                    rate: c.rate,
                })
                .collect(),
        }
    }
}

pub(super) async fn create_region<S: Storage>(
    State(state): State<SharedState<S>>,
    JsonBody(req): JsonBody<CreateTaxRegionRequest>,
) -> Result<(StatusCode, Json<TaxRegionResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = CreateTaxRegion {
        country: req.country,
        default_rate: req.default_rate,
    };
    let region = create_tax_region(&ctx, ports.tax_regions(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(TaxRegionResponse::from(&region))))
}

pub(super) async fn fetch_region<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<TaxRegionId>,
) -> Result<Json<TaxRegionResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let region = get_tax_region(&ctx, ports.tax_regions(), id).await?;
    Ok(Json(TaxRegionResponse::from(&region)))
}

pub(super) async fn update_region<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<TaxRegionId>,
    JsonBody(req): JsonBody<UpdateTaxRegionRequest>,
) -> Result<Json<TaxRegionResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let region = set_default_rate(&ctx, ports.tax_regions(), id, req.default_rate).await?;
    Ok(Json(TaxRegionResponse::from(&region)))
}

pub(super) async fn set_region_class_rate<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId((id, class)): PathId<(TaxRegionId, TaxClassId)>,
    JsonBody(req): JsonBody<SetClassRateRequest>,
) -> Result<Json<TaxRegionResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let region = set_class_rate(
        &ctx,
        ports.tax_regions(),
        ports.tax_classes(),
        id,
        class,
        req.rate,
    )
    .await
    .map_err(|e| ApiError::from(e).not_found_if("tax_class_not_found"))?;
    Ok(Json(TaxRegionResponse::from(&region)))
}

pub(super) async fn remove_region_class_rate<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId((id, class)): PathId<(TaxRegionId, TaxClassId)>,
) -> Result<Json<TaxRegionResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let region = remove_class_rate(&ctx, ports.tax_regions(), id, class).await?;
    Ok(Json(TaxRegionResponse::from(&region)))
}
