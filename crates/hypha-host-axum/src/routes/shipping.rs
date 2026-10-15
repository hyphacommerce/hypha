//! Shipping setup (design note 0015, section 8): zones and their options, and the
//! external apps that calculate rates.

use hypha_core::Stamped;

use super::stamp_times;
use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{Currency, Money, Price, ShippingOptionId, ShippingZoneId, TaxClassId};
use hypha_shipping::{
    AddOption, AppProvider, BuiltinRate, CommandError, CreateAppProvider, CreateZone,
    DeliveryEstimate, HasAppProviders, HasShippingZones, OptionPatch, OptionRate, ProviderKey,
    RateBasis, RateTable, ServiceInfo, ShippingOption, ShippingZone, Tier, UpdateAppProvider,
    UpdateZone, add_option, create_app_provider, create_zone, delete_app_provider, delete_zone,
    get_app_provider, get_zone, list_app_providers, list_zones, provider_services, remove_option,
    reorder_options, rotate_app_provider_secret, update_app_provider, update_option, update_zone,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

// ---------- DTOs ----------

/// How an option's price is found: one of a provider's services, or a built-in rate.
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RateDto {
    /// Computed by Hypha: a table of tiers per currency over a basis. Flat is one open
    /// tier; free over a subtotal is two subtotal tiers, the second costing 0.
    Builtin {
        basis: BasisDto,
        tables: Vec<RateTableDto>,
    },
    /// One service of a provider (`app_<name>` or `spore_<name>`), quoted when offered.
    Provider { provider: String, service: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum BasisDto {
    Flat,
    /// The group's subtotal, in minor units.
    Subtotal,
    /// The group's weight, in grams.
    Weight,
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct RateTableDto {
    currency: Currency,
    /// Ascending; the last has no `up_to`.
    tiers: Vec<TierDto>,
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct TierDto {
    /// The tier applies while the basis is below this (exclusive); `null` on the last.
    #[serde(default)]
    up_to: Option<u64>,
    /// In minor units of the table's currency.
    amount: i64,
}

/// Delivery in days, from `min_days` to `max_days`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct DeliveryEstimateDto {
    pub(super) min_days: u16,
    pub(super) max_days: u16,
}

impl From<DeliveryEstimate> for DeliveryEstimateDto {
    fn from(e: DeliveryEstimate) -> Self {
        Self {
            min_days: e.min_days(),
            max_days: e.max_days(),
        }
    }
}

impl RateDto {
    fn into_rate(self) -> Result<OptionRate, ApiError> {
        Ok(match self {
            RateDto::Provider { provider, service } => OptionRate::Provider {
                provider: ProviderKey::new(&provider).map_err(CommandError::from)?,
                service,
            },
            RateDto::Builtin { basis, tables } => {
                let tables = tables
                    .into_iter()
                    .map(|t| {
                        let tiers = t
                            .tiers
                            .into_iter()
                            .map(|tier| {
                                let price = Money::new(tier.amount, t.currency)
                                    .and_then(Price::new)
                                    .map_err(CommandError::from)?;
                                Ok(Tier {
                                    up_to: tier.up_to,
                                    price,
                                })
                            })
                            .collect::<Result<_, ApiError>>()?;
                        Ok(RateTable::new(t.currency, tiers).map_err(CommandError::from)?)
                    })
                    .collect::<Result<_, ApiError>>()?;
                let basis = match basis {
                    BasisDto::Flat => RateBasis::Flat,
                    BasisDto::Subtotal => RateBasis::Subtotal,
                    BasisDto::Weight => RateBasis::Weight,
                };
                OptionRate::Builtin(BuiltinRate::new(basis, tables).map_err(CommandError::from)?)
            }
        })
    }

    fn from_rate(rate: &OptionRate) -> Self {
        match rate {
            OptionRate::Provider { provider, service } => RateDto::Provider {
                provider: provider.as_str().to_string(),
                service: service.clone(),
            },
            OptionRate::Builtin(rate) => RateDto::Builtin {
                basis: match rate.basis() {
                    RateBasis::Flat => BasisDto::Flat,
                    RateBasis::Subtotal => BasisDto::Subtotal,
                    RateBasis::Weight => BasisDto::Weight,
                },
                tables: rate
                    .tables()
                    .iter()
                    .map(|t| RateTableDto {
                        currency: t.currency(),
                        tiers: t
                            .tiers()
                            .iter()
                            .map(|tier| TierDto {
                                up_to: tier.up_to,
                                amount: tier.price.money().amount(),
                            })
                            .collect(),
                    })
                    .collect(),
            },
        }
    }
}

fn estimate(dto: DeliveryEstimateDto) -> Result<DeliveryEstimate, ApiError> {
    Ok(DeliveryEstimate::new(dto.min_days, dto.max_days).map_err(CommandError::from)?)
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct ShippingZoneResponse {
    id: ShippingZoneId,
    name: String,
    /// A country is in at most one zone.
    countries: Vec<hypha_core::CountryCode>,
    /// In display order.
    options: Vec<ShippingOptionResponse>,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct ShippingOptionResponse {
    id: ShippingOptionId,
    name: String,
    /// Stable, for apps and analytics; unique within the zone.
    code: String,
    rate: RateDto,
    estimate: Option<DeliveryEstimateDto>,
    /// Shipping by this option is taxed at this class's rate; the region's default
    /// otherwise.
    tax_class: Option<TaxClassId>,
    /// Offered only when every provider option of the zone failed.
    is_backup: bool,
    enabled: bool,
}

impl From<&ShippingOption> for ShippingOptionResponse {
    fn from(o: &ShippingOption) -> Self {
        Self {
            id: o.id(),
            name: o.name().to_string(),
            code: o.code().as_str().to_string(),
            rate: RateDto::from_rate(o.rate()),
            estimate: o.estimate().map(Into::into),
            tax_class: o.tax_class(),
            is_backup: o.is_backup(),
            enabled: o.enabled(),
        }
    }
}

impl ShippingZoneResponse {
    fn new(z: &ShippingZone) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(z.stamps())?;
        Ok(Self {
            id: z.id(),
            name: z.name().to_string(),
            countries: z.countries().to_vec(),
            options: z.options().iter().map(Into::into).collect(),
            created_at,
            updated_at,
        })
    }
}

// ---------- zones ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateZoneRequest {
    name: String,
    /// ISO 3166-1 alpha-2 codes, each in no other zone.
    countries: Vec<String>,
}

/// Creates a shipping zone.
#[utoipa::path(
    post,
    path = "/admin/shipping-zones",
    request_body = CreateZoneRequest,
    responses((status = 201, description = "Created", body = ShippingZoneResponse)),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn create_zone_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateZoneRequest>,
) -> Result<(StatusCode, Json<ShippingZoneResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateZone {
        name: req.name,
        countries: req.countries,
    };
    let zone = create_zone(&ctx, ports.shipping_zones(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(ShippingZoneResponse::new(&zone)?)))
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct ShippingZoneList {
    /// By name. A store has a handful, so there are no pages.
    items: Vec<ShippingZoneResponse>,
}

/// Every shipping zone, with its options.
#[utoipa::path(
    get,
    path = "/admin/shipping-zones",
    responses((status = 200, description = "OK", body = ShippingZoneList)),
    extensions(("x-hypha-scopes" = json!(["shipping:read"]))),
)]
pub(super) async fn list_zones_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<ShippingZoneList>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let zones = list_zones(&ctx, ports.shipping_zones()).await?;
    Ok(Json(ShippingZoneList {
        items: zones
            .iter()
            .map(ShippingZoneResponse::new)
            .collect::<Result<_, _>>()?,
    }))
}

/// A shipping zone, with its options.
#[utoipa::path(
    get,
    path = "/admin/shipping-zones/{id}",
    params(("id" = ShippingZoneId, Path)),
    responses((status = 200, description = "OK", body = ShippingZoneResponse)),
    extensions(("x-hypha-scopes" = json!(["shipping:read"]))),
)]
pub(super) async fn fetch_zone<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ShippingZoneId>,
) -> Result<Json<ShippingZoneResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let zone = get_zone(&ctx, ports.shipping_zones(), id).await?;
    Ok(Json(ShippingZoneResponse::new(&zone)?))
}

