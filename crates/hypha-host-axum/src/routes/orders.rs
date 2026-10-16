use hypha_core::Stamped;

use super::stamp_times;
use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_checkout::{Checkout, checkout};
use hypha_core::{
    CartId, CountryCode, Currency, CustomerId, Email, LocationId, MarketId, Money, OrderId,
    OrderLineId, ProductId, ShippingGroupId, ShippingOptionId, TaxClassId, TaxRate, VariantId,
};
use hypha_order_actions::{BuyerEmail, CancelOrder, OrderView, cancel_order, order_view};
use hypha_orders::{
    CancelReason, FinancialStatus, FulfillmentStatus, HasOrders, Order, OrderLine, OrderSummary,
    list_orders, mark_order_paid,
};
use serde::{Deserialize, Serialize};

use super::{
    address::PostalAddressResponse,
    page::{ListQuery, PageParams, PageResponse},
    rfc3339,
    shipping::DeliveryEstimateDto,
};
use crate::{
    error::{ApiError, ApiErrorBody},
    extract::{Caller, JsonBody, KeyedJson, PathId, Storefront, idempotent_response},
    rate_limit::{self, Rule},
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
    PartiallyFulfilled,
    Fulfilled,
}

impl From<FulfillmentStatus> for FulfillmentStatusDto {
    fn from(s: FulfillmentStatus) -> Self {
        match s {
            FulfillmentStatus::Unfulfilled => FulfillmentStatusDto::Unfulfilled,
            FulfillmentStatus::PartiallyFulfilled => FulfillmentStatusDto::PartiallyFulfilled,
            FulfillmentStatus::Fulfilled => FulfillmentStatusDto::Fulfilled,
        }
    }
}

/// `CancelReason` on the wire (design note 0018).
#[derive(Serialize, Deserialize, Clone, Copy, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum CancelReasonDto {
    /// The buyer asked.
    Customer,
    /// The goods ran out or can't be shipped.
    Inventory,
    Fraud,
    /// Payment was declined.
    Declined,
    Staff,
    Other,
}

impl From<CancelReason> for CancelReasonDto {
    fn from(r: CancelReason) -> Self {
        match r {
            CancelReason::Customer => CancelReasonDto::Customer,
            CancelReason::Inventory => CancelReasonDto::Inventory,
            CancelReason::Fraud => CancelReasonDto::Fraud,
            CancelReason::Declined => CancelReasonDto::Declined,
            CancelReason::Staff => CancelReasonDto::Staff,
            CancelReason::Other => CancelReasonDto::Other,
        }
    }
}

