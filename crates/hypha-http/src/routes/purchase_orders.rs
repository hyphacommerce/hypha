//! Purchase orders: what the store orders from its suppliers, to which location, and how
//! much of it has arrived.

use std::collections::HashMap;

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{
    Currency, ExchangeRate, Keyset, LocationId, Money, PurchaseOrderId, SortKey, Stamped,
    SupplierId, UnitCost, VariantId,
};
use hypha_inventory::{HasLocations, LocationRepo};
use hypha_products::{HasProducts, ProductRepo};
use hypha_purchasing::{
    CostAllocation, CostDraft, CostKind, HasSuppliers, LineInput, NewPurchaseOrder, PurchaseOrder,
    PurchaseOrderFilter, PurchaseOrderSort, PurchaseOrderStatus, PurchaseOrderSummary,
    ReceiptStatus, SendPurchaseOrder, ShipTo, SupplierRepo, UpdatePurchaseOrder, VariantInfo,
    cancel_purchase_order, create_purchase_order, delete_purchase_order, get_purchase_order,
    list_purchase_orders, order_purchase_order, send_purchase_order,
};
use hypha_receiving::{close_purchase_order, update_purchase_order};
use serde::{Deserialize, Serialize};

use super::{
    markets::Instant,
    page::{ListQuery, PageParams, PageResponse, SortedFilters},
    rfc3339, stamp_times,
    stores::store_currency,
    suppliers::{PaymentTermsDto, terms_dto},
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

/// Where a purchase order is in its life.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PurchaseOrderStatusDto {
    /// Being written: anything may change, and it may be deleted.
    Draft,
    /// Placed with the supplier: goods may arrive.
    Ordered,
    /// Done: everything arrived, or what didn't was cancelled.
    Closed,
    /// Called off before anything arrived.
    Cancelled,
}

impl From<PurchaseOrderStatus> for PurchaseOrderStatusDto {
    fn from(s: PurchaseOrderStatus) -> Self {
        match s {
            PurchaseOrderStatus::Draft => Self::Draft,
            PurchaseOrderStatus::Ordered => Self::Ordered,
            PurchaseOrderStatus::Closed => Self::Closed,
            PurchaseOrderStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<PurchaseOrderStatusDto> for PurchaseOrderStatus {
    fn from(s: PurchaseOrderStatusDto) -> Self {
        match s {
            PurchaseOrderStatusDto::Draft => Self::Draft,
            PurchaseOrderStatusDto::Ordered => Self::Ordered,
            PurchaseOrderStatusDto::Closed => Self::Closed,
            PurchaseOrderStatusDto::Cancelled => Self::Cancelled,
        }
    }
}

/// How much of a purchase order has arrived.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatusDto {
    /// Nothing received or rejected yet.
    None,
    /// Some, not all.
    Partial,
    /// Every line accounted for by receipts.
    Received,
}

impl From<ReceiptStatus> for ReceiptStatusDto {
    fn from(s: ReceiptStatus) -> Self {
        match s {
            ReceiptStatus::None => Self::None,
            ReceiptStatus::Partial => Self::Partial,
            ReceiptStatus::Received => Self::Received,
        }
    }
}

impl From<ReceiptStatusDto> for ReceiptStatus {
    fn from(s: ReceiptStatusDto) -> Self {
        match s {
            ReceiptStatusDto::None => Self::None,
            ReceiptStatusDto::Partial => Self::Partial,
            ReceiptStatusDto::Received => Self::Received,
        }
    }
}

/// What an extra cost is for.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CostKindDto {
    Shipping,
    Duties,
    Fees,
    Other,
}

impl From<CostKind> for CostKindDto {
    fn from(k: CostKind) -> Self {
        match k {
            CostKind::Shipping => Self::Shipping,
            CostKind::Duties => Self::Duties,
            CostKind::Fees => Self::Fees,
            CostKind::Other => Self::Other,
        }
    }
}

impl From<CostKindDto> for CostKind {
    fn from(k: CostKindDto) -> Self {
        match k {
            CostKindDto::Shipping => Self::Shipping,
            CostKindDto::Duties => Self::Duties,
            CostKindDto::Fees => Self::Fees,
            CostKindDto::Other => Self::Other,
        }
    }
}

/// How an extra cost is spread over the lines' unit costs: by each line's value
/// (`by_value`, the default), by its quantity, or not at all (`none`: an expense on the
/// total only).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CostAllocationDto {
    #[default]
    ByValue,
    ByQuantity,
    None,
}

