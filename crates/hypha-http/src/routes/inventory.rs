//! Locations, inventory items and their stock (all live in `hypha-inventory`).

use std::collections::HashMap;

use hypha_core::Stamped;

use super::stamp_times;
use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{
    Ctx, FulfillmentId, InventoryItemId, Keyset, LocationId, MarketId, OrderId, ProductId,
    PurchaseOrderId, ReceiptId, SortKey, Timestamp, TransferId, UnitCost, VariantId,
};
use hypha_inventory::{
    AdjustOnHand, AdjustmentReason, CreateInventoryItem, CreateLocation, HasInventoryItems,
    HasLocations, HasStockMovements, HasTransfers, InventoryFilter, InventoryItem, Location,
    MovementFilter, SetOnHand, SetUnitCost, StockMovementEntry, StockReason, StockSource,
    TransferRepo, UpdateInventoryItem, UpdateLocation, adjust_on_hand, create_inventory_item,
    create_location, get_inventory_item, get_location, get_variant_inventory,
    get_variants_inventory, list_inventory_items, list_locations, list_stock_movements,
    set_on_hand, set_unit_cost, update_inventory_item, update_location,
};
use hypha_products::{CommandError as ProductsError, HasProducts, ProductRepo, get_product};
use hypha_purchasing::{
    CommandError as PurchasingError, HasPurchaseOrders, IncomingStock, PurchaseOrderRepo,
};
use serde::{Deserialize, Serialize};

use super::{
    address::{PostalAddressRequest, PostalAddressResponse},
    page::{ListQuery, PageParams, PageResponse, SortedFilters},
    payments::instant,
    rfc3339,
    stores::store_currency,
};
use crate::webhooks::{EventActor, actor_of};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    redact::{self, Family, Redacted},
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
    /// The product the variant belongs to; `null` if it was removed a moment ago.
    product: Option<ProductId>,
    tracked: bool,
    continue_selling: bool,
    levels: Vec<LevelResponse>,
    /// What one unit cost, in the store's base currency: the average over what came in,
    /// or what was set by hand. `null` until known. Absent without `purchasing:read`
    /// (staff included), and listed in `redacted`.
    unit_cost: Option<UnitCost>,
    /// Fields left out for this caller: `unit_cost` without `purchasing:read`, staff
    /// included. Absent when nothing was.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
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
    /// Units on their way here: what ordered purchase orders to this location still
    /// bring, and what transfers in transit to it carry. Not sellable until received. A
    /// location with only incoming units is listed with zero counts.
    incoming: u64,
    /// The soonest an order or transfer bringing some of `incoming` is expected, RFC 3339
    /// in UTC; `null` when nothing is coming or none says.
    next_expected_at: Option<String>,
}

impl LevelResponse {
    fn counted(location: LocationId, on_hand: i64, reserved: i64, available: i64) -> Self {
        Self {
            location,
            on_hand,
            reserved,
            available,
            incoming: 0,
            next_expected_at: None,
        }
    }
}

/// What's on its way, per variant: each variant's incoming units by location, from every
/// source of them (open purchase orders, transfers in transit).
pub(crate) type Incoming = HashMap<VariantId, Vec<IncomingStock>>;

/// The incoming units of `variants`, read once for them all. Derived on every read, never
/// stored on the items, so ordering or moving stock never touches what checkouts lock.
pub(crate) async fn incoming_of<P: HasPurchaseOrders + HasTransfers>(
    ports: &P,
    variants: &[VariantId],
) -> Result<Incoming, ApiError> {
    let ordered = ports
        .purchase_orders()
        .incoming(variants)
        .await
        .map_err(PurchasingError::from)?;
    let moving = ports
        .transfers()
        .incoming(variants)
        .await
        .map_err(hypha_inventory::CommandError::from)?;
    let found = ordered
        .into_iter()
        .chain(moving.into_iter().map(|t| IncomingStock {
            variant: t.variant,
            location: t.location,
            quantity: t.quantity,
            next_expected_at: t.next_expected_at,
        }));
    let mut incoming = Incoming::new();
    for stock in found {
        let at = incoming.entry(stock.variant).or_default();
        match at.iter_mut().find(|s| s.location == stock.location) {
            Some(same) => {
                same.quantity += stock.quantity;
                same.next_expected_at =
                    Timestamp::earliest(same.next_expected_at, stock.next_expected_at);
            }
            None => at.push(stock),
        }
    }
    Ok(incoming)
}

