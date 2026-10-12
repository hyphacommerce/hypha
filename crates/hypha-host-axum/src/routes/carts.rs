use axum::{Json, extract::State, http::StatusCode};
use hypha_cart::{
    AddLine, LineStatus, PricedCart, add_line, create_cart, get_cart, set_cart_customer,
    set_cart_email,
};
use hypha_core::{
    CartId, CountryCode, Ctx, CustomerId, Email, ErrorCode, Money, ProductId, TaxClassId, TaxRate,
    VariantId,
};
use hypha_tax::TaxLine;
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{JsonBody, PathId},
    state::{SharedState, Storage},
};

#[derive(Deserialize)]
pub(super) struct CreateCartRequest {
    /// Where the cart ships to; picks the market and the tax rates.
    country: CountryCode,
}

#[derive(Deserialize)]
pub(super) struct AddLineRequest {
    variant: VariantId,
    quantity: u32,
}

#[derive(Deserialize)]
pub(super) struct SetEmailRequest {
    /// `null` clears it.
    email: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct SetCustomerRequest {
    /// `null` unlinks the customer (the cart keeps their email).
    customer: Option<CustomerId>,
}

#[derive(Serialize)]
pub(super) struct CartResponse {
    id: CartId,
    country: CountryCode,
    /// Who to contact about the order; required at checkout.
    email: Option<Email>,
    customer: Option<CustomerId>,
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
    rate: TaxRate,
    amount: Money,
    /// The class whose rate applied; `null` for the country's default rate.
    tax_class: Option<TaxClassId>,
}

impl From<&TaxLine> for TaxLineResponse {
    fn from(t: &TaxLine) -> Self {
        Self {
            rate: t.rate,
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
    #[serde(flatten)]
    state: LineState,
}

impl From<&PricedCart> for CartResponse {
    fn from(cart: &PricedCart) -> Self {
        Self {
            id: cart.id,
            country: cart.country,
            email: cart.email.clone(),
            customer: cart.customer,
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

pub(super) async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    JsonBody(req): JsonBody<CreateCartRequest>,
) -> Result<(StatusCode, Json<CartResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let cart = create_cart(&ctx, &ports, &state.ids, req.country).await?;
    // A new cart is empty, so pricing it can't fail; return the same shape as GET.
    let priced = get_cart(&ctx, &ports, cart.id()).await?;
    Ok((StatusCode::CREATED, Json(CartResponse::from(&priced))))
}

pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<CartId>,
) -> Result<Json<CartResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let priced = get_cart(&ctx, &ports, id).await?;
    Ok(Json(CartResponse::from(&priced)))
}

pub(super) async fn add<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<CartId>,
    JsonBody(req): JsonBody<AddLineRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    let input = AddLine {
        cart: id,
        variant: req.variant,
        quantity: req.quantity,
    };
    let priced = add_line(&ctx, &ports, input).await?;
    Ok(Json(CartResponse::from(&priced)))
}

pub(super) async fn set_email<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<CartId>,
    JsonBody(req): JsonBody<SetEmailRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    set_cart_email(&ctx, &ports, id, req.email).await?;
    let priced = get_cart(&ctx, &ports, id).await?;
    Ok(Json(CartResponse::from(&priced)))
}

pub(super) async fn set_customer<S: Storage>(
    State(state): State<SharedState<S>>,
    PathId(id): PathId<CartId>,
    JsonBody(req): JsonBody<SetCustomerRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let ports = state.ports().await?;
    set_cart_customer(&ctx, &ports, id, req.customer).await?;
    let priced = get_cart(&ctx, &ports, id).await?;
    Ok(Json(CartResponse::from(&priced)))
}