impl From<CostAllocation> for CostAllocationDto {
    fn from(a: CostAllocation) -> Self {
        match a {
            CostAllocation::ByValue => Self::ByValue,
            CostAllocation::ByQuantity => Self::ByQuantity,
            CostAllocation::None => Self::None,
        }
    }
}

impl From<CostAllocationDto> for CostAllocation {
    fn from(a: CostAllocationDto) -> Self {
        match a {
            CostAllocationDto::ByValue => Self::ByValue,
            CostAllocationDto::ByQuantity => Self::ByQuantity,
            CostAllocationDto::None => Self::None,
        }
    }
}

/// One variant on a purchase order.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PurchaseOrderLineResponse {
    variant: VariantId,
    /// Snapshots of the variant as it was when added: it may be removed later.
    product_title: String,
    variant_title: Option<String>,
    sku: Option<String>,
    /// The supplier's code for it.
    supplier_sku: Option<String>,
    quantity: u32,
    /// Per unit, in the order's currency, net of tax.
    unit_cost: UnitCost,
    received: u32,
    rejected: u32,
    /// Cancelled when the order was closed early.
    cancelled: u32,
    /// Still to arrive: quantity less received, rejected and cancelled.
    outstanding: u32,
    /// Quantity less cancelled, times the unit cost, in the order's currency.
    total: Money,
    /// What one unit costs to bring in, in the store's currency: its unit cost and its
    /// share of the extra costs, at the order's exchange rate. `null` without the store's
    /// currency or a needed rate.
    landed_unit_cost: Option<UnitCost>,
}

/// Something paid to bring the goods in besides their price.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PurchaseOrderCostResponse {
    kind: CostKindDto,
    description: Option<String>,
    /// In the order's currency or the store's.
    amount: Money,
    allocation: CostAllocationDto,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PurchaseOrderResponse {
    id: PurchaseOrderId,
    /// The human number, shown as "PO-1001".
    number: i64,
    supplier: SupplierId,
    /// As it was when ordered; the supplier's current name on a draft.
    supplier_name: String,
    /// Where the goods go; required to order.
    destination: Option<LocationId>,
    currency: Currency,
    /// One unit of the order's currency in the store's; only when they differ.
    exchange_rate: Option<ExchangeRate>,
    /// The store's base currency, which landed costs are in.
    store_currency: Option<Currency>,
    payment_terms: Option<PaymentTermsDto>,
    /// RFC 3339: when the goods are due.
    expected_at: Option<String>,
    /// The supplier's own number for the order.
    supplier_reference: Option<String>,
    /// Printed and emailed with the order.
    note_to_supplier: Option<String>,
    /// For staff only.
    note: Option<String>,
    lines: Vec<PurchaseOrderLineResponse>,
    costs: Vec<PurchaseOrderCostResponse>,
    /// For the total only: purchase tax is in no cost.
    tax: Option<Money>,
    /// The lines, in the order's currency.
    subtotal: Money,
    /// Extra costs in the order's currency.
    costs_total: Money,
    /// Subtotal, extra costs and tax, in the order's currency.
    total: Money,
    /// Extra costs paid in the store's currency, when any are.
    costs_in_store_currency: Option<Money>,
    status: PurchaseOrderStatusDto,
    receipt_status: ReceiptStatusDto,
    /// RFC 3339.
    ordered_at: Option<String>,
    /// RFC 3339: when it was last emailed to the supplier.
    sent_at: Option<String>,
    closed_at: Option<String>,
    cancelled_at: Option<String>,
    /// Edits since it was ordered.
    revision: u32,
    created_at: String,
    updated_at: String,
}

fn times(at: Option<hypha_core::Timestamp>) -> Result<Option<String>, ApiError> {
    at.map(rfc3339).transpose()
}

