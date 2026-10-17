//! Locations, inventory items and their stock (all live in `hypha-inventory`).

use hypha_core::Stamped;

use super::stamp_times;
use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{InventoryItemId, LocationId, MarketId, ProductId, VariantId};
use hypha_inventory::{
    AdjustOnHand, CreateInventoryItem, CreateLocation, HasInventoryItems, HasLocations,
    InventoryItem, Location, SetOnHand, UpdateInventoryItem, UpdateLocation, adjust_on_hand,
    create_inventory_item, create_location, get_inventory_item, get_location,
    get_variant_inventory, get_variants_inventory, list_locations, set_on_hand,
    update_inventory_item, update_location,
};
use hypha_products::{HasProducts, get_product};
use serde::{Deserialize, Serialize};

use super::address::{PostalAddressRequest, PostalAddressResponse};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

// ---------- locations ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateLocationRequest {
    name: String,
    /// The markets whose buyers this location's stock is sold to.
    #[serde(default)]
    markets: Vec<MarketId>,
    /// Lower goes first when several locations could fill a line. Defaults to 0.
    #[serde(default)]
    priority: u32,
}

/// Every field optional: only the ones sent change.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateLocationRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    // Optional, but `null` is refused (`patch::non_null`): not nullable in the spec.
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    // Optional, but `null` is refused (`patch::non_null`): not nullable in the spec.
    #[schema(value_type = Vec<MarketId>, required = false)]
    markets: Option<Vec<MarketId>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    // Optional, but `null` is refused (`patch::non_null`): not nullable in the spec.
    #[schema(value_type = u32, required = false)]
    priority: Option<u32>,
    /// Where it ships from; `null` removes it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<PostalAddressRequest>, required = false)]
    address: Option<Option<PostalAddressRequest>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct LocationResponse {
    id: LocationId,
    name: String,
    markets: Vec<MarketId>,
    priority: u32,
    /// Where it ships from: carriers quote from it. `null` until set.
    address: Option<PostalAddressResponse>,
}

impl From<&Location> for LocationResponse {
    fn from(l: &Location) -> Self {
        Self {
            id: l.id(),
            name: l.name().to_string(),
            markets: l.markets().to_vec(),
            priority: l.priority(),
            address: l.address().map(PostalAddressResponse::from),
        }
    }
}

/// Creates a stock location.
#[utoipa::path(
    post,
    path = "/admin/locations",
    request_body = CreateLocationRequest,
    responses((status = 201, description = "Created", body = LocationResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub(super) async fn create_location_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateLocationRequest>,
) -> Result<(StatusCode, Json<LocationResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateLocation {
        name: req.name,
        markets: req.markets,
        priority: req.priority,
    };
    let location = create_location(&ctx, ports.locations(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(LocationResponse::from(&location))))
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct LocationList {
    /// By priority, then ID.
    items: Vec<LocationResponse>,
}

/// Every stock location. A store starts with one, "Default", serving no market.
#[utoipa::path(
    get,
    path = "/admin/locations",
    responses((status = 200, description = "OK", body = LocationList)),
    extensions(("x-hypha-scopes" = json!(["inventory:read"]))),
)]
pub(super) async fn list_locations_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<LocationList>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let locations = list_locations(&ctx, ports.locations()).await?;
    Ok(Json(LocationList {
        items: locations.iter().map(LocationResponse::from).collect(),
    }))
}

/// A stock location.
#[utoipa::path(
    get,
    path = "/admin/locations/{id}",
    params(("id" = LocationId, Path)),
    responses((status = 200, description = "OK", body = LocationResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:read"]))),
)]
pub(super) async fn fetch_location<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<LocationId>,
) -> Result<Json<LocationResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let location = get_location(&ctx, ports.locations(), id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("location_not_found"))?;
    Ok(Json(LocationResponse::from(&location)))
}

/// Changes a stock location.
#[utoipa::path(
    patch,
    path = "/admin/locations/{id}",
    params(("id" = LocationId, Path)),
    request_body = UpdateLocationRequest,
    responses((status = 200, description = "OK", body = LocationResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub(super) async fn update_location_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<LocationId>,
    body: JsonBody<UpdateLocationRequest>,
) -> Result<Json<LocationResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateLocation {
        name: req.name,
        markets: req.markets,
        priority: req.priority,
        address: req.address.map(|a| a.map(Into::into)),
    };
    let location = update_location(&ctx, ports.locations(), id, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("location_not_found"))?;
    Ok(Json(LocationResponse::from(&location)))
}

