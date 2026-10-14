//! Tax classes, their products, and tax regions (all live in `hypha-tax`).

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{CountryCode, ProductId, TaxClassId, TaxRate, TaxRegionId};
use hypha_tax::{
    AssignTaxClass, CreateTaxClass, CreateTaxRegion, HasTaxClasses, HasTaxRegions, TaxClass,
    TaxRegion, assign_tax_class, create_tax_class, create_tax_region, get_tax_region,
    remove_class_rate, set_class_rate, set_default_rate,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

// ---------- classes ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateTaxClassRequest {
    name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
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

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct AssignTaxClassRequest {
    products: Vec<ProductId>,
    /// `null` puts the products back on each country's default rate.
    tax_class: Option<TaxClassId>,
}

/// Creates a tax class.
#[utoipa::path(
    post,
    path = "/admin/tax-classes",
    request_body = CreateTaxClassRequest,
    responses((status = 201, description = "Created", body = TaxClassResponse)),
    extensions(("x-hypha-scopes" = json!(["taxes:write"]))),
)]
pub(super) async fn create_class<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateTaxClassRequest>,
) -> Result<(StatusCode, Json<TaxClassResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateTaxClass { name: req.name };
    let class = create_tax_class(&ctx, ports.tax_classes(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(TaxClassResponse::from(&class))))
}

/// Assigns products to a tax class.
#[utoipa::path(
    put,
    path = "/admin/tax-assignments",
    request_body = AssignTaxClassRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["taxes:write"]))),
)]
pub(super) async fn assign_class<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<AssignTaxClassRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = AssignTaxClass {
        products: req.products,
        class: req.tax_class,
    };
    assign_tax_class(&ctx, ports.tax_classes(), input).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- regions ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateTaxRegionRequest {
    country: String,
    /// Parts per million: 250000 = 25%.
    default_rate_ppm: u32,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateTaxRegionRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    default_rate_ppm: Option<u32>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SetClassRateRequest {
    rate_ppm: u32,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct TaxRegionResponse {
    id: TaxRegionId,
    country: CountryCode,
    default_rate_ppm: TaxRate,
    class_rates: Vec<ClassRateResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct ClassRateResponse {
    tax_class: TaxClassId,
    rate_ppm: TaxRate,
}

impl From<&TaxRegion> for TaxRegionResponse {
    fn from(r: &TaxRegion) -> Self {
        Self {
            id: r.id(),
            country: r.country(),
            default_rate_ppm: r.default_rate(),
            class_rates: r
                .class_rates()
                .iter()
                .map(|c| ClassRateResponse {
                    tax_class: c.class,
                    rate_ppm: c.rate,
                })
                .collect(),
        }
    }
}

/// Creates a tax region for a country.
#[utoipa::path(
    post,
    path = "/admin/tax-regions",
    request_body = CreateTaxRegionRequest,
    responses((status = 201, description = "Created", body = TaxRegionResponse)),
    extensions(("x-hypha-scopes" = json!(["taxes:write"]))),
)]
pub(super) async fn create_region<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateTaxRegionRequest>,
) -> Result<(StatusCode, Json<TaxRegionResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateTaxRegion {
        country: req.country,
        default_rate: req.default_rate_ppm,
    };
    let region = create_tax_region(&ctx, ports.tax_regions(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(TaxRegionResponse::from(&region))))
}

/// A tax region with its rates.
#[utoipa::path(
    get,
    path = "/admin/tax-regions/{id}",
    params(("id" = TaxRegionId, Path)),
    responses((status = 200, description = "OK", body = TaxRegionResponse)),
    extensions(("x-hypha-scopes" = json!(["taxes:read"]))),
)]
pub(super) async fn fetch_region<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<TaxRegionId>,
) -> Result<Json<TaxRegionResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let region = get_tax_region(&ctx, ports.tax_regions(), id).await?;
    Ok(Json(TaxRegionResponse::from(&region)))
}

/// Changes a tax region's default rate.
#[utoipa::path(
    patch,
    path = "/admin/tax-regions/{id}",
    params(("id" = TaxRegionId, Path)),
    request_body = UpdateTaxRegionRequest,
    responses((status = 200, description = "OK", body = TaxRegionResponse)),
    extensions(("x-hypha-scopes" = json!(["taxes:write"]))),
)]
pub(super) async fn update_region<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<TaxRegionId>,
    body: JsonBody<UpdateTaxRegionRequest>,
) -> Result<Json<TaxRegionResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let region = match req.default_rate_ppm {
        Some(rate) => set_default_rate(&ctx, ports.tax_regions(), id, rate).await?,
        None => get_tax_region(&ctx, ports.tax_regions(), id).await?,
    };
    Ok(Json(TaxRegionResponse::from(&region)))
}

/// Sets the rate a tax class has in a region.
#[utoipa::path(
    put,
    path = "/admin/tax-regions/{id}/class-rates/{class_id}",
    params(("id" = TaxRegionId, Path), ("class_id" = TaxClassId, Path)),
    request_body = SetClassRateRequest,
    responses((status = 200, description = "OK", body = TaxRegionResponse)),
    extensions(("x-hypha-scopes" = json!(["taxes:write"]))),
)]
pub(super) async fn set_region_class_rate<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, class)): PathId<(TaxRegionId, TaxClassId)>,
    body: JsonBody<SetClassRateRequest>,
) -> Result<Json<TaxRegionResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let region = set_class_rate(
        &ctx,
        ports.tax_regions(),
        ports.tax_classes(),
        id,
        class,
        req.rate_ppm,
    )
    .await
    .map_err(|e| ApiError::from(e).not_found_if("tax_class_not_found"))?;
    Ok(Json(TaxRegionResponse::from(&region)))
}

/// Removes a tax class's own rate in a region.
#[utoipa::path(
    delete,
    path = "/admin/tax-regions/{id}/class-rates/{class_id}",
    params(("id" = TaxRegionId, Path), ("class_id" = TaxClassId, Path)),
    responses((status = 200, description = "OK", body = TaxRegionResponse)),
    extensions(("x-hypha-scopes" = json!(["taxes:write"]))),
)]
pub(super) async fn remove_region_class_rate<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, class)): PathId<(TaxRegionId, TaxClassId)>,
) -> Result<Json<TaxRegionResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let region = remove_class_rate(&ctx, ports.tax_regions(), id, class).await?;
    Ok(Json(TaxRegionResponse::from(&region)))
}