impl PurchaseOrderResponse {
    fn new(
        po: &PurchaseOrder,
        supplier_name: String,
        store: Option<Currency>,
    ) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(po.stamps())?;
        let totals = po
            .totals(store)
            .map_err(hypha_purchasing::CommandError::from)?;
        // A landed cost past the largest amount is shown as unknown, not as an error.
        let landed = store
            .map(|store| po.landed_costs(store, po.exchange_rate()).ok())
            .unwrap_or_default()
            .unwrap_or_else(|| vec![None; po.lines().len()]);
        let lines = po
            .lines()
            .iter()
            .zip(landed)
            .map(|(l, landed)| {
                Ok(PurchaseOrderLineResponse {
                    variant: l.variant(),
                    product_title: l.product_title().to_string(),
                    variant_title: l.variant_title().map(str::to_string),
                    sku: l.sku().map(str::to_string),
                    supplier_sku: l.supplier_sku().map(str::to_string),
                    quantity: l.quantity(),
                    unit_cost: l.unit_cost(),
                    received: l.received(),
                    rejected: l.rejected(),
                    cancelled: l.cancelled(),
                    outstanding: l.outstanding(),
                    total: l.total()?,
                    landed_unit_cost: landed,
                })
            })
            .collect::<Result<_, hypha_purchasing::PurchasingError>>()
            .map_err(hypha_purchasing::CommandError::from)?;
        Ok(Self {
            id: po.id(),
            number: po.number().value(),
            supplier: po.supplier(),
            supplier_name: po.supplier_name().map_or(supplier_name, str::to_string),
            destination: po.destination(),
            currency: po.currency(),
            exchange_rate: po.exchange_rate(),
            store_currency: store,
            payment_terms: po.payment_terms().map(terms_dto),
            expected_at: times(po.expected_at())?,
            supplier_reference: po.supplier_reference().map(str::to_string),
            note_to_supplier: po.note_to_supplier().map(str::to_string),
            note: po.note().map(str::to_string),
            lines,
            costs: po
                .costs()
                .iter()
                .map(|c| PurchaseOrderCostResponse {
                    kind: c.kind().into(),
                    description: c.description().map(str::to_string),
                    amount: c.amount(),
                    allocation: c.allocation().into(),
                })
                .collect(),
            tax: po.tax(),
            subtotal: totals.subtotal,
            costs_total: totals.costs,
            total: totals.total,
            costs_in_store_currency: totals.costs_in_store_currency,
            status: po.status().into(),
            receipt_status: po.receipt_status().into(),
            ordered_at: times(po.ordered_at())?,
            sent_at: times(po.sent_at())?,
            closed_at: times(po.closed_at())?,
            cancelled_at: times(po.cancelled_at())?,
            revision: po.revision(),
            created_at,
            updated_at,
        })
    }
}

/// The supplier's current name, for an order not yet ordered (which keeps no snapshot).
async fn supplier_name<P: HasSuppliers>(ports: &P, id: SupplierId) -> Result<String, ApiError> {
    Ok(ports
        .suppliers()
        .get(id)
        .await
        .map_err(hypha_purchasing::CommandError::from)?
        .map(|s| s.name().to_string())
        .unwrap_or_default())
}

pub(crate) async fn respond<S: Storage>(
    state: &SharedState<S>,
    ports: &S::Ports<'_>,
    po: &PurchaseOrder,
) -> Result<PurchaseOrderResponse, ApiError> {
    let name = match po.supplier_name() {
        Some(name) => name.to_string(),
        None => supplier_name(ports, po.supplier()).await?,
    };
    let store = store_currency(state, ports).await?;
    PurchaseOrderResponse::new(po, name, store)
}

/// What the lines' variants are: their products' titles and SKUs, read as the system (a
/// line keeps them as snapshots). A variant the store lacks is left out, and the command
/// refuses its line.
pub(crate) async fn variant_infos<P: HasProducts>(
    ports: &P,
    variants: &[VariantId],
) -> Result<Vec<VariantInfo>, ApiError> {
    if variants.is_empty() {
        return Ok(Vec::new());
    }
    let owners = ports
        .products()
        .ids_of_variants(variants)
        .await
        .map_err(hypha_products::CommandError::from)?;
    let products = ports
        .products()
        .get_many(&owners)
        .await
        .map_err(hypha_products::CommandError::from)?;
    let mut infos = Vec::new();
    for product in &products {
        for variant in product.variants() {
            if variants.contains(&variant.id()) {
                infos.push(VariantInfo {
                    variant: variant.id(),
                    product_title: product.title().to_string(),
                    variant_title: product.variant_title(variant.id()),
                    sku: variant.sku().map(str::to_string),
                });
            }
        }
    }
    Ok(infos)
}