// ---------- items ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateInventoryItemRequest {
    variant: VariantId,
    /// Defaults to true: creating an item is how stock tracking starts.
    #[serde(default = "yes")]
    tracked: bool,
    /// Keep selling when stock runs out. Defaults to false.
    #[serde(default)]
    continue_selling: bool,
}

fn yes() -> bool {
    true
}

/// Every field optional: only the ones sent change.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateInventoryItemRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    // Optional, but `null` is refused (`patch::non_null`): not nullable in the spec.
    #[schema(value_type = bool, required = false)]
    tracked: Option<bool>,
    #[serde(default, deserialize_with = "patch::non_null")]
    // Optional, but `null` is refused (`patch::non_null`): not nullable in the spec.
    #[schema(value_type = bool, required = false)]
    continue_selling: Option<bool>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SetOnHandRequest {
    on_hand: i64,
    /// The count last seen. If it moved since (a sale, another count), the request
    /// fails with `on_hand_changed` instead of overwriting it.
    expected_on_hand: Option<i64>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct AdjustOnHandRequest {
    /// Negative to remove stock.
    delta: i64,
}

/// A variant's stock. `id` is `null` when stock was never set up for the variant,
/// which means untracked (always sellable).
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct InventoryResponse {
    id: Option<InventoryItemId>,
    variant: VariantId,
    tracked: bool,
    continue_selling: bool,
    levels: Vec<LevelResponse>,
    /// When the item was created, RFC 3339 in UTC; `null` for a variant without one.
    created_at: Option<String>,
    /// When the item last changed; `null` for a variant without one.
    updated_at: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct LevelResponse {
    location: LocationId,
    on_hand: i64,
    /// Set aside for orders, not yet shipped.
    reserved: i64,
    /// `on_hand - reserved`; negative when more is reserved than is there.
    available: i64,
}

impl InventoryResponse {
    fn new(i: &InventoryItem) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(i.stamps())?;
        // Every location with a count or a reservation, counts first.
        let mut locations: Vec<LocationId> = i.levels().iter().map(|l| l.location).collect();
        for r in i.reservations() {
            if !locations.contains(&r.location) {
                locations.push(r.location);
            }
        }
        Ok(Self {
            created_at: Some(created_at),
            updated_at: Some(updated_at),
            id: Some(i.id()),
            variant: i.variant(),
            tracked: i.tracked(),
            continue_selling: i.continue_selling(),
            levels: locations
                .into_iter()
                .map(|location| LevelResponse {
                    location,
                    on_hand: i.on_hand(location),
                    reserved: i.reserved(location),
                    available: i.available(location),
                })
                .collect(),
        })
    }
}

impl InventoryResponse {
    fn untracked(variant: VariantId) -> Self {
        Self {
            id: None,
            variant,
            tracked: false,
            continue_selling: false,
            levels: Vec::new(),
            created_at: None,
            updated_at: None,
        }
    }
}

/// Creates the inventory item that tracks a variant's stock.
#[utoipa::path(
    post,
    path = "/admin/inventory-items",
    request_body = CreateInventoryItemRequest,
    responses((status = 201, description = "Created", body = InventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub(super) async fn create_item<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateInventoryItemRequest>,
) -> Result<(StatusCode, Json<InventoryResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateInventoryItem {
        variant: req.variant,
        tracked: req.tracked,
        continue_selling: req.continue_selling,
    };
    let item = create_inventory_item(&ctx, ports.inventory_items(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(InventoryResponse::new(&item)?)))
}

/// An inventory item with its levels per location.
#[utoipa::path(
    get,
    path = "/admin/inventory-items/{id}",
    params(("id" = InventoryItemId, Path)),
    responses((status = 200, description = "OK", body = InventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:read"]))),
)]
pub(super) async fn fetch_item<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<InventoryItemId>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let item = get_inventory_item(&ctx, ports.inventory_items(), id).await?;
    Ok(Json(InventoryResponse::new(&item)?))
}

/// Changes how an inventory item is tracked.
#[utoipa::path(
    patch,
    path = "/admin/inventory-items/{id}",
    params(("id" = InventoryItemId, Path)),
    request_body = UpdateInventoryItemRequest,
    responses((status = 200, description = "OK", body = InventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub(super) async fn update_item<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<InventoryItemId>,
    body: JsonBody<UpdateInventoryItemRequest>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateInventoryItem {
        tracked: req.tracked,
        continue_selling: req.continue_selling,
    };
    let item = update_inventory_item(&ctx, ports.inventory_items(), id, input).await?;
    Ok(Json(InventoryResponse::new(&item)?))
}

/// Sets the stock on hand at a location.
#[utoipa::path(
    put,
    path = "/admin/inventory-items/{id}/levels/{location_id}",
    params(("id" = InventoryItemId, Path), ("location_id" = LocationId, Path)),
    request_body = SetOnHandRequest,
    responses((status = 200, description = "OK", body = InventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub(super) async fn set_level<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((item, location)): PathId<(InventoryItemId, LocationId)>,
    body: JsonBody<SetOnHandRequest>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = SetOnHand {
        item,
        location,
        quantity: req.on_hand,
        expected: req.expected_on_hand,
    };
    let item = set_on_hand(&ctx, ports.inventory_items(), ports.locations(), input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("location_not_found"))?;
    Ok(Json(InventoryResponse::new(&item)?))
}

/// Adjusts the stock on hand at a location by a delta.
#[utoipa::path(
    post,
    path = "/admin/inventory-items/{id}/levels/{location_id}/adjustments",
    params(("id" = InventoryItemId, Path), ("location_id" = LocationId, Path)),
    request_body = AdjustOnHandRequest,
    responses((status = 200, description = "OK", body = InventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub(super) async fn adjust_level<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((item, location)): PathId<(InventoryItemId, LocationId)>,
    body: JsonBody<AdjustOnHandRequest>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = AdjustOnHand {
        item,
        location,
        delta: req.delta,
    };
    let item = adjust_on_hand(&ctx, ports.inventory_items(), ports.locations(), input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("location_not_found"))?;
    Ok(Json(InventoryResponse::new(&item)?))
}

/// A variant's stock, whether or not it has an item (design note 0004, I3).
#[utoipa::path(
    get,
    path = "/admin/variants/{id}/inventory",
    params(("id" = VariantId, Path)),
    responses((status = 200, description = "OK", body = InventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:read"]))),
)]
pub(super) async fn fetch_variant_inventory<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(variant): PathId<VariantId>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let response = match get_variant_inventory(&ctx, ports.inventory_items(), variant).await? {
        Some(item) => InventoryResponse::new(&item)?,
        None => InventoryResponse::untracked(variant),
    };
    Ok(Json(response))
}

/// Every variant's stock in a product, in the product's order.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct ProductInventoryResponse {
    /// One per variant, as `GET /admin/variants/{id}/inventory` answers it.
    items: Vec<InventoryResponse>,
}

/// A product's stock, every variant at once: what the product editor shows, in one
/// call instead of one per variant (design note 0025, §6).
#[utoipa::path(
    get,
    path = "/admin/products/{id}/inventory",
    params(("id" = ProductId, Path)),
    responses((status = 200, description = "OK", body = ProductInventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read", "inventory:read"]))),
)]
pub(super) async fn fetch_product_inventory<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
) -> Result<Json<ProductInventoryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let product = get_product(&ctx, ports.products(), id).await?;
    let variants: Vec<VariantId> = product.variants().iter().map(|v| v.id()).collect();
    let mut found: std::collections::HashMap<VariantId, InventoryItem> =
        get_variants_inventory(&ctx, ports.inventory_items(), &variants)
            .await?
            .into_iter()
            .map(|item| (item.variant(), item))
            .collect();
    let items = variants
        .into_iter()
        .map(|variant| match found.remove(&variant) {
            Some(item) => InventoryResponse::new(&item),
            None => Ok(InventoryResponse::untracked(variant)),
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(Json(ProductInventoryResponse { items }))
}
