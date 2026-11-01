//! Locations, inventory items and their stock (all live in `hypha-inventory`).

use hypha_core::Stamped;

use super::stamp_times;
use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{
    FulfillmentId, InventoryItemId, Keyset, LocationId, MarketId, OrderId, ProductId,
    PurchaseOrderId, ReceiptId, SortKey, Timestamp, TransferId, VariantId,
};
use hypha_inventory::{
    AdjustOnHand, AdjustmentReason, CreateInventoryItem, CreateLocation, HasInventoryItems,
    HasLocations, HasStockMovements, InventoryFilter, InventoryItem, Location, MovementFilter,
    SetOnHand, StockMovementEntry, StockReason, StockSource, UpdateInventoryItem, UpdateLocation,
    adjust_on_hand, create_inventory_item, create_location, get_inventory_item, get_location,
    get_variant_inventory, get_variants_inventory, list_inventory_items, list_locations,
    list_stock_movements, set_on_hand, update_inventory_item, update_location,
};
use hypha_products::{CommandError as ProductsError, HasProducts, ProductRepo, get_product};
use serde::{Deserialize, Serialize};

use super::{
    address::{PostalAddressRequest, PostalAddressResponse},
    page::{ListQuery, PageParams, PageResponse, SortedFilters},
    payments::instant,
    rfc3339,
};
use crate::webhooks::{EventActor, actor_of};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

