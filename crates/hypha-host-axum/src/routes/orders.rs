use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_checkout::{Checkout, checkout};
use hypha_core::{
    CartId, CountryCode, Currency, CustomerId, Email, MarketId, Money, OrderId, OrderLineId,
    ProductId, TaxClassId, TaxRate, VariantId,
};
use hypha_orders::{
    FinancialStatus, FulfillmentStatus, HasOrders, Order, OrderLine, get_order, list_orders,
    mark_order_paid,
};
use serde::{Deserialize, Serialize};

use super::{next_cursor, page_request, rfc3339};
use crate::{
    error::ApiError,
    extract::{Caller, KeyedJson, PathId, QueryParams, Storefront, idempotent_response},
    state::{SharedState, Storage},
};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CheckoutRequest {
    /// The total the buyer saw; if the cart now costs something else, checkout fails
    /// with `total_changed` (409) instead of placing a different order.
    #[serde(default)]
    expected_total: Option<Money>,
}

/// `FinancialStatus` on the wire. A DTO of its own, so the domain enum carries no
/// serde and renaming a variant can't silently change the API.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum FinancialStatusDto {
    Pending,
    Paid,
}

impl From<FinancialStatus> for FinancialStatusDto {
    fn from(s: FinancialStatus) -> Self {
        match s {
            FinancialStatus::Pending => FinancialStatusDto::Pending,
            FinancialStatus::Paid => FinancialStatusDto::Paid,
        }
    }
}

/// `FulfillmentStatus` on the wire.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum FulfillmentStatusDto {
    Unfulfilled,
}

