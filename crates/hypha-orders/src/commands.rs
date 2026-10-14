use hypha_core::{AuthError, Ctx, ErrorCode, OrderId, Page, PageRequest, RepoError, Scope};
use thiserror::Error;

use crate::{FinancialStatus, Order, OrderRepo};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error("order {0} not found")]
    OrderNotFound(OrderId),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::OrderNotFound(_) => "order_not_found",
        }
    }
}

/// A page of orders, newest first (design note 0012).
pub async fn list_orders<R: OrderRepo>(
    ctx: &Ctx,
    orders: &R,
    page: PageRequest<OrderId>,
) -> Result<Page<Order>, CommandError> {
    ctx.require(Scope::OrdersRead)?;
    Ok(orders.list(page).await?)
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
/// do this through the same command). Marking a paid order again changes nothing, even
/// when two requests mark it at once.
pub async fn mark_order_paid<R: OrderRepo>(
    ctx: &Ctx,
    orders: &R,
    id: OrderId,
) -> Result<Order, CommandError> {
    ctx.require(Scope::OrdersWrite)?;
    let mut order = load(orders, id).await?;
    if !order.mark_paid() {
        return Ok(order);
    }
    match orders.update(&mut order).await {
        Ok(()) => Ok(order),
        // Changed since it was loaded. If the change was the same payment (a double
        // click, a webhook retry), there's nothing left to do.
        Err(RepoError::StaleVersion) => {
            let now = load(orders, id).await?;
            match now.financial_status() {
                FinancialStatus::Paid => Ok(now),
                FinancialStatus::Pending => Err(RepoError::StaleVersion.into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}

async fn load<R: OrderRepo>(orders: &R, id: OrderId) -> Result<Order, CommandError> {
    orders.get(id).await?.ok_or(CommandError::OrderNotFound(id))
}