/// `levels` with `incoming`'s units beside them, adding a level for each location with
/// only incoming units, after the counted ones.
fn with_incoming(
    mut levels: Vec<LevelResponse>,
    incoming: &[IncomingStock],
) -> Result<Vec<LevelResponse>, ApiError> {
    for stock in incoming {
        let expected = stock.next_expected_at.map(rfc3339).transpose()?;
        match levels.iter_mut().find(|l| l.location == stock.location) {
            Some(level) => {
                level.incoming = stock.quantity;
                level.next_expected_at = expected;
            }
            None => levels.push(LevelResponse {
                incoming: stock.quantity,
                next_expected_at: expected,
                ..LevelResponse::counted(stock.location, 0, 0, 0)
            }),
        }
    }
    Ok(levels)
}

impl InventoryResponse {
    fn new(
        i: &InventoryItem,
        product: Option<ProductId>,
        incoming: &[IncomingStock],
    ) -> Result<Self, ApiError> {
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
            product,
            tracked: i.tracked(),
            continue_selling: i.continue_selling(),
            unit_cost: i.unit_cost(),
            redacted: None,
            levels: with_incoming(
                locations
                    .into_iter()
                    .map(|location| {
                        LevelResponse::counted(
                            location,
                            i.on_hand(location),
                            i.reserved(location),
                            i.available(location),
                        )
                    })
                    .collect(),
                incoming,
            )?,
        })
    }
}

/// An item as `ctx` may see it, with what's incoming: its cost only with
/// `purchasing:read`.
async fn item_view<P: HasPurchaseOrders + HasTransfers + HasProducts>(
    ports: &P,
    ctx: &Ctx,
    item: &InventoryItem,
) -> Result<Json<Redacted>, ApiError> {
    let incoming = incoming_of(ports, &[item.variant()]).await?;
    let incoming = incoming.get(&item.variant()).map_or(&[][..], Vec::as_slice);
    let product = product_of(ports, item.variant()).await?;
    Ok(Json(redact::one(
        ctx,
        Family::InventoryItem,
        InventoryResponse::new(item, product, incoming)?,
    )?))
}

/// The product `variant` belongs to, if it still exists.
async fn product_of<P: HasProducts>(
    ports: &P,
    variant: VariantId,
) -> Result<Option<ProductId>, ApiError> {
    let ids = ports.products().ids_of_variants(&[variant]).await?;
    Ok(ids.first().copied())
}

