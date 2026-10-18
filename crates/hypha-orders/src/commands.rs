use hypha_core::{AuthError, Ctx, ErrorCode, Keyset, OrderId, Page, PageRequest, RepoError, Scope};
use thiserror::Error;

use crate::{Order, OrderError, OrderFilter, OrderFilterInput, OrderRepo, OrderSort, OrderSummary};

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

/// A page of order summaries, newest first (design note 0012).
pub async fn list_orders<R: OrderRepo>(
    ctx: &Ctx,
    orders: &R,
    input: ListOrders,
    page: PageRequest<Keyset<OrderId>>,
) -> Result<Page<OrderSummary>, CommandError> {
    ctx.require(Scope::OrdersRead)?;
    let filter = OrderFilter::new(input.filter)?;
    input.sort.check(&filter)?;
    Ok(orders.list(&filter, input.sort, page).await?)
}

/// An order list's filters and order as a request gives them; `list_orders` checks them.
#[derive(Debug, Default)]
pub struct ListOrders {
    pub filter: OrderFilterInput,
    pub sort: OrderSort,
}

/// The order a storefront's buyer placed, for its confirmation (design note 0024, §7):
/// needs `checkout`, the scope that placed it, and, for a key of one channel, the order
/// must be that channel's (another channel's is `OrderNotFound`, as if absent). Hosts
/// check the order's access token before calling this: knowing an order's ID is not
/// enough to read it.
pub async fn get_placed_order<R: OrderRepo>(
    ctx: &Ctx,
    orders: &R,
    id: OrderId,
) -> Result<Order, CommandError> {
    ctx.require(Scope::Checkout)?;
    let order = load(orders, id).await?;
    match ctx.channel() {
        Some(channel) if channel != order.channel() => Err(CommandError::OrderNotFound(id)),
        _ => Ok(order),
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

async fn load<R: OrderRepo>(orders: &R, id: OrderId) -> Result<Order, CommandError> {
    orders.get(id).await?.ok_or(CommandError::OrderNotFound(id))
}
