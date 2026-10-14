use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_checkout::{Checkout, checkout};
use hypha_core::{
    CartId, CountryCode, Currency, CustomerId, Email, MarketId, Money, OrderId, OrderLineId,
    ProductId, TaxClassId, TaxRate, VariantId,
};
use hypha_orders::{
    FinancialStatus, FulfillmentStatus, HasOrders, Order, OrderLine, get_order, mark_order_paid,
};
use serde::{Deserialize, Serialize};

use super::rfc3339;
use crate::{
    error::ApiError,
    extract::{Caller, KeyedJson, PathId, idempotent_response},
    state::{SharedState, Storage},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CheckoutRequest {
    /// The total the buyer saw; if the cart now costs something else, checkout fails
    /// with `total_changed` (409) instead of placing a different order.
    #[serde(default)]
    expected_total: Option<Money>,
}

/// `FinancialStatus` on the wire. A DTO of its own, so the domain enum carries no
/// serde and renaming a variant can't silently change the API.
#[derive(Serialize)]
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
#[derive(Serialize)]
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

#[derive(Serialize)]
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

#[derive(Serialize)]
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

#[derive(Serialize)]
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
pub(super) async fn checkout_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(cart): PathId<CartId>,
    req: KeyedJson<CheckoutRequest>,
) -> Result<Response, ApiError> {
    let ports = state.ports().await?;
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

pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<OrderId>,
) -> Result<Json<OrderResponse>, ApiError> {
    let ports = state.ports().await?;
    let order = get_order(&ctx, ports.orders(), id).await?;
    Ok(Json(OrderResponse::new(&order)?))
}

/// Records that the order is paid (e.g. a bank transfer arrived). Marking a paid order
/// again changes nothing.
pub(super) async fn mark_paid<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<OrderId>,
) -> Result<Json<OrderResponse>, ApiError> {
    let ports = state.ports().await?;
    let order = mark_order_paid(&ctx, ports.orders(), id).await?;
    Ok(Json(OrderResponse::new(&order)?))
}