impl InventoryResponse {
    fn untracked(
        variant: VariantId,
        product: Option<ProductId>,
        incoming: &[IncomingStock],
    ) -> Result<Self, ApiError> {
        Ok(Self {
            id: None,
            variant,
            product,
            tracked: false,
            continue_selling: false,
            levels: with_incoming(Vec::new(), incoming)?,
            unit_cost: None,
            redacted: None,
            created_at: None,
            updated_at: None,
        })
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
) -> Result<(StatusCode, Json<Redacted>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateInventoryItem {
        variant: req.variant,
        tracked: req.tracked,
        continue_selling: req.continue_selling,
    };
    let item = create_inventory_item(&ctx, ports.inventory_items(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, item_view(&ports, &ctx, &item).await?))
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
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let item = get_inventory_item(&ctx, ports.inventory_items(), id).await?;
    item_view(&ports, &ctx, &item).await
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
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateInventoryItem {
        tracked: req.tracked,
        continue_selling: req.continue_selling,
    };
    let item = update_inventory_item(&ctx, ports.inventory_items(), id, input).await?;
    item_view(&ports, &ctx, &item).await
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
) -> Result<Json<Redacted>, ApiError> {
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
    item_view(&ports, &ctx, &item).await
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
) -> Result<Json<Redacted>, ApiError> {
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
    item_view(&ports, &ctx, &item).await
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetUnitCostRequest {
    /// What one unit cost, in the store's base currency; `null` clears it.
    unit_cost: Option<UnitCost>,
    /// Why, for the stock history: at most 500 characters.
    note: Option<String>,
}

/// Sets what one unit of an item cost, by hand: a `cost_adjustment` in its history and
/// `inventory_item.cost_changed`. Receiving goods changes it as they come in; this is for
/// stock bought outside purchase orders. The store's base currency must be set first
/// (`409 store_currency_required`), and the cost is in it (`422 cost_currency_mismatch`).
#[utoipa::path(
    put,
    path = "/admin/inventory-items/{id}/cost",
    params(("id" = InventoryItemId, Path)),
    request_body = SetUnitCostRequest,
    responses((status = 200, description = "OK", body = InventoryResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn set_cost<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(item): PathId<InventoryItemId>,
    body: JsonBody<SetUnitCostRequest>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let store_currency = store_currency(&state, &ports).await?;
    let input = SetUnitCost {
        item,
        unit_cost: req.unit_cost,
        note: req.note,
    };
    let item = set_unit_cost(&ctx, ports.inventory_items(), store_currency, input).await?;
    item_view(&ports, &ctx, &item).await
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
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let found = get_variant_inventory(&ctx, ports.inventory_items(), variant).await?;
    let incoming = incoming_of(&ports, &[variant]).await?;
    let incoming = incoming.get(&variant).map_or(&[][..], Vec::as_slice);
    let product = product_of(&ports, variant).await?;
    let response = match found {
        Some(item) => InventoryResponse::new(&item, product, incoming)?,
        None => InventoryResponse::untracked(variant, product, incoming)?,
    };
    Ok(Json(redact::one(&ctx, Family::InventoryItem, response)?))
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
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let product = get_product(&ctx, ports.products(), id).await?;
    let variants: Vec<VariantId> = product.variants().iter().map(|v| v.id()).collect();
    let incoming = incoming_of(&ports, &variants).await?;
    let mut found: HashMap<VariantId, InventoryItem> =
        get_variants_inventory(&ctx, ports.inventory_items(), &variants)
            .await?
            .into_iter()
            .map(|item| (item.variant(), item))
            .collect();
    let items = variants
        .into_iter()
        .map(|variant| {
            let incoming = incoming.get(&variant).map_or(&[][..], Vec::as_slice);
            match found.remove(&variant) {
                Some(item) => InventoryResponse::new(&item, Some(id), incoming),
                None => InventoryResponse::untracked(variant, Some(id), incoming),
            }
        })
        .collect::<Result<_, ApiError>>()?;
    // `items`, like a page's: each one redacted.
    Ok(Json(redact::page(
        &ctx,
        Family::InventoryItem,
        ProductInventoryResponse { items },
    )?))
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
    /// What these units cost, in the store's base currency: the item's average when
    /// they moved; for a `cost_adjustment`, the cost before it. `null` while there was
    /// none. Absent without `purchasing:read` (staff included), and listed in `redacted`.
    unit_cost: Option<UnitCost>,
    /// The item's unit cost after it. Absent without `purchasing:read`, like `unit_cost`.
    average_after: Option<UnitCost>,
    /// Fields left out for this caller: `unit_cost` and `average_after` without `purchasing:read`, staff
    /// included. Absent when nothing was.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
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
            unit_cost: m.unit_cost,
            average_after: m.average_after,
            redacted: None,
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
) -> Result<Json<Redacted>, ApiError> {
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
    let page = PageResponse::new(page, &scope, |m| m.position, StockMovementResponse::new)?;
    Ok(Json(redact::page(&ctx, Family::StockMovement, page)?))
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
    let incoming = incoming_of(&ports, &page_variants).await?;
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
            Ok(InventoryItemSummaryResponse {
                id: item.id,
                variant: item.variant,
                product: product.map(|p| p.id()),
                product_title: product.map(|p| p.title().to_string()),
                variant_title: product.and_then(|p| p.variant_title(item.variant)),
                sku: variant.and_then(|v| v.sku()).map(str::to_string),
                tracked: item.tracked,
                continue_selling: item.continue_selling,
                levels: with_incoming(
                    item.levels
                        .iter()
                        .map(|l| {
                            LevelResponse::counted(l.location, l.on_hand, l.reserved, l.available)
                        })
                        .collect(),
                    incoming.get(&item.variant).map_or(&[][..], Vec::as_slice),
                )?,
                on_hand: item.on_hand(),
                reserved: item.reserved(),
                available: item.available,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
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