/// Name and countries (JSON Merge Patch; neither can be `null`).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateZoneRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    /// Replaces the zone's countries.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<String>, required = false)]
    countries: Option<Vec<String>>,
}

/// Renames a zone or changes its countries.
#[utoipa::path(
    patch,
    path = "/admin/shipping-zones/{id}",
    params(("id" = ShippingZoneId, Path)),
    request_body = UpdateZoneRequest,
    responses((status = 200, description = "OK", body = ShippingZoneResponse)),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn update_zone_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ShippingZoneId>,
    body: JsonBody<UpdateZoneRequest>,
) -> Result<Json<ShippingZoneResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateZone {
        name: req.name,
        countries: req.countries,
    };
    let zone = update_zone(&ctx, ports.shipping_zones(), id, input).await?;
    Ok(Json(ShippingZoneResponse::new(&zone)?))
}

/// Deletes a zone and its options. Carts that chose one of them choose again.
#[utoipa::path(
    delete,
    path = "/admin/shipping-zones/{id}",
    params(("id" = ShippingZoneId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn delete_zone_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ShippingZoneId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_zone(&ctx, ports.shipping_zones(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- options ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct AddOptionRequest {
    name: String,
    code: String,
    rate: RateDto,
    #[serde(default)]
    estimate: Option<DeliveryEstimateDto>,
    #[serde(default)]
    tax_class: Option<TaxClassId>,
    #[serde(default)]
    is_backup: bool,
    /// Defaults to `true`.
    #[serde(default = "enabled_by_default")]
    enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

/// Adds an option to a zone, last.
#[utoipa::path(
    post,
    path = "/admin/shipping-zones/{id}/options",
    params(("id" = ShippingZoneId, Path)),
    request_body = AddOptionRequest,
    responses((status = 201, description = "Created", body = ShippingZoneResponse)),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn add_option_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ShippingZoneId>,
    body: JsonBody<AddOptionRequest>,
) -> Result<(StatusCode, Json<ShippingZoneResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = AddOption {
        name: req.name,
        code: req.code,
        rate: req.rate.into_rate()?,
        estimate: req.estimate.map(estimate).transpose()?,
        tax_class: req.tax_class,
        is_backup: req.is_backup,
        enabled: req.enabled,
    };
    let zone = add_option(&ctx, ports.shipping_zones(), &state.ids, id, input).await?;
    Ok((StatusCode::CREATED, Json(ShippingZoneResponse::new(&zone)?)))
}

/// An option's fields (JSON Merge Patch; `estimate` and `tax_class` take `null`).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateOptionRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    code: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = RateDto, required = false)]
    rate: Option<RateDto>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<DeliveryEstimateDto>, required = false)]
    estimate: Option<Option<DeliveryEstimateDto>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<TaxClassId>, required = false)]
    tax_class: Option<Option<TaxClassId>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    is_backup: Option<bool>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    enabled: Option<bool>,
}

