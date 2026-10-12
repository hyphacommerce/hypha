use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use hypha_cart::{AddLine, LineStatus, PricedCart, add_line, create_cart, get_cart};
use hypha_core::{CartId, Ctx, ErrorCode, MarketId, Money, ProductId, VariantId};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{JsonBody, PathId},
    state::SharedState,
};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/carts", post(create))
        .route("/carts/{id}", get(fetch))
        .route("/carts/{id}/lines", post(add))
}

#[derive(Deserialize)]
struct CreateCartRequest {
    market: MarketId,
}

#[derive(Deserialize)]
struct AddLineRequest {
    variant: VariantId,
    quantity: u32,
}

#[derive(Serialize)]
struct CartResponse {
    id: CartId,
    /// Sum of the available lines only.
    total: Money,
    /// False when any line is unavailable; checkout should refuse such a cart.
    fully_available: bool,
    lines: Vec<LineResponse>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum LineState {
    Priced {
        unit_price: Money,
        total: Money,
    },
    Unavailable {
        /// Stable `ErrorCode`, e.g. "product_not_active".
        reason_code: &'static str,
        reason: String,
    },
}

#[derive(Serialize)]
struct LineResponse {
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
                        LineStatus::Priced { unit_price, total } => LineState::Priced {
                            unit_price: unit_price.money(),
                            total: *total,
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

async fn create(
    State(state): State<SharedState>,
    JsonBody(req): JsonBody<CreateCartRequest>,
) -> Result<(StatusCode, Json<CartResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let cart = create_cart(&ctx, state.as_ref(), &state.ids, req.market).await?;
    // A new cart is empty, so pricing it can't fail; return the same shape as GET.
    let priced = get_cart(&ctx, state.as_ref(), cart.id()).await?;
    Ok((StatusCode::CREATED, Json(CartResponse::from(&priced))))
}

async fn fetch(
    State(state): State<SharedState>,
    PathId(id): PathId<CartId>,
) -> Result<Json<CartResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let priced = get_cart(&ctx, state.as_ref(), id).await?;
    Ok(Json(CartResponse::from(&priced)))
}

async fn add(
    State(state): State<SharedState>,
    PathId(id): PathId<CartId>,
    JsonBody(req): JsonBody<AddLineRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let input = AddLine {
        cart: id,
        variant: req.variant,
        quantity: req.quantity,
    };
    let priced = add_line(&ctx, state.as_ref(), input).await?;
    Ok(Json(CartResponse::from(&priced)))
}
