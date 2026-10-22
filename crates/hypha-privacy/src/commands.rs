use hypha_auth::{HasIdentities, IdentityRepo, Principal, StageIdentities, StageSessions};
use hypha_cart::{CartRepo, HasCarts, StageCarts};
use hypha_core::{AuthError, Clock, Ctx, CustomerId, ErrorCode, HasBatch, RepoError, Scope};
use hypha_customers::{Customer, CustomerRepo, HasCustomers, StageCustomers};
use hypha_orders::{HasOrders, OrderRepo};
use thiserror::Error;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Cart(#[from] hypha_cart::CartError),
    /// An erasure request's rules (requested already, none to cancel, erased).
    #[error(transparent)]
    Customer(#[from] hypha_customers::CustomerError),
    #[error("customer {0} not found")]
    CustomerNotFound(CustomerId),
    /// Placed orders are never deleted, so neither is their customer: erase instead.
    #[error("customer {customer} has {orders} order(s): erase it instead of deleting it")]
    CustomerHasOrders { customer: CustomerId, orders: u64 },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Cart(e) => e.code(),
            CommandError::Customer(e) => e.code(),
            CommandError::CustomerNotFound(_) => "customer_not_found",
            CommandError::CustomerHasOrders { .. } => "customer_has_orders",
        }
    }
}

/// Everything the privacy workflows read and write, named once so implementors (a
/// host's ports, the test world) never list it.
pub trait PrivacyPorts: HasCustomers + HasOrders + HasCarts + HasIdentities + HasBatch
where
    Self::Batch: StageCustomers + StageCarts + StageIdentities + StageSessions,
{
}

impl<S> PrivacyPorts for S
where
    S: HasCustomers + HasOrders + HasCarts + HasIdentities + HasBatch,
    S::Batch: StageCustomers + StageCarts + StageIdentities + StageSessions,
{
}

async fn load<S: HasCustomers>(ports: &S, id: CustomerId) -> Result<Customer, CommandError> {
    ports
        .customers()
        .get(id)
        .await?
        .ok_or(CommandError::CustomerNotFound(id))
}

/// Removes a customer that has no orders: the row, its address book and memberships go; its
/// carts are unlinked (`customer` cleared, the email kept: a cart without an order is the
/// shopper's own data); its custom field values follow `customer.deleted`
/// (`CustomFieldCleanup`). One with orders is refused (`customer_has_orders`): erase it
/// instead.
pub async fn delete_customer<S: PrivacyPorts>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: CustomerId,
) -> Result<(), CommandError>
where
    S::Batch: StageCustomers + StageCarts + StageIdentities + StageSessions,
{
    ctx.require(Scope::CustomersWrite)?;
    let mut customer = load(ports, id).await?;
    let stats = ports.orders().stats_for(id).await?;
    let orders = stats.orders_count + stats.cancelled_count;
    if orders > 0 {
        return Err(CommandError::CustomerHasOrders {
            customer: id,
            orders,
        });
    }
    // A completed cart has an order, so every cart found here is open.
    let mut carts = ports.carts().of_customer(id).await?;
    for cart in &mut carts {
        cart.set_customer(None)?;
    }
    customer.delete();
    let account = ports
        .identities()
        .get_by_principal(Principal::Customer(id))
        .await?;
    let mut batch = ports.batch();
    for cart in &mut carts {
        batch.update_cart(cart);
    }
    // Their account goes with them.
    if let Some(identity) = &account {
        batch.revoke_sessions(identity.id(), None, clock.now());
        batch.delete_identity(identity);
    }
    batch.delete_customer(&mut customer);
    ports.commit(batch).await?;
    Ok(())
}