/// Changes an option.
#[utoipa::path(
    patch,
    path = "/admin/shipping-zones/{id}/options/{option_id}",
    params(("id" = ShippingZoneId, Path), ("option_id" = ShippingOptionId, Path)),
    request_body = UpdateOptionRequest,
    responses((status = 200, description = "OK", body = ShippingZoneResponse)),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn update_option_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, option)): PathId<(ShippingZoneId, ShippingOptionId)>,
    body: JsonBody<UpdateOptionRequest>,
) -> Result<Json<ShippingZoneResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let patch = OptionPatch {
        name: req.name,
        code: req.code,
        rate: req.rate.map(RateDto::into_rate).transpose()?,
        estimate: req
            .estimate
            .map(|e| e.map(estimate).transpose())
            .transpose()?,
        tax_class: req.tax_class,
        is_backup: req.is_backup,
        enabled: req.enabled,
    };
    let zone = update_option(&ctx, ports.shipping_zones(), id, option, patch)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("shipping_option_not_found"))?;
    Ok(Json(ShippingZoneResponse::new(&zone)?))
}

/// Removes an option.
#[utoipa::path(
    delete,
    path = "/admin/shipping-zones/{id}/options/{option_id}",
    params(("id" = ShippingZoneId, Path), ("option_id" = ShippingOptionId, Path)),
    responses((status = 200, description = "OK", body = ShippingZoneResponse)),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn remove_option_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, option)): PathId<(ShippingZoneId, ShippingOptionId)>,
) -> Result<Json<ShippingZoneResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let zone = remove_option(&ctx, ports.shipping_zones(), id, option)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("shipping_option_not_found"))?;
    Ok(Json(ShippingZoneResponse::new(&zone)?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReorderOptionsRequest {
    /// Every option of the zone, once, in the new order.
    options: Vec<ShippingOptionId>,
}

/// Puts a zone's options in a new order.
#[utoipa::path(
    put,
    path = "/admin/shipping-zones/{id}/options/order",
    params(("id" = ShippingZoneId, Path)),
    request_body = ReorderOptionsRequest,
    responses((status = 200, description = "OK", body = ShippingZoneResponse)),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn reorder_options_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ShippingZoneId>,
    body: JsonBody<ReorderOptionsRequest>,
) -> Result<Json<ShippingZoneResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let zone = reorder_options(&ctx, ports.shipping_zones(), id, req.options).await?;
    Ok(Json(ShippingZoneResponse::new(&zone)?))
}