impl From<CancelReasonDto> for CancelReason {
    fn from(r: CancelReasonDto) -> Self {
        match r {
            CancelReasonDto::Customer => CancelReason::Customer,
            CancelReasonDto::Inventory => CancelReason::Inventory,
            CancelReasonDto::Fraud => CancelReason::Fraud,
            CancelReasonDto::Declined => CancelReason::Declined,
            CancelReasonDto::Staff => CancelReason::Staff,
            CancelReasonDto::Other => CancelReason::Other,
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
    /// When it was cancelled, RFC 3339 in UTC; `null` if it wasn't.
    cancelled_at: Option<String>,
    /// Why it was cancelled; `null` if it wasn't.
    cancel_reason: Option<CancelReasonDto>,
    /// Whether line prices include tax (the market decided, at checkout).
    taxes_included: bool,
    subtotal: Money,
    /// What shipping was charged, as part of `total`.
    shipping_total: Money,
    /// Tax on lines and shipping: part of `subtotal` and `shipping_total` if
    /// `taxes_included`, else on top.
    tax_total: Money,
    total: Money,
    lines: Vec<OrderLineResponse>,
    /// `null` when nothing in the order ships.
    shipping_address: Option<PostalAddressResponse>,
    /// How each group of lines ships, as charged.
    shipping_lines: Vec<OrderShippingLineResponse>,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct OrderShippingLineResponse {
    group: ShippingGroupId,
    /// The order lines this ships.
    lines: Vec<OrderLineId>,
    option: ShippingOptionId,
    code: String,
    name: String,
    /// `app_…` or `spore_…`; `null` for a built-in rate.
    provider: Option<String>,
    service: Option<String>,
    /// The provider's quote ID.
    reference: Option<String>,
    amount: Money,
    /// As estimated at checkout.
    estimate: Option<DeliveryEstimateDto>,
    /// `null` when the country had no tax rates.
    tax: Option<OrderTaxResponse>,
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
    /// How many of `quantity` standing fulfilments shipped.
    fulfilled_quantity: u32,
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
        let (created_at, updated_at) = stamp_times(order.stamps())?;
        Ok(Self {
            created_at,
            updated_at,
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
            cancelled_at: order.cancelled().map(|c| rfc3339(c.at)).transpose()?,
            cancel_reason: order.cancelled().map(|c| c.reason.into()),
            taxes_included: order.taxes_included(),
            subtotal: order.subtotal(),
            shipping_total: order.shipping_total(),
            tax_total: order.tax_total(),
            total: order.total(),
            lines: order.lines().iter().map(OrderLineResponse::from).collect(),
            shipping_address: order.shipping_address().map(PostalAddressResponse::from),
            shipping_lines: order
                .shipping_lines()
                .iter()
                .map(|s| OrderShippingLineResponse {
                    group: s.group,
                    lines: s.lines.clone(),
                    option: s.option,
                    code: s.code.clone(),
                    name: s.name.clone(),
                    provider: s.provider.clone(),
                    service: s.service.clone(),
                    reference: s.reference.clone(),
                    amount: s.amount.money(),
                    estimate: s
                        .estimate_days
                        .map(|(min_days, max_days)| DeliveryEstimateDto { min_days, max_days }),
                    tax: s.tax.map(|t| OrderTaxResponse {
                        rate_ppm: t.rate,
                        amount: t.amount,
                        tax_class: t.class,
                    }),
                })
                .collect(),
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
            fulfilled_quantity: l.fulfilled_quantity,
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
    responses((status = 201, description = "Created", body = OrderResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`."))), (status = 429, description = "Over the rate limit for this cart (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!(["checkout"]))),
)]
pub(super) async fn checkout_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(cart): PathId<CartId>,
    req: KeyedJson<CheckoutRequest>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    // Renewing an expired quote calls rate apps, like asking for options.
    rate_limit::enforce(&state.limiter, Rule::ShippingQuotesCart, &cart.to_string()).await?;
    let req = req.parse()?;
    let input = Checkout {
        cart,
        expected_total: req.body.expected_total,
        idempotency: req.idempotency,
    };
    let rates = state.rates(&ports);
    let placed = checkout(&ctx, &ports, &state.ids, &state.clock, &rates, input).await?;
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
    /// When it was cancelled, RFC 3339 in UTC; `null` if it wasn't.
    cancelled_at: Option<String>,
    total: Money,
    /// How many items, adding up line quantities.
    item_count: u64,
}

impl OrderSummaryResponse {
    fn new(o: OrderSummary) -> Result<Self, ApiError> {
        Ok(Self {
            id: o.id,
            number: o.number.value(),
            placed_at: rfc3339(o.placed_at)?,
            customer: o.customer,
            email: o.email,
            country: o.country,
            financial_status: o.financial_status.into(),
            fulfillment_status: o.fulfillment_status.into(),
            cancelled_at: o.cancelled_at.map(rfc3339).transpose()?,
            total: o.total,
            item_count: o.item_count,
        })
    }
}

/// A page of orders (named for the documents: utoipa's `body =` takes a plain name).
type OrderPage = PageResponse<OrderSummaryResponse>;

/// Orders, newest first.
#[utoipa::path(
    get,
    path = "/admin/orders",
    params(PageParams),
    responses((status = 200, description = "OK", body = OrderPage)),
    extensions(("x-hypha-scopes" = json!(["orders:read"]))),
)]
pub(super) async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery,
) -> Result<Json<OrderPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, _, scope) = query.parse()?;
    let page = list_orders(&ctx, ports.orders(), page).await?;
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |o| o.id,
        OrderSummaryResponse::new,
    )?))
}

/// Units of a line still to ship, and where they wait (design note 0018, FU4).
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct UnfulfilledDto {
    line: OrderLineId,
    /// The location holding them for the line; `null` for units held nowhere (an
    /// untracked item): any location.
    location: Option<LocationId>,
    quantity: u32,
}

/// An order as the admin sees it: with what's left to ship, by location.
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct AdminOrderResponse {
    #[serde(flatten)]
    order: OrderResponse,
    /// Per line with anything left, in line order; empty once everything shipped or the
    /// order was cancelled.
    unfulfilled: Vec<UnfulfilledDto>,
}

impl AdminOrderResponse {
    fn new(view: &OrderView) -> Result<Self, ApiError> {
        Ok(Self {
            order: OrderResponse::new(&view.order)?,
            unfulfilled: view
                .unfulfilled
                .iter()
                .map(|u| UnfulfilledDto {
                    line: u.line,
                    location: u.location,
                    quantity: u.quantity,
                })
                .collect(),
        })
    }
}

/// An order, with what's left to ship.
#[utoipa::path(
    get,
    path = "/admin/orders/{id}",
    params(("id" = OrderId, Path)),
    responses((status = 200, description = "OK", body = AdminOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:read"]))),
)]
pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<OrderId>,
) -> Result<Json<AdminOrderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let view = order_view(&ctx, &ports, id).await?;
    Ok(Json(AdminOrderResponse::new(&view)?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CancelOrderRequest {
    reason: CancelReasonDto,
    /// Email the buyer that the order is cancelled (`false` if absent).
    #[serde(default)]
    notify: bool,
}

/// Cancels the order: every line's reserved stock is released. Refused while
/// fulfilments stand (`order_has_fulfillments`, 409, listing them): cancel those first.
/// A paid order stays paid; refund it outside Hypha for now. Cancelling it again
/// changes nothing.
#[utoipa::path(
    post,
    path = "/admin/orders/{id}/cancel",
    params(("id" = OrderId, Path)),
    request_body = CancelOrderRequest,
    responses((status = 200, description = "OK", body = AdminOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:write"]))),
)]
pub(super) async fn cancel<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<OrderId>,
    body: JsonBody<CancelOrderRequest>,
) -> Result<Json<AdminOrderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let before = hypha_orders::get_order(&ctx, ports.orders(), id).await?;
    let input = CancelOrder {
        order: id,
        reason: req.reason.into(),
    };
    let order = cancel_order(&ctx, &ports, &state.clock, input).await?;
    // Only the request that cancelled it tells the buyer.
    if req.notify
        && before.cancelled().is_none()
        && let Some(email) = BuyerEmail::cancelled(&order)
    {
        super::fulfillments::notify_buyer(&state, email);
    }
    // A cancelled order has nothing left to ship.
    Ok(Json(AdminOrderResponse::new(&OrderView {
        order,
        unfulfilled: Vec::new(),
    })?))
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