impl From<FulfillmentStatus> for FulfillmentStatusDto {
    fn from(s: FulfillmentStatus) -> Self {
        match s {
            FulfillmentStatus::Unfulfilled => FulfillmentStatusDto::Unfulfilled,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct OrderResponse {
    id: OrderId,
    /// The human number, shown as "#1001".
    number: i64,
    /// RFC 3339, UTC.
    placed_at: String,
    cart: CartId,
    market: MarketId,
    currency: Currency,
    country: CountryCode,
    customer: CustomerId,
    email: Email,
    financial_status: FinancialStatusDto,
    fulfillment_status: FulfillmentStatusDto,
    /// Whether line prices include tax (the market decided, at checkout).
    taxes_included: bool,
    subtotal: Money,
    /// Part of `subtotal` if `taxes_included`, else on top.
    tax_total: Money,
    total: Money,
    lines: Vec<OrderLineResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct OrderLineResponse {
    id: OrderLineId,
    product: ProductId,
    variant: VariantId,
    quantity: u32,
    product_title: String,
    /// The variant's option values ("M / Blue"); `null` for a product without options.
    variant_title: Option<String>,
    sku: Option<String>,
    unit_price: Money,
    total: Money,
    /// `null` when the country had no tax rates.
    tax: Option<OrderTaxResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct OrderTaxResponse {
    /// Parts per million: 250000 = 25%.
    rate_ppm: TaxRate,
    amount: Money,
    /// The class whose rate applied; `null` for the country's default rate.
    tax_class: Option<TaxClassId>,
}

impl OrderResponse {
    fn new(order: &Order) -> Result<Self, ApiError> {
        Ok(Self {
            id: order.id(),
            number: order.number().value(),
            placed_at: rfc3339(order.placed_at())?,
            cart: order.cart(),
            market: order.market(),
            currency: order.currency(),
            country: order.country(),
            customer: order.customer(),
            email: order.email().clone(),
            financial_status: order.financial_status().into(),
            fulfillment_status: order.fulfillment_status().into(),
            taxes_included: order.taxes_included(),
            subtotal: order.subtotal(),
            tax_total: order.tax_total(),
            total: order.total(),
            lines: order.lines().iter().map(OrderLineResponse::from).collect(),
        })
    }
}

impl From<&OrderLine> for OrderLineResponse {
    fn from(l: &OrderLine) -> Self {
        Self {
            id: l.id,
            product: l.product,
            variant: l.variant,
            quantity: l.quantity.value(),
            product_title: l.product_title.clone(),
            variant_title: l.variant_title.clone(),
            sku: l.sku.clone(),
            unit_price: l.unit_price.money(),
            total: l.total,
            tax: l.tax.map(|t| OrderTaxResponse {
                rate_ppm: t.rate,
                amount: t.amount,
                tax_class: t.class,
            }),
        }
    }
}

/// Places the order for the cart. Takes an `Idempotency-Key` header: a retry returns
/// the order the first request placed. The body is `{}` or `{"expected_total": …}`.
#[utoipa::path(
    post,
    path = "/storefront/carts/{id}/checkout",
    params(("id" = CartId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Makes the request safe to retry: a retry with the same key and body replays the first answer (marked `Idempotent-Replayed: true`) instead of repeating the effect. Same key with another body: `422 idempotency_key_reused`.")),
    request_body = CheckoutRequest,
    responses((status = 201, description = "Created", body = OrderResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`.")))),
    extensions(("x-hypha-scopes" = json!(["checkout"]))),
)]
pub(super) async fn checkout_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(cart): PathId<CartId>,
    req: KeyedJson<CheckoutRequest>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = req.parse()?;
    let input = Checkout {
        cart,
        expected_total: req.body.expected_total,
        idempotency: req.idempotency,
    };
    let placed = checkout(&ctx, &ports, &state.ids, &state.clock, input).await?;
    Ok(idempotent_response(
        StatusCode::CREATED,
        OrderResponse::new(&placed.value)?,
        placed.replayed,
    ))
}

/// An order in a list: what a table of orders shows, without lines.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct OrderSummaryResponse {
    id: OrderId,
    /// The human number, shown as "#1001".
    number: i64,
    /// RFC 3339, UTC.
    placed_at: String,
    customer: CustomerId,
    email: Email,
    country: CountryCode,
    financial_status: FinancialStatusDto,
    fulfillment_status: FulfillmentStatusDto,
    total: Money,
    /// How many items, adding up line quantities.
    item_count: u64,
}

impl OrderSummaryResponse {
    fn new(order: &Order) -> Result<Self, ApiError> {
        Ok(Self {
            id: order.id(),
            number: order.number().value(),
            placed_at: rfc3339(order.placed_at())?,
            customer: order.customer(),
            email: order.email().clone(),
            country: order.country(),
            financial_status: order.financial_status().into(),
            fulfillment_status: order.fulfillment_status().into(),
            total: order.total(),
            item_count: order
                .lines()
                .iter()
                .map(|l| u64::from(l.quantity.value()))
                .sum(),
        })
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(super) struct ListOrdersQuery {
    /// The previous page's `next_cursor`; absent for the first page.
    cursor: Option<String>,
    /// Orders per page, 1 to 100 (default 50).
    #[param(minimum = 1, maximum = 100)]
    limit: Option<u32>,
}

/// A page of orders, newest first (design note 0012).
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct OrderPage {
    items: Vec<OrderSummaryResponse>,
    /// Whether there are more after these.
    has_more: bool,
    /// Pass as `cursor` for the next page; `null` on the last page. Opaque: don't parse it.
    next_cursor: Option<String>,
}

/// Orders, newest first.
#[utoipa::path(
    get,
    path = "/admin/orders",
    params(ListOrdersQuery),
    responses((status = 200, description = "OK", body = OrderPage)),
    extensions(("x-hypha-scopes" = json!(["orders:read"]))),
)]
pub(super) async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: QueryParams<ListOrdersQuery>,
) -> Result<Json<OrderPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let query = query.parse()?;
    let page = page_request(query.cursor.as_deref(), query.limit)?;
    let page = list_orders(&ctx, ports.orders(), page).await?;
    Ok(Json(OrderPage {
        items: page
            .items
            .iter()
            .map(OrderSummaryResponse::new)
            .collect::<Result<_, _>>()?,
        has_more: page.has_more,
        next_cursor: next_cursor(&page, Order::id),
    }))
}

/// An order.
#[utoipa::path(
    get,
    path = "/admin/orders/{id}",
    params(("id" = OrderId, Path)),
    responses((status = 200, description = "OK", body = OrderResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:read"]))),
)]
pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<OrderId>,
) -> Result<Json<OrderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let order = get_order(&ctx, ports.orders(), id).await?;
    Ok(Json(OrderResponse::new(&order)?))
}

/// Records that the order is paid (e.g. a bank transfer arrived). Marking a paid order
/// again changes nothing.
#[utoipa::path(
    post,
    path = "/admin/orders/{id}/paid",
    params(("id" = OrderId, Path)),
    responses((status = 200, description = "OK", body = OrderResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:write"]))),
)]
pub(super) async fn mark_paid<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<OrderId>,
) -> Result<Json<OrderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let order = mark_order_paid(&ctx, ports.orders(), id).await?;
    Ok(Json(OrderResponse::new(&order)?))
}