// ---------- providers ----------

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ServiceDto {
    code: String,
    name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct AppProviderResponse {
    /// `app_<name>`: what options name it by.
    key: String,
    name: String,
    /// Where Hypha POSTs rate requests (signed with the secret).
    callback_url: String,
    timeout_ms: u32,
    /// Declared up front; empty means Hypha asks `<callback_url>/services`.
    services: Vec<ServiceDto>,
    enabled: bool,
}

impl From<&AppProvider> for AppProviderResponse {
    fn from(p: &AppProvider) -> Self {
        Self {
            key: p.key().as_str().to_string(),
            name: p.name().to_string(),
            callback_url: p.callback_url().as_str().to_string(),
            timeout_ms: p.timeout_ms(),
            services: p
                .services()
                .iter()
                .map(|s| ServiceDto {
                    code: s.code.clone(),
                    name: s.name.clone(),
                })
                .collect(),
            enabled: p.enabled(),
        }
    }
}

/// A provider with its signing secret: returned once, when created or rotated.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct AppProviderSecretResponse {
    provider: AppProviderResponse,
    /// `hypha_shs_…`: verify `Hypha-Signature` with it. Shown only now.
    secret: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateAppProviderRequest {
    /// `app_<name>`: lowercase letters, digits and `_`.
    key: String,
    name: String,
    /// `https://`, or `http://localhost` for development.
    callback_url: String,
    /// 100 to 10000; defaults to 3000.
    #[serde(default)]
    timeout_ms: Option<u32>,
    #[serde(default)]
    services: Vec<ServiceDto>,
}

fn services(dtos: Vec<ServiceDto>) -> Vec<ServiceInfo> {
    dtos.into_iter()
        .map(|s| ServiceInfo {
            code: s.code,
            name: s.name,
        })
        .collect()
}

/// Registers an external app that calculates rates (design note 0015, section 5).
/// Hypha POSTs it signed rate requests; its secret is returned here, once.
#[utoipa::path(
    post,
    path = "/admin/shipping-providers",
    request_body = CreateAppProviderRequest,
    responses((status = 201, description = "Created", body = AppProviderSecretResponse)),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn create_provider_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateAppProviderRequest>,
) -> Result<(StatusCode, Json<AppProviderSecretResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateAppProvider {
        key: req.key,
        name: req.name,
        callback_url: req.callback_url,
        timeout_ms: req.timeout_ms,
        services: services(req.services),
    };
    let (provider, secret) =
        create_app_provider(&ctx, ports.app_providers(), &state.env, &state.env, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(AppProviderSecretResponse {
            provider: (&provider).into(),
            secret,
        }),
    ))
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct AppProviderList {
    /// By key.
    items: Vec<AppProviderResponse>,
}

