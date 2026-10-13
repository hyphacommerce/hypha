//! Locations, inventory items and their stock (all live in `hypha-inventory`).

use axum::{Json, extract::State, http::StatusCode};
use hypha_cart::CartPorts;
use hypha_core::{Ctx, InventoryItemId, LocationId, MarketId, VariantId};
use hypha_inventory::{
    AdjustOnHand, CreateInventoryItem, CreateLocation, InventoryItem, Location, SetOnHand,
    UpdateInventoryItem, UpdateLocation, adjust_on_hand, create_inventory_item, create_location,
    get_inventory_item, get_location, get_variant_inventory, set_on_hand, update_inventory_item,
    update_location,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

// ---------- locations ----------

#[derive(Deserialize)]
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateLocationRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    markets: Option<Vec<MarketId>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    priority: Option<u32>,
}

#[derive(Serialize)]
pub(super) struct LocationResponse {
    id: LocationId,
    name: String,
    markets: Vec<MarketId>,
    priority: u32,
}

impl From<&Location> for LocationResponse {
    fn from(l: &Location) -> Self {
        Self {
            id: l.id(),
            name: l.name().to_string(),
            markets: l.markets().to_vec(),
            priority: l.priority(),
        }
    }
}

pub(super) async fn create_location_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    JsonBody(req): JsonBody<CreateLocationRequest>,
) -> Result<(StatusCode, Json<LocationResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = CreateLocation {
        name: req.name,
        markets: req.markets,
        priority: req.priority,
    };
    let location = create_location(&ctx, ports.locations(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(LocationResponse::from(&location))))
}

pub(super) async fn fetch_location<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<LocationId>,
) -> Result<Json<LocationResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let location = get_location(&ctx, ports.locations(), id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("location_not_found"))?;
    Ok(Json(LocationResponse::from(&location)))
}

pub(super) async fn update_location_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<LocationId>,
    JsonBody(req): JsonBody<UpdateLocationRequest>,
) -> Result<Json<LocationResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = UpdateLocation {
        name: req.name,
        markets: req.markets,
        priority: req.priority,
    };
    let location = update_location(&ctx, ports.locations(), id, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("location_not_found"))?;
    Ok(Json(LocationResponse::from(&location)))
}

// ---------- items ----------

#[derive(Deserialize)]
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateInventoryItemRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    tracked: Option<bool>,
    #[serde(default, deserialize_with = "patch::non_null")]
    continue_selling: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetOnHandRequest {
    on_hand: i64,
    /// The count last seen. If it moved since (a sale, another count), the request
    /// fails with `on_hand_changed` instead of overwriting it.
    expected_on_hand: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdjustOnHandRequest {
    /// Negative to remove stock.
    delta: i64,
}

/// A variant's stock. `id` is `null` when stock was never set up for the variant,
/// which means untracked (always sellable).
#[derive(Serialize)]
pub(super) struct InventoryResponse {
    id: Option<InventoryItemId>,
    variant: VariantId,
    tracked: bool,
    continue_selling: bool,
    levels: Vec<LevelResponse>,
}

#[derive(Serialize)]
pub(super) struct LevelResponse {
    location: LocationId,
    on_hand: i64,
    /// Set aside for orders, not yet shipped.
    reserved: i64,
    /// `on_hand - reserved`; negative when more is reserved than is there.
    available: i64,
}

impl From<&InventoryItem> for InventoryResponse {
    fn from(i: &InventoryItem) -> Self {
        // Every location with a count or a reservation, counts first.
        let mut locations: Vec<LocationId> = i.levels().iter().map(|l| l.location).collect();
        for r in i.reservations() {
            if !locations.contains(&r.location) {
                locations.push(r.location);
            }
        }
        Self {
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
        }
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
        }
    }
}

pub(super) async fn create_item<S: Storage>(
    State(state): State<SharedState<S>>,
    JsonBody(req): JsonBody<CreateInventoryItemRequest>,
) -> Result<(StatusCode, Json<InventoryResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = CreateInventoryItem {
        variant: req.variant,
        tracked: req.tracked,
        continue_selling: req.continue_selling,
    };
    let item = create_inventory_item(&ctx, ports.inventory_items(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(InventoryResponse::from(&item))))
}

pub(super) async fn fetch_item<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<InventoryItemId>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let item = get_inventory_item(&ctx, ports.inventory_items(), id).await?;
    Ok(Json(InventoryResponse::from(&item)))
}

pub(super) async fn update_item<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<InventoryItemId>,
    JsonBody(req): JsonBody<UpdateInventoryItemRequest>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = UpdateInventoryItem {
        tracked: req.tracked,
        continue_selling: req.continue_selling,
    };
    let item = update_inventory_item(&ctx, ports.inventory_items(), id, input).await?;
    Ok(Json(InventoryResponse::from(&item)))
}

pub(super) async fn set_level<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId((item, location)): PathId<(InventoryItemId, LocationId)>,
    JsonBody(req): JsonBody<SetOnHandRequest>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = SetOnHand {
        item,
        location,
        quantity: req.on_hand,
        expected: req.expected_on_hand,
    };
    let item = set_on_hand(&ctx, ports.inventory_items(), ports.locations(), input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("location_not_found"))?;
    Ok(Json(InventoryResponse::from(&item)))
}

pub(super) async fn adjust_level<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId((item, location)): PathId<(InventoryItemId, LocationId)>,
    JsonBody(req): JsonBody<AdjustOnHandRequest>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = AdjustOnHand {
        item,
        location,
        delta: req.delta,
    };
    let item = adjust_on_hand(&ctx, ports.inventory_items(), ports.locations(), input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("location_not_found"))?;
    Ok(Json(InventoryResponse::from(&item)))
}

/// A variant's stock, whether or not it has an item (design note 0004, I3).
pub(super) async fn fetch_variant_inventory<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(variant): PathId<VariantId>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let response = match get_variant_inventory(&ctx, ports.inventory_items(), variant).await? {
        Some(item) => InventoryResponse::from(&item),
        None => InventoryResponse::untracked(variant),
    };
    Ok(Json(response))
}