/// A destination must be one of the store's locations.
async fn check_location<P: HasLocations>(
    ports: &P,
    location: Option<LocationId>,
) -> Result<(), ApiError> {
    let Some(location) = location else {
        return Ok(());
    };
    let found = ports
        .locations()
        .get(location)
        .await
        .map_err(hypha_inventory::CommandError::from)?;
    if found.is_none() {
        return Err(hypha_inventory::CommandError::LocationNotFound(location).into());
    }
    Ok(())
}

/// A line, sent.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PurchaseOrderLineRequest {
    variant: VariantId,
    /// At least 1.
    quantity: u32,
    /// Per unit, in the order's currency, net of tax. Absent: the line's current cost,
    /// else the supplier's catalogue price, else the cost on the supplier's last order
    /// with this variant (`422 unit_cost_required` when none is in the order's currency).
    #[serde(default)]
    unit_cost: Option<UnitCost>,
    /// The supplier's code; absent keeps it or takes the catalogue's, `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    supplier_sku: Option<Option<String>>,
}

impl From<PurchaseOrderLineRequest> for LineInput {
    fn from(l: PurchaseOrderLineRequest) -> Self {
        LineInput {
            variant: l.variant,
            quantity: l.quantity,
            unit_cost: l.unit_cost,
            supplier_sku: l.supplier_sku,
        }
    }
}

/// An extra cost, sent.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PurchaseOrderCostRequest {
    kind: CostKindDto,
    #[serde(default)]
    description: Option<String>,
    /// Not negative; in the order's currency or the store's.
    amount: Money,
    #[serde(default)]
    allocation: CostAllocationDto,
}

impl From<PurchaseOrderCostRequest> for CostDraft {
    fn from(c: PurchaseOrderCostRequest) -> Self {
        CostDraft {
            kind: c.kind.into(),
            description: c.description,
            amount: c.amount,
            allocation: c.allocation.into(),
        }
    }
}

fn variants_of(lines: &[PurchaseOrderLineRequest]) -> Vec<VariantId> {
    lines.iter().map(|l| l.variant).collect()
}

/// A purchase order as a list shows it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PurchaseOrderSummaryResponse {
    id: PurchaseOrderId,
    /// Shown as "PO-1001".
    number: i64,
    supplier: SupplierId,
    /// As it was when ordered; the supplier's current name on a draft.
    supplier_name: String,
    destination: Option<LocationId>,
    currency: Currency,
    status: PurchaseOrderStatusDto,
    receipt_status: ReceiptStatusDto,
    /// RFC 3339.
    expected_at: Option<String>,
    ordered_at: Option<String>,
    line_count: usize,
    /// Subtotal, extra costs in its currency and tax; `null` past the largest amount.
    total: Option<Money>,
    created_at: Option<String>,
}

/// The order of a purchase order list. Default `number_desc` (newest first);
/// `expected_at` puts the soonest due first and those with no date last.
#[derive(Debug, Clone, Copy, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PurchaseOrderSortDto {
    NumberDesc,
    NumberAsc,
    ExpectedAt,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct PurchaseOrderFilters {
    supplier: Option<SupplierId>,
    #[param(inline)]
    status: Option<PurchaseOrderStatusDto>,
    #[param(inline)]
    receipt_status: Option<ReceiptStatusDto>,
    /// Orders going to this location.
    destination: Option<LocationId>,
    /// Orders with a line for this variant.
    variant: Option<VariantId>,
    #[param(inline)]
    sort: Option<PurchaseOrderSortDto>,
}

impl PurchaseOrderFilters {
    fn sort(&self) -> PurchaseOrderSort {
        match self.sort {
            None | Some(PurchaseOrderSortDto::NumberDesc) => PurchaseOrderSort::NumberDesc,
            Some(PurchaseOrderSortDto::NumberAsc) => PurchaseOrderSort::NumberAsc,
            Some(PurchaseOrderSortDto::ExpectedAt) => PurchaseOrderSort::ExpectedAt,
        }
    }
}

impl SortedFilters for PurchaseOrderFilters {
    fn fits(&self, key: &SortKey) -> bool {
        self.sort().fits(key)
    }
}

type PurchaseOrderPage = PageResponse<PurchaseOrderSummaryResponse>;