// ---------- locations ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateLocationRequest {
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
pub struct UpdateLocationRequest {
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
pub struct LocationResponse {
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
pub async fn create_location_handler<S: Storage>(
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
pub struct LocationList {
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
pub async fn list_locations_handler<S: Storage>(
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
pub async fn fetch_location<S: Storage>(
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
pub async fn update_location_handler<S: Storage>(
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
pub struct CreateInventoryItemRequest {
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
pub struct UpdateInventoryItemRequest {
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
pub struct SetOnHandRequest {
    on_hand: i64,
    /// The count last seen. If it moved since (a sale, another count), the request
    /// fails with `on_hand_changed` instead of overwriting it.
    expected_on_hand: Option<i64>,
    /// Why, for the stock history (recorded as `counted`): at most 500 characters.
    note: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdjustOnHandRequest {
    /// Negative to remove stock.
    delta: i64,
    /// Why, for the stock history; `correction` when absent.
    #[serde(default)]
    reason: AdjustmentReasonDto,
    /// At most 500 characters.
    note: Option<String>,
}

/// Why units were added or removed by hand.
#[derive(Debug, Clone, Copy, Default, Deserialize, utoipa::ToSchema)]
#[schema(as = AdjustmentReason)]
#[serde(rename_all = "snake_case")]
pub enum AdjustmentReasonDto {
    #[default]
    Correction,
    Damaged,
    /// Theft or loss.
    Shrinkage,
    /// Samples and gifts.
    Promotion,
    Other,
}

impl From<AdjustmentReasonDto> for AdjustmentReason {
    fn from(r: AdjustmentReasonDto) -> Self {
        match r {
            AdjustmentReasonDto::Correction => AdjustmentReason::Correction,
            AdjustmentReasonDto::Damaged => AdjustmentReason::Damaged,
            AdjustmentReasonDto::Shrinkage => AdjustmentReason::Shrinkage,
            AdjustmentReasonDto::Promotion => AdjustmentReason::Promotion,
            AdjustmentReasonDto::Other => AdjustmentReason::Other,
        }
    }
}

/// A variant's stock. `id` is `null` when stock was never set up for the variant,
/// which means untracked (always sellable).
#[derive(Serialize, utoipa::ToSchema)]
pub struct InventoryResponse {
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
pub struct LevelResponse {
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
pub async fn create_item<S: Storage>(
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
pub async fn fetch_item<S: Storage>(
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
pub async fn update_item<S: Storage>(
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

/// Sets the stock on hand at a location: a `counted` movement in its history.
#[utoipa::path(
    put,
    path = "/admin/inventory-items/{id}/levels/{location_id}",
    params(("id" = InventoryItemId, Path), ("location_id" = LocationId, Path)),
    request_body = SetOnHandRequest,
    responses((status = 200, description = "OK", body = InventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub async fn set_level<S: Storage>(
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
        note: req.note,
    };
    let item = set_on_hand(&ctx, ports.inventory_items(), ports.locations(), input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("location_not_found"))?;
    Ok(Json(InventoryResponse::new(&item)?))
}

/// Adjusts the stock on hand at a location by a delta, for a reason kept in its history.
#[utoipa::path(
    post,
    path = "/admin/inventory-items/{id}/levels/{location_id}/adjustments",
    params(("id" = InventoryItemId, Path), ("location_id" = LocationId, Path)),
    request_body = AdjustOnHandRequest,
    responses((status = 200, description = "OK", body = InventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:write"]))),
)]
pub async fn adjust_level<S: Storage>(
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
        reason: req.reason.into(),
        note: req.note,
    };
    let item = adjust_on_hand(&ctx, ports.inventory_items(), ports.locations(), input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("location_not_found"))?;
    Ok(Json(InventoryResponse::new(&item)?))
}

/// A variant's stock, whether or not it has an item.
#[utoipa::path(
    get,
    path = "/admin/variants/{id}/inventory",
    params(("id" = VariantId, Path)),
    responses((status = 200, description = "OK", body = InventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["inventory:read"]))),
)]
pub async fn fetch_variant_inventory<S: Storage>(
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
pub struct ProductInventoryResponse {
    /// One per variant, as `GET /admin/variants/{id}/inventory` answers it.
    items: Vec<InventoryResponse>,
}

/// A product's stock, every variant at once: what the product editor shows, in one call
/// instead of one per variant.
#[utoipa::path(
    get,
    path = "/admin/products/{id}/inventory",
    params(("id" = ProductId, Path)),
    responses((status = 200, description = "OK", body = ProductInventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read", "inventory:read"]))),
)]
pub async fn fetch_product_inventory<S: Storage>(
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

// ---------- stock history ----------

/// Why stock moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = StockReason)]
#[serde(rename_all = "snake_case")]
pub enum StockReasonDto {
    /// Goods arrived from a supplier (negative when a receipt is taken back).
    Received,
    TransferOut,
    TransferIn,
    /// Units left in a fulfilment.
    Sold,
    /// A cancelled fulfilment's units came back.
    Restocked,
    /// The count was set: the delta is the new count less the old.
    Counted,
    Correction,
    Damaged,
    /// Theft, loss, or lost in transit.
    Shrinkage,
    /// Samples and gifts.
    Promotion,
    Other,
    /// The unit cost changed; no units moved.
    CostAdjustment,
    /// What the location held when the history started.
    Opening,
}

impl From<StockReason> for StockReasonDto {
    fn from(r: StockReason) -> Self {
        match r {
            StockReason::Received => StockReasonDto::Received,
            StockReason::TransferOut => StockReasonDto::TransferOut,
            StockReason::TransferIn => StockReasonDto::TransferIn,
            StockReason::Sold => StockReasonDto::Sold,
            StockReason::Restocked => StockReasonDto::Restocked,
            StockReason::Counted => StockReasonDto::Counted,
            StockReason::Correction => StockReasonDto::Correction,
            StockReason::Damaged => StockReasonDto::Damaged,
            StockReason::Shrinkage => StockReasonDto::Shrinkage,
            StockReason::Promotion => StockReasonDto::Promotion,
            StockReason::Other => StockReasonDto::Other,
            StockReason::CostAdjustment => StockReasonDto::CostAdjustment,
            StockReason::Opening => StockReasonDto::Opening,
        }
    }
}

impl From<StockReasonDto> for StockReason {
    fn from(r: StockReasonDto) -> Self {
        match r {
            StockReasonDto::Received => StockReason::Received,
            StockReasonDto::TransferOut => StockReason::TransferOut,
            StockReasonDto::TransferIn => StockReason::TransferIn,
            StockReasonDto::Sold => StockReason::Sold,
            StockReasonDto::Restocked => StockReason::Restocked,
            StockReasonDto::Counted => StockReason::Counted,
            StockReasonDto::Correction => StockReason::Correction,
            StockReasonDto::Damaged => StockReason::Damaged,
            StockReasonDto::Shrinkage => StockReason::Shrinkage,
            StockReasonDto::Promotion => StockReason::Promotion,
            StockReasonDto::Other => StockReason::Other,
            StockReasonDto::CostAdjustment => StockReason::CostAdjustment,
            StockReasonDto::Opening => StockReason::Opening,
        }
    }
}

/// What caused a movement, by the documents behind it.
#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = StockSource)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StockSourceDto {
    /// Someone set or adjusted the count.
    Manual,
    Fulfillment {
        order: OrderId,
        fulfillment: FulfillmentId,
    },
    Receipt {
        purchase_order: PurchaseOrderId,
        receipt: ReceiptId,
    },
    /// A cost added to a purchase order after its goods arrived.
    PurchaseOrder {
        purchase_order: PurchaseOrderId,
    },
    Transfer {
        transfer: TransferId,
    },
    /// Written when the history started.
    Migration,
}

impl From<StockSource> for StockSourceDto {
    fn from(s: StockSource) -> Self {
        match s {
            StockSource::Manual => StockSourceDto::Manual,
            StockSource::Fulfillment { order, fulfillment } => {
                StockSourceDto::Fulfillment { order, fulfillment }
            }
            StockSource::Receipt {
                purchase_order,
                receipt,
            } => StockSourceDto::Receipt {
                purchase_order,
                receipt,
            },
            StockSource::PurchaseOrder(purchase_order) => {
                StockSourceDto::PurchaseOrder { purchase_order }
            }
            StockSource::Transfer(transfer) => StockSourceDto::Transfer { transfer },
            StockSource::Migration => StockSourceDto::Migration,
        }
    }
}

/// One change to a count, as the stock history keeps it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StockMovementResponse {
    /// Its place in the ledger: an item's later changes have higher positions.
    position: i64,
    /// `null` for a change that moved no units (a cost).
    location: Option<LocationId>,
    /// Units added (positive) or removed (negative).
    delta: i64,
    /// On hand at the location after it.
    after: Option<i64>,
    reason: StockReasonDto,
    source: StockSourceDto,
    note: Option<String>,
    /// Who made the change.
    actor: EventActor,
    /// When it was written, RFC 3339.
    at: String,
}

impl StockMovementResponse {
    fn new(m: StockMovementEntry) -> Result<Self, ApiError> {
        Ok(Self {
            position: m.position,
            location: m.location,
            delta: m.delta,
            after: m.after,
            reason: m.reason.into(),
            source: m.source.into(),
            note: m.note,
            actor: actor_of(m.actor),
            at: rfc3339(m.at)?,
        })
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct StockMovementFilters {
    /// Only movements at this location.
    #[param(value_type = Option<LocationId>)]
    location: Option<LocationId>,
    /// Only movements for this reason.
    #[param(inline)]
    reason: Option<StockReasonDto>,
    /// Only movements at or after this instant (RFC 3339).
    #[serde(default, deserialize_with = "instant")]
    #[param(value_type = Option<String>)]
    from: Option<Timestamp>,
    /// Only movements before this instant (RFC 3339).
    #[serde(default, deserialize_with = "instant")]
    #[param(value_type = Option<String>)]
    to: Option<Timestamp>,
}

/// A page of stock movements (named for the documents: utoipa's `body =` takes a plain
/// name).
type StockMovementPage = PageResponse<StockMovementResponse>;

/// An item's stock history, newest first: every change to a count, why, what caused it
/// and who made it. Kept for good.
#[utoipa::path(
    get,
    path = "/admin/inventory-items/{id}/movements",
    params(("id" = InventoryItemId, Path), PageParams, StockMovementFilters),
    responses((status = 200, description = "OK", body = StockMovementPage)),
    extensions(("x-hypha-scopes" = json!(["inventory:read"]))),
)]
pub async fn list_movements<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(item): PathId<InventoryItemId>,
    query: ListQuery<StockMovementFilters>,
) -> Result<Json<StockMovementPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse()?;
    let filter = MovementFilter {
        location: filters.location,
        reason: filters.reason.map(Into::into),
        from: filters.from,
        to: filters.to,
    };
    let page = list_stock_movements(
        &ctx,
        ports.inventory_items(),
        ports.stock_movements(),
        item,
        &filter,
        page,
    )
    .await?;
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |m| m.position,
        StockMovementResponse::new,
    )?))
}

// ---------- across products ----------

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct InventoryItemFilters {
    /// Only tracked items with less than this many available (strictly below): across
    /// locations, or at `location` when given. Untracked stock never runs low.
    below: Option<i64>,
    /// Only items with stock or a reservation at this location; `below` and the order then
    /// count this location alone. An unknown one is `422 location_not_found`.
    #[param(value_type = Option<LocationId>)]
    location: Option<LocationId>,
    /// Only this product's variants. An unknown one is `422 product_not_found`.
    #[param(value_type = Option<ProductId>)]
    product: Option<ProductId>,
    /// Only the variant with exactly this SKU (case-sensitive, no prefix match); when no
    /// variant has it, the list is empty.
    sku: Option<String>,
}

impl SortedFilters for InventoryItemFilters {
    fn fits(&self, key: &SortKey) -> bool {
        InventoryFilter::fits(key)
    }
}

/// An inventory item in a list across products: what it stocks, and the product and
/// variant it belongs to.
#[derive(Serialize, utoipa::ToSchema)]
pub struct InventoryItemSummaryResponse {
    id: InventoryItemId,
    variant: VariantId,
    /// The product the variant belongs to; `null` if it was removed a moment ago.
    product: Option<ProductId>,
    product_title: Option<String>,
    /// The variant's option values ("M / Blue"); `null` for a simple product.
    variant_title: Option<String>,
    sku: Option<String>,
    tracked: bool,
    continue_selling: bool,
    /// Every location with a count or a reservation, by location ID.
    levels: Vec<LevelResponse>,
    /// On hand across locations.
    on_hand: i64,
    /// Reserved across locations.
    reserved: i64,
    /// Available across locations.
    available: i64,
}

/// A page of inventory items (named for the documents: utoipa's `body =` takes a plain
/// name).
pub(super) type InventoryItemSummaryPage = PageResponse<InventoryItemSummaryResponse>;

/// Inventory items across products, lowest available first (at `location` when given),
/// then by ID: what's running low. Items whose variant was removed are left out. Each
/// names its product, title and SKU, so it takes `products:read` too.
#[utoipa::path(
    get,
    path = "/admin/inventory-items",
    params(PageParams, InventoryItemFilters),
    responses((status = 200, description = "OK", body = InventoryItemSummaryPage)),
    extensions(("x-hypha-scopes" = json!(["inventory:read", "products:read"]))),
)]
pub async fn list_items<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<InventoryItemFilters>,
) -> Result<Json<InventoryItemSummaryPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    ctx.require(hypha_core::Scope::ProductsRead)
        .map_err(ProductsError::from)?;
    let (page, filters, scope) = query.parse_sorted()?;
    if let Some(location) = filters.location {
        get_location(&ctx, ports.locations(), location).await?;
    }
    let filter = InventoryFilter {
        below: filters.below,
        location: filters.location,
        variants: variants_of(&ports, filters.product, filters.sku.as_deref()).await?,
    };
    let page = list_inventory_items(&ctx, ports.inventory_items(), &filter, page).await?;

    // The page's products, in two reads however many items.
    let page_variants: Vec<VariantId> = page.items.iter().map(|i| i.variant).collect();
    let catalog = ports.products();
    let owners = catalog
        .ids_of_variants(&page_variants)
        .await
        .map_err(ProductsError::from)?;
    let products = catalog
        .get_many(&owners)
        .await
        .map_err(ProductsError::from)?;
    let items = page
        .items
        .iter()
        .map(|item| {
            let product = products
                .iter()
                .find(|p| p.variants().iter().any(|v| v.id() == item.variant));
            let variant =
                product.and_then(|p| p.variants().iter().find(|v| v.id() == item.variant));
            InventoryItemSummaryResponse {
                id: item.id,
                variant: item.variant,
                product: product.map(|p| p.id()),
                product_title: product.map(|p| p.title().to_string()),
                variant_title: product.and_then(|p| p.variant_title(item.variant)),
                sku: variant.and_then(|v| v.sku()).map(str::to_string),
                tracked: item.tracked,
                continue_selling: item.continue_selling,
                levels: item
                    .levels
                    .iter()
                    .map(|l| LevelResponse {
                        location: l.location,
                        on_hand: l.on_hand,
                        reserved: l.reserved,
                        available: l.available,
                    })
                    .collect(),
                on_hand: item.on_hand(),
                reserved: item.reserved(),
                available: item.available,
            }
        })
        .collect();
    let response = PageResponse::sorted(
        &page,
        &scope,
        |i| Keyset {
            key: filter.key(i),
            id: i.id,
        },
        items,
    )?;
    Ok(Json(response))
}

/// The variants the `product` and `sku` filters leave (both: the product's variant with
/// that SKU), or `None` without either. Products only name the items here, so the list
/// takes `inventory:read` alone: the catalog is read through its repository, not its
/// commands.
async fn variants_of<P: HasProducts>(
    ports: &P,
    product: Option<ProductId>,
    sku: Option<&str>,
) -> Result<Option<Vec<VariantId>>, ApiError> {
    let mut variants: Option<Vec<VariantId>> = None;
    if let Some(id) = product {
        let found = ports
            .products()
            .get(id)
            .await
            .map_err(ProductsError::from)?;
        let product = found.ok_or_else(|| {
            ApiError::from(ProductsError::ProductNotFound(id)).unprocessable_if("product_not_found")
        })?;
        variants = Some(product.variants().iter().map(|v| v.id()).collect());
    }
    if let Some(sku) = sku {
        let found = ports
            .products()
            .get_by_sku(sku)
            .await
            .map_err(ProductsError::from)?;
        let of_sku = found.and_then(|p| {
            p.variants()
                .iter()
                .find(|v| v.sku() == Some(sku))
                .map(|v| v.id())
        });
        variants = Some(
            of_sku
                .into_iter()
                .filter(|v| variants.as_ref().is_none_or(|of| of.contains(v)))
                .collect(),
        );
    }
    Ok(variants)
}
