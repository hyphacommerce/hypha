use hypha_core::{AuthError, Ctx, ErrorCode, OrderId, RepoError, Scope};
use thiserror::Error;

use crate::{Order, OrderError, OrderRepo};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Order(#[from] OrderError),
    #[error("order {0} not found")]
    OrderNotFound(OrderId),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Order(e) => e.code(),
            CommandError::OrderNotFound(_) => "order_not_found",
        }
    }
}

pub async fn get_order<R: OrderRepo>(
    ctx: &Ctx,
    orders: &R,
    id: OrderId,
) -> Result<Order, CommandError> {
    ctx.require(Scope::OrdersRead)?;
    load(orders, id).await
}

/// Records that the order is paid, e.g. a bank transfer arrived (payment providers will
/// do this through the same command). Marking a paid order again changes nothing.
pub async fn mark_order_paid<R: OrderRepo>(
    ctx: &Ctx,
    orders: &R,
    id: OrderId,
) -> Result<Order, CommandError> {
    ctx.require(Scope::OrdersWrite)?;
    let mut order = load(orders, id).await?;
    if order.mark_paid() {
        orders.update(&mut order).await?;
    }
    Ok(order)
}

async fn load<R: OrderRepo>(orders: &R, id: OrderId) -> Result<Order, CommandError> {
    orders.get(id).await?.ok_or(CommandError::OrderNotFound(id))
}