/// Purchase orders, newest first or in `sort`'s order, narrowed by the filters.
#[utoipa::path(
    get,
    path = "/admin/purchase-orders",
    params(PageParams, PurchaseOrderFilters),
    responses((status = 200, description = "OK", body = PurchaseOrderPage)),
    extensions(("x-hypha-scopes" = json!(["purchasing:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<PurchaseOrderFilters>,
) -> Result<Json<PurchaseOrderPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse_sorted()?;
    let sort = filters.sort();
    let filter = PurchaseOrderFilter {
        supplier: filters.supplier,
        status: filters.status.map(Into::into),
        receipt_status: filters.receipt_status.map(Into::into),
        destination: filters.destination,
        variant: filters.variant,
    };
    let page = list_purchase_orders(&ctx, &ports, filter, sort, page).await?;
    // Drafts keep no supplier name: the suppliers' current ones, read once each.
    let mut names: HashMap<SupplierId, String> = HashMap::new();
    for summary in &page.items {
        if summary.supplier_name.is_none() && !names.contains_key(&summary.supplier) {
            let name = supplier_name(&ports, summary.supplier).await?;
            names.insert(summary.supplier, name);
        }
    }
    let items = page
        .items
        .iter()
        .map(|s: &PurchaseOrderSummary| {
            Ok(PurchaseOrderSummaryResponse {
                id: s.id,
                number: s.number.value(),
                supplier: s.supplier,
                supplier_name: s
                    .supplier_name
                    .clone()
                    .or_else(|| names.get(&s.supplier).cloned())
                    .unwrap_or_default(),
                destination: s.destination,
                currency: s.currency,
                status: s.status.into(),
                receipt_status: s.receipt_status.into(),
                expected_at: times(s.expected_at)?,
                ordered_at: times(s.ordered_at)?,
                line_count: s.line_count,
                total: s.total,
                created_at: times(s.created_at)?,
            })
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(Json(PageResponse::sorted(
        &page,
        &scope,
        |s| Keyset {
            key: sort.key(s),
            id: s.id,
        },
        items,
    )?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePurchaseOrderRequest {
    supplier: SupplierId,
    /// Where the goods go: one of the store's locations. Required to order.
    #[serde(default)]
    destination: Option<LocationId>,
    /// ISO 4217; defaults to the supplier's.
    #[serde(default)]
    currency: Option<Currency>,
    /// One unit of the order's currency in the store's, as a decimal string with up to 8
    /// places. Required to order in another currency than the store's.
    #[serde(default)]
    exchange_rate: Option<ExchangeRate>,
    /// Absent copies the supplier's; `null` sets none.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<PaymentTermsDto>, required = false)]
    payment_terms: Option<Option<PaymentTermsDto>>,
    /// RFC 3339. Absent: ordering sets it from the supplier's lead time.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = DateTime)]
    expected_at: Option<Instant>,
    #[serde(default)]
    supplier_reference: Option<String>,
    /// At most 5,000 characters.
    #[serde(default)]
    note_to_supplier: Option<String>,
    /// At most 5,000 characters.
    #[serde(default)]
    note: Option<String>,
    /// One per variant, at most 1,000.
    #[serde(default)]
    lines: Vec<PurchaseOrderLineRequest>,
    /// At most 20.
    #[serde(default)]
    costs: Vec<PurchaseOrderCostRequest>,
    /// In the order's currency.
    #[serde(default)]
    tax: Option<Money>,
}

/// Writes a draft purchase order. Archived suppliers take none (`409 supplier_archived`).
#[utoipa::path(
    post,
    path = "/admin/purchase-orders",
    request_body = CreatePurchaseOrderRequest,
    responses((status = 201, description = "Created", body = PurchaseOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreatePurchaseOrderRequest>,
) -> Result<(StatusCode, Json<PurchaseOrderResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    check_location(&ports, req.destination).await?;
    let variants = variant_infos(&ports, &variants_of(&req.lines)).await?;
    let input = NewPurchaseOrder {
        supplier: req.supplier,
        destination: req.destination,
        currency: req.currency,
        exchange_rate: req.exchange_rate,
        payment_terms: req.payment_terms.map(|t| t.map(Into::into)),
        expected_at: req.expected_at.map(|i| i.0),
        supplier_reference: req.supplier_reference,
        note_to_supplier: req.note_to_supplier,
        note: req.note,
        lines: req.lines.into_iter().map(Into::into).collect(),
        costs: req.costs.into_iter().map(Into::into).collect(),
        tax: req.tax,
    };
    let po = match create_purchase_order(&ctx, &ports, &state.ids, &variants, input).await {
        // The supplier is in the body, not the URL.
        Err(hypha_purchasing::CommandError::SupplierNotFound(id)) => {
            return Err(ApiError::of(
                StatusCode::UNPROCESSABLE_ENTITY,
                &hypha_purchasing::CommandError::SupplierNotFound(id),
            ));
        }
        other => other?,
    };
    Ok((
        StatusCode::CREATED,
        Json(respond(&state, &ports, &po).await?),
    ))
}

/// A purchase order, with its totals and each line's landed cost.
#[utoipa::path(
    get,
    path = "/admin/purchase-orders/{id}",
    params(("id" = PurchaseOrderId, Path)),
    responses((status = 200, description = "OK", body = PurchaseOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PurchaseOrderId>,
) -> Result<Json<PurchaseOrderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let po = get_purchase_order(&ctx, &ports, id).await?;
    Ok(Json(respond(&state, &ports, &po).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePurchaseOrderRequest {
    /// `null` clears it (drafts only: an ordered order needs one).
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<LocationId>, required = false)]
    destination: Option<Option<LocationId>>,
    /// Drafts only (`409 purchase_order_currency_fixed`); the lines must be in it.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Currency, required = false)]
    currency: Option<Currency>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<ExchangeRate>, required = false)]
    exchange_rate: Option<Option<ExchangeRate>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<PaymentTermsDto>, required = false)]
    payment_terms: Option<Option<PaymentTermsDto>>,
    /// RFC 3339; `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, format = DateTime, required = false)]
    expected_at: Option<Option<Instant>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    supplier_reference: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    note_to_supplier: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    note: Option<Option<String>>,
    /// Replaces them all. On an ordered order, a line goods arrived on stays
    /// (`422 purchase_order_line_received`) with a quantity no lower than what arrived
    /// (`422 below_received`).
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<PurchaseOrderLineRequest>, required = false)]
    lines: Option<Vec<PurchaseOrderLineRequest>>,
    /// Replaces them all.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<PurchaseOrderCostRequest>, required = false)]
    costs: Option<Vec<PurchaseOrderCostRequest>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<Money>, required = false)]
    tax: Option<Option<Money>>,
}

/// Changes a draft or ordered purchase order (JSON Merge Patch), or a closed one's
/// extra costs (anything else on it is `409 purchase_order_not_open`). Each change to an
/// ordered or closed one counts a revision; one whose lines are all accounted for after
/// it closes. When goods arrived on it, a change to what its extra costs put on each unit
/// revalues that stock: each item's cost moves by its share of the difference over the
/// units on hand, with a `cost_adjustment` row in its history.
#[utoipa::path(
    patch,
    path = "/admin/purchase-orders/{id}",
    params(("id" = PurchaseOrderId, Path)),
    request_body = UpdatePurchaseOrderRequest,
    responses((status = 200, description = "OK", body = PurchaseOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PurchaseOrderId>,
    body: JsonBody<UpdatePurchaseOrderRequest>,
) -> Result<Json<PurchaseOrderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    check_location(&ports, req.destination.flatten()).await?;
    let variants = match &req.lines {
        Some(lines) => variant_infos(&ports, &variants_of(lines)).await?,
        None => Vec::new(),
    };
    let input = UpdatePurchaseOrder {
        destination: req.destination,
        currency: req.currency,
        exchange_rate: req.exchange_rate,
        payment_terms: req.payment_terms.map(|t| t.map(Into::into)),
        expected_at: req.expected_at.map(|e| e.map(|i| i.0)),
        supplier_reference: req.supplier_reference,
        note_to_supplier: req.note_to_supplier,
        note: req.note,
        lines: req.lines.map(|l| l.into_iter().map(Into::into).collect()),
        costs: req.costs.map(|c| c.into_iter().map(Into::into).collect()),
        tax: req.tax,
    };
    let store = store_currency(&state, &ports).await?;
    let po = update_purchase_order(&ctx, &ports, &state.clock, store, &variants, id, input).await?;
    Ok(Json(respond(&state, &ports, &po).await?))
}

/// Deletes a draft purchase order (`409 purchase_order_not_draft` otherwise: cancel it).
#[utoipa::path(
    delete,
    path = "/admin/purchase-orders/{id}",
    params(("id" = PurchaseOrderId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn remove<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PurchaseOrderId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_purchase_order(&ctx, &ports, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Places a draft with its supplier. It needs a destination, a line, the store's base
/// currency (`409 store_currency_required`) and, in another currency, an exchange rate
/// (`409 exchange_rate_required`). An unset expected date becomes now plus the
/// supplier's lead time.
#[utoipa::path(
    post,
    path = "/admin/purchase-orders/{id}/order",
    params(("id" = PurchaseOrderId, Path)),
    responses((status = 200, description = "OK", body = PurchaseOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn order<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PurchaseOrderId>,
) -> Result<Json<PurchaseOrderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let store = store_currency(&state, &ports).await?;
    let po = order_purchase_order(&ctx, &ports, &state.clock, store, id).await?;
    Ok(Json(respond(&state, &ports, &po).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SendPurchaseOrderRequest {
    /// Who gets it; the supplier's primary contact when absent
    /// (`422 supplier_email_required` when it has no email).
    #[serde(default)]
    #[schema(required = false)]
    to: Option<Vec<String>>,
    /// Copied in, each sent their own email. At most 10 addresses with `to`.
    #[serde(default)]
    #[schema(required = false)]
    cc: Vec<String>,
    /// A note for this send, shown above the order: at most 2,000 characters.
    #[serde(default)]
    #[schema(required = false)]
    message: Option<String>,
}

/// Emails the purchase order to its supplier: one email per address, queued and sent
/// like the store's other emails (the `supplier_purchase_order` template), with where
/// the goods go. A draft is ordered first (the same rules as `…/order`); an ordered one
/// is sent again as it is now, with its revision. Sets `sent_at` and records
/// `purchase_order.sent`. Needs `purchasing:send` as well: an agent may only propose it.
#[utoipa::path(
    post,
    path = "/admin/purchase-orders/{id}/send",
    params(("id" = PurchaseOrderId, Path)),
    request_body = SendPurchaseOrderRequest,
    responses((status = 200, description = "OK", body = PurchaseOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:write", "purchasing:send"]))),
)]
pub async fn send<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PurchaseOrderId>,
    body: JsonBody<SendPurchaseOrderRequest>,
) -> Result<Json<PurchaseOrderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let store = store_currency(&state, &ports).await?;
    // Where the goods go, as the supplier is told now.
    let current = get_purchase_order(&ctx, &ports, id).await?;
    let ship_to = match current.destination() {
        Some(location) => ports
            .locations()
            .get(location)
            .await
            .map_err(ApiError::internal)?
            .map(|l| ShipTo::of(l.name(), l.address())),
        None => None,
    };
    let input = SendPurchaseOrder {
        to: req.to,
        cc: req.cc,
        message: req.message,
    };
    let po = send_purchase_order(&ctx, &ports, &state.clock, store, id, ship_to, input).await?;
    Ok(Json(respond(&state, &ports, &po).await?))
}

/// Ends an ordered purchase order early: what's outstanding on each line is cancelled.
/// The extra costs then spread over fewer units, so the stock that arrived is revalued
/// for its bigger share.
#[utoipa::path(
    post,
    path = "/admin/purchase-orders/{id}/close",
    params(("id" = PurchaseOrderId, Path)),
    responses((status = 200, description = "OK", body = PurchaseOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn close<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PurchaseOrderId>,
) -> Result<Json<PurchaseOrderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let store = store_currency(&state, &ports).await?;
    let po = close_purchase_order(&ctx, &ports, &state.clock, store, id).await?;
    Ok(Json(respond(&state, &ports, &po).await?))
}

/// Calls off a draft or ordered purchase order nothing arrived on
/// (`409 purchase_order_received` otherwise: close it).
#[utoipa::path(
    post,
    path = "/admin/purchase-orders/{id}/cancel",
    params(("id" = PurchaseOrderId, Path)),
    responses((status = 200, description = "OK", body = PurchaseOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn cancel<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PurchaseOrderId>,
) -> Result<Json<PurchaseOrderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let po = cancel_purchase_order(&ctx, &ports, &state.clock, id).await?;
    Ok(Json(respond(&state, &ports, &po).await?))
}