/// The external apps registered as rate providers.
#[utoipa::path(
    get,
    path = "/admin/shipping-providers",
    responses((status = 200, description = "OK", body = AppProviderList)),
    extensions(("x-hypha-scopes" = json!(["shipping:read"]))),
)]
pub(super) async fn list_providers_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<AppProviderList>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let providers = list_app_providers(&ctx, ports.app_providers()).await?;
    Ok(Json(AppProviderList {
        items: providers.iter().map(Into::into).collect(),
    }))
}

/// An external app provider.
#[utoipa::path(
    get,
    path = "/admin/shipping-providers/{key}",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_shipcalc", description = "`app_<name>` or `spore_<name>`")),
    responses((status = 200, description = "OK", body = AppProviderResponse)),
    extensions(("x-hypha-scopes" = json!(["shipping:read"]))),
)]
pub(super) async fn fetch_provider<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<Json<AppProviderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let provider = get_app_provider(&ctx, ports.app_providers(), &key).await?;
    Ok(Json((&provider).into()))
}

/// Name, callback, timeout, services, enabled (JSON Merge Patch; none take `null`).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateAppProviderRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    callback_url: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = u32, required = false)]
    timeout_ms: Option<u32>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<ServiceDto>, required = false)]
    services: Option<Vec<ServiceDto>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    enabled: Option<bool>,
}

/// Changes an external app provider.
#[utoipa::path(
    patch,
    path = "/admin/shipping-providers/{key}",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_shipcalc", description = "`app_<name>` or `spore_<name>`")),
    request_body = UpdateAppProviderRequest,
    responses((status = 200, description = "OK", body = AppProviderResponse)),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn update_provider_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
    body: JsonBody<UpdateAppProviderRequest>,
) -> Result<Json<AppProviderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateAppProvider {
        name: req.name,
        callback_url: req.callback_url,
        timeout_ms: req.timeout_ms,
        services: req.services.map(services),
        enabled: req.enabled,
    };
    let provider = update_app_provider(&ctx, ports.app_providers(), &key, input).await?;
    Ok(Json((&provider).into()))
}

/// Removes an external app provider; options naming it stop being offered.
#[utoipa::path(
    delete,
    path = "/admin/shipping-providers/{key}",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_shipcalc", description = "`app_<name>` or `spore_<name>`")),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn delete_provider_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_app_provider(&ctx, ports.app_providers(), &key).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Replaces the signing secret and returns the new one, once. The old one stops working.
#[utoipa::path(
    post,
    path = "/admin/shipping-providers/{key}/secret",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_shipcalc", description = "`app_<name>` or `spore_<name>`")),
    responses((status = 200, description = "OK", body = AppProviderSecretResponse)),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn rotate_secret_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<Json<AppProviderSecretResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (provider, secret) =
        rotate_app_provider_secret(&ctx, ports.app_providers(), &state.env, &state.env, &key)
            .await?;
    Ok(Json(AppProviderSecretResponse {
        provider: (&provider).into(),
        secret,
    }))
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct ServiceList {
    items: Vec<ServiceDto>,
}

/// The services a provider (app or spore) can quote, for the option editor.
#[utoipa::path(
    get,
    path = "/admin/shipping-providers/{key}/services",
    params(("key" = String, Path, pattern = "^(app|spore)_[a-z][a-z0-9_]*$", example = "app_shipcalc", description = "`app_<name>` or `spore_<name>`")),
    responses((status = 200, description = "OK", body = ServiceList)),
    extensions(("x-hypha-scopes" = json!(["shipping:write"]))),
)]
pub(super) async fn provider_services_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<Json<ServiceList>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let rates = state.rates(ports.app_providers());
    let found = provider_services(&ctx, &rates, &key).await?;
    Ok(Json(ServiceList {
        items: found
            .into_iter()
            .map(|s| ServiceDto {
                code: s.code,
                name: s.name,
            })
            .collect(),
    }))
}
