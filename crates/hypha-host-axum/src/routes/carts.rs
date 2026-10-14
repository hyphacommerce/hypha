use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_cart::{
    AddLine, CreateCart, LineStatus, PricedCart, SetLineQuantity, add_line, create_cart, get_cart,
    remove_cart_line, set_cart_customer, set_cart_email, set_line_quantity,
};
use hypha_core::{
    CartId, CountryCode, Currency, CustomerId, Email, ErrorCode, MarketId, Money, OrderId,
    ProductId, TaxClassId, TaxRate, VariantId,
};
use hypha_tax::TaxLine;
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, KeyedJson, PathId, idempotent_response},
    state::{SharedState, Storage},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateCartRequest {
    /// Where the cart ships to; picks the market and the tax rates.
    country: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AddLineRequest {
    variant: VariantId,
    quantity: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetEmailRequest {
    /// `null` clears it.
    email: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetCustomerRequest {
    /// `null` unlinks the customer (the cart keeps their email).
    customer: Option<CustomerId>,
}

#[derive(Serialize)]
pub(super) struct CartResponse {
    id: CartId,
    /// Picked from the country when the cart was created; it sets the currency.
    market: MarketId,
    /// The currency of every amount in the cart.
    currency: Currency,
    country: CountryCode,
    /// Who to contact about the order; required at checkout.
    email: Option<Email>,
    customer: Option<CustomerId>,
    /// The order this cart was checked out as; `null` while it can still change.
    order: Option<OrderId>,
    /// Whether line prices include tax (set on the market).
    taxes_included: bool,
    /// Sum of the available lines, as priced. All totals count available lines only.
    subtotal: Money,
    /// Tax on the available lines: part of `subtotal` if `taxes_included`, else on top.
    tax_total: Money,
    /// What the buyer pays.
    total: Money,
    /// False when any line is unavailable; checkout should refuse such a cart.
    fully_available: bool,
    lines: Vec<LineResponse>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum LineState {
    Priced {
        unit_price: Money,
        total: Money,
        /// `null` when the cart's country has no tax rates.
        tax: Option<TaxLineResponse>,
    },
    Unavailable {
        /// Stable `ErrorCode`, e.g. "product_not_active".
        reason_code: &'static str,
        reason: String,
    },
}

#[derive(Serialize)]
pub(super) struct TaxLineResponse {
    /// Parts per million: 250000 = 25%.
    rate_ppm: TaxRate,
    amount: Money,
    /// The class whose rate applied; `null` for the country's default rate.
    tax_class: Option<TaxClassId>,
}

impl From<&TaxLine> for TaxLineResponse {
    fn from(t: &TaxLine) -> Self {
        Self {
            rate_ppm: t.rate,
            amount: t.amount,
            tax_class: t.class,
        }
    }
}

#[derive(Serialize)]
pub(super) struct LineResponse {
    product: ProductId,
    variant: VariantId,
    quantity: u32,
    /// The product's title now; `null` only when the product or variant is gone, so an
    /// unavailable line still says what it was.
    product_title: Option<String>,
    /// The variant's option values ("M / Blue"); `null` for a product without options.
    variant_title: Option<String>,
    sku: Option<String>,
    #[serde(flatten)]
    state: LineState,
}

impl From<&PricedCart> for CartResponse {
    fn from(cart: &PricedCart) -> Self {
        Self {
            id: cart.id,
            market: cart.market,
            currency: cart.currency,
            country: cart.country,
            email: cart.email.clone(),
            customer: cart.customer,
            order: cart.order,
            taxes_included: cart.taxes_included,
            subtotal: cart.subtotal,
            tax_total: cart.tax_total,
            total: cart.total,
            fully_available: cart.is_fully_available(),
            lines: cart
                .lines
                .iter()
                .map(|l| LineResponse {
                    product: l.product,
                    variant: l.variant,
                    quantity: l.quantity.value(),
                    product_title: l.details.as_ref().map(|d| d.product_title.clone()),
                    variant_title: l.details.as_ref().and_then(|d| d.variant_title.clone()),
                    sku: l.details.as_ref().and_then(|d| d.sku.clone()),
                    state: match &l.status {
                        LineStatus::Priced {
                            unit_price,
                            total,
                            tax,
                        } => LineState::Priced {
                            unit_price: unit_price.money(),
                            total: *total,
                            tax: tax.as_ref().map(TaxLineResponse::from),
                        },
                        LineStatus::Unavailable(reason) => LineState::Unavailable {
                            reason_code: reason.code(),
                            reason: reason.to_string(),
                        },
                    },
                })
                .collect(),
        }
    }
}

/// Takes an `Idempotency-Key` header: a retry returns the cart the first request made.
pub(super) async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    req: KeyedJson<CreateCartRequest>,
) -> Result<Response, ApiError> {
    let ports = state.ports().await?;
    let input = CreateCart {
        country: req.body.country,
        idempotency: req.idempotency,
    };
    let created = create_cart(&ctx, &ports, &state.ids, &state.clock, input).await?;
    Ok(idempotent_response(
        StatusCode::CREATED,
        CartResponse::from(&created.value),
        created.replayed,
    ))
}

pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<CartId>,
) -> Result<Json<CartResponse>, ApiError> {
    let ports = state.ports().await?;
    let priced = get_cart(&ctx, &ports, id).await?;
    Ok(Json(CartResponse::from(&priced)))
}

/// Takes an `Idempotency-Key` header: a retry returns the cart without adding again.
pub(super) async fn add<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<CartId>,
    req: KeyedJson<AddLineRequest>,
) -> Result<Response, ApiError> {
    let ports = state.ports().await?;
    let input = AddLine {
        cart: id,
        variant: req.body.variant,
        quantity: req.body.quantity,
        idempotency: req.idempotency,
    };
    let added = add_line(&ctx, &ports, &state.clock, input).await?;
    Ok(idempotent_response(
        StatusCode::OK,
        CartResponse::from(&added.value),
        added.replayed,
    ))
}

pub(super) async fn set_email<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<CartId>,
    JsonBody(req): JsonBody<SetEmailRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let ports = state.ports().await?;
    let priced = set_cart_email(&ctx, &ports, id, req.email).await?;
    Ok(Json(CartResponse::from(&priced)))
}

pub(super) async fn set_customer<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<CartId>,
    JsonBody(req): JsonBody<SetCustomerRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let ports = state.ports().await?;
    let priced = set_cart_customer(&ctx, &ports, id, req.customer).await?;
    Ok(Json(CartResponse::from(&priced)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetQuantityRequest {
    /// At least 1; `DELETE` the line to take it out.
    quantity: u32,
}

pub(super) async fn set_quantity<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId((id, variant)): PathId<(CartId, VariantId)>,
    JsonBody(req): JsonBody<SetQuantityRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let ports = state.ports().await?;
    let input = SetLineQuantity {
        cart: id,
        variant,
        quantity: req.quantity,
    };
    let priced = set_line_quantity(&ctx, &ports, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("line_not_found"))?;
    Ok(Json(CartResponse::from(&priced)))
}

pub(super) async fn remove_line<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId((id, variant)): PathId<(CartId, VariantId)>,
) -> Result<Json<CartResponse>, ApiError> {
    let ports = state.ports().await?;
    let priced = remove_cart_line(&ctx, &ports, id, variant)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("line_not_found"))?;
    Ok(Json(CartResponse::from(&priced)))
}
