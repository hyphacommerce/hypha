use hypha_cart::{
    CartPorts, CartRepo, CartStatus, LineStatus, PricedLine, StageCarts, price_for_checkout,
};
use hypha_core::{
    AuthError, CartId, Clock, Ctx, CustomerId, ErrorCode, HasBatch, IdGenerator, IdempotencyError,
    IdempotencyRecord, IdempotencyRequest, Idempotent, Money, OrderId, OrderLineId, RepoError,
    Scope, StageIdempotency, VariantId, earlier_result, idempotency::keys::IDEMPOTENCY_KEY,
};
use hypha_customers::{Customer, CustomerRepo, StageCustomers, repo::keys as customer_keys};
use hypha_inventory::{
    InventoryItem, MAX_ATTEMPTS, ReservationRef, StageInventoryItems, StockContext, reserve_lines,
};
use hypha_orders::{
    HasOrders, NewOrder, Order, OrderError, OrderLine, OrderRepo, OrderTax, OrderTaxSource,
    StageOrders, repo::keys as order_keys,
};
use hypha_tax::TaxSource;
use thiserror::Error;

/// The operation name checkout's idempotency records carry.
pub const CHECKOUT: &str = "checkout";

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Cart(#[from] hypha_cart::CommandError),
    #[error(transparent)]
    Inventory(#[from] hypha_inventory::CommandError),
    #[error(transparent)]
    Order(#[from] OrderError),
    #[error(transparent)]
    Idempotency(#[from] IdempotencyError),
    #[error("cart {0} not found")]
    CartNotFound(CartId),
    #[error("cart {cart} was already checked out as order {order}")]
    CartCompleted { cart: CartId, order: OrderId },
    #[error("cart {0} is empty")]
    EmptyCart(CartId),
    #[error("cart {0} has no email to send the order to")]
    MissingEmail(CartId),
    /// Lines that went stale since they were added (archived, out of stock, …). The
    /// priced cart (`get_cart`) says why for each.
    #[error("{} of the cart's lines can't be bought right now", .lines.len())]
    Unavailable { lines: Vec<VariantId> },
    /// The buyer confirmed one total, and prices, tax or stock changed since.
    #[error(
        "the cart's total is now {} {}, not the expected {} {}",
        .actual.amount(), .actual.currency(), .expected.amount(), .expected.currency()
    )]
    TotalChanged { expected: Money, actual: Money },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Cart(e) => e.code(),
            CommandError::Inventory(e) => e.code(),
            CommandError::Order(e) => e.code(),
            CommandError::Idempotency(e) => e.code(),
            CommandError::CartNotFound(_) => "cart_not_found",
            CommandError::CartCompleted { .. } => "cart_completed",
            CommandError::EmptyCart(_) => "cart_empty",
            CommandError::MissingEmail(_) => "cart_email_missing",
            CommandError::Unavailable { .. } => "cart_unavailable",
            CommandError::TotalChanged { .. } => "total_changed",
        }
    }
}

/// Everything checkout reads and writes: the cart's ports, orders, and a batch that
/// can stage every aggregate checkout touches. Anything with all of them is
/// `CheckoutPorts` (the blanket impl below).
pub trait CheckoutPorts:
    CartPorts
    + HasOrders
    + HasBatch<
        Batch: StageCarts + StageOrders + StageInventoryItems + StageCustomers + StageIdempotency,
    >
{
}

impl<T> CheckoutPorts for T where
    T: CartPorts
        + HasOrders
        + HasBatch<
            Batch: StageCarts
                       + StageOrders
                       + StageInventoryItems
                       + StageCustomers
                       + StageIdempotency,
        > + ?Sized
{
}

pub struct Checkout {
    pub cart: CartId,
    /// The total the buyer saw; if the cart now costs something else, checkout fails
    /// with `total_changed` instead of charging a different amount.
    pub expected_total: Option<Money>,
    /// A retry with the same key returns the order the first request placed.
    pub idempotency: Option<IdempotencyRequest>,
}

/// Turns a cart into an order (design note 0007, section 2).
///
/// Refuses a cart that is checked out already, empty, without an email, with any line
/// that can't be bought now, or whose total isn't `expected_total`. Otherwise reserves
/// stock for every line (the order line ID is the reservation's reference) and writes,
/// in one batch: the order, the cart marked completed, the changed inventory items, a
/// new customer when the buyer is a guest without one, and the idempotency key.
///
/// When another request changed something in between (stock, the cart, a customer with
/// the same email), the commit fails and checkout runs again on fresh data, up to
/// `MAX_ATTEMPTS` times; a real shortage or change then shows up as its own error.
pub async fn checkout<S: CheckoutPorts, I: IdGenerator, C: Clock>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    clock: &C,
    input: Checkout,
) -> Result<Idempotent<Order>, CommandError> {
    ctx.require(Scope::Checkout)?;
    if let Some(order) = replay_of(ctx, ports, input.idempotency.as_ref()).await? {
        return replay(ports, order).await;
    }

    let mut tries = 1;
    loop {
        match attempt(ctx, ports, ids, clock, &input).await {
            Err(e) if lost_a_race(&e) && tries < MAX_ATTEMPTS => tries += 1,
            result => return result,
        }
    }
}

/// Errors that mean another request committed first, so a fresh attempt can succeed:
/// a changed cart or item, a guest's customer created meanwhile, or the same cart
/// checked out (the next attempt then reports `cart_completed`).
fn lost_a_race(e: &CommandError) -> bool {
    matches!(
        e,
        CommandError::Repo(
            RepoError::StaleVersion
                | RepoError::Duplicate(customer_keys::CUSTOMER_EMAIL)
                | RepoError::Duplicate(order_keys::ORDER_CART)
        )
    )
}

async fn attempt<S: CheckoutPorts, I: IdGenerator, C: Clock>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    clock: &C,
    input: &Checkout,
) -> Result<Idempotent<Order>, CommandError> {
    let mut cart = ports
        .carts()
        .get(input.cart)
        .await?
        .ok_or(CommandError::CartNotFound(input.cart))?;
    if let CartStatus::Completed { order } = cart.status() {
        return Err(CommandError::CartCompleted {
            cart: input.cart,
            order,
        });
    }

    let (priced, stock) = price_for_checkout(ports, &cart).await?;
    if priced.lines.is_empty() {
        return Err(CommandError::EmptyCart(cart.id()));
    }
    let email = priced
        .email
        .clone()
        .ok_or(CommandError::MissingEmail(cart.id()))?;
    let unavailable: Vec<VariantId> = priced.unavailable_lines().map(|l| l.variant).collect();
    if !unavailable.is_empty() {
        return Err(CommandError::Unavailable { lines: unavailable });
    }
    if let Some(expected) = input.expected_total
        && expected != priced.total
    {
        return Err(CommandError::TotalChanged {
            expected,
            actual: priced.total,
        });
    }

    let lines = priced
        .lines
        .iter()
        .map(|line| order_line(line, OrderLineId::from_uuid(ids.new_uuid())))
        .collect::<Result<Vec<_>, _>>()?;

    // Reserve before taking a number, so a shortage doesn't use one up.
    let StockContext {
        items: mut stock_items,
        locations,
    } = stock;
    let reservations = lines
        .iter()
        .map(|l| {
            let reference = ReservationRef::new(&l.id.to_string())
                .map_err(hypha_inventory::CommandError::from)?;
            Ok((reference, l.variant, l.quantity))
        })
        .collect::<Result<Vec<_>, CommandError>>()?;
    let changed = reserve_lines(&mut stock_items, &locations, &reservations)?;

    // The buyer (0007, O10): the linked customer, else the customer with this email,
    // else a new one created with the order.
    let mut new_customer = None;
    let customer = match priced.customer {
        Some(linked) => linked,
        None => match ports.customers().get_by_email(&email).await? {
            Some(existing) => existing.id(),
            None => {
                let guest = Customer::new(CustomerId::from_uuid(ids.new_uuid()), email.clone());
                let id = guest.id();
                new_customer = Some(guest);
                id
            }
        },
    };

    let mut order = Order::place(NewOrder {
        id: OrderId::from_uuid(ids.new_uuid()),
        number: ports.orders().next_number().await?,
        placed_at: clock.now(),
        cart: cart.id(),
        market: priced.market,
        currency: priced.currency,
        country: priced.country,
        taxes_included: priced.taxes_included,
        customer,
        email,
        lines,
        subtotal: priced.subtotal,
        tax_total: priced.tax_total,
        total: priced.total,
    })?;
    cart.complete(order.id())
        .map_err(hypha_cart::CommandError::from)?;

    let mut batch = ports.batch();
    batch.insert_order(&mut order);
    batch.update_cart(&mut cart);
    let mut reserved: Vec<&mut InventoryItem> = stock_items
        .values_mut()
        .filter(|item| changed.contains(&item.variant()))
        .collect();
    for item in &mut reserved {
        batch.update_inventory_item(item);
    }
    if let Some(guest) = &mut new_customer {
        batch.insert_customer(guest);
    }
    if let Some(request) = &input.idempotency {
        let record = IdempotencyRecord::new(
            ctx.actor(),
            request,
            CHECKOUT,
            order.id().as_uuid(),
            clock.now(),
        )?;
        batch.record_idempotency(record);
    }

    match ports.commit(batch).await {
        // Another request with the same key placed its order first: replay it.
        Err(RepoError::Duplicate(IDEMPOTENCY_KEY)) => {
            match replay_of(ctx, ports, input.idempotency.as_ref()).await? {
                Some(winner) => replay(ports, winner).await,
                None => Err(RepoError::Duplicate(IDEMPOTENCY_KEY).into()),
            }
        }
        written => {
            written?;
            Ok(Idempotent::fresh(order))
        }
    }
}

/// An available line as the order keeps it: names, prices and tax copied (0007, O4).
fn order_line(line: &PricedLine, id: OrderLineId) -> Result<OrderLine, CommandError> {
    // Unavailable lines were refused above, so these always match; checked anyway.
    let unavailable = || CommandError::Unavailable {
        lines: vec![line.variant],
    };
    let LineStatus::Priced {
        unit_price,
        total,
        tax,
    } = &line.status
    else {
        return Err(unavailable());
    };
    let details = line.details.as_ref().ok_or_else(unavailable)?;
    Ok(OrderLine {
        id,
        product: line.product,
        variant: line.variant,
        quantity: line.quantity,
        product_title: details.product_title.clone(),
        variant_title: details.variant_title.clone(),
        sku: details.sku.clone(),
        unit_price: *unit_price,
        total: *total,
        tax: tax.map(|t| OrderTax {
            rate: t.rate,
            class: t.class,
            amount: t.amount,
            source: match t.source {
                TaxSource::RateTable { region } => OrderTaxSource::RateTable { region },
            },
        }),
    })
}

/// The order an earlier request with this key placed, if this caller sent it before;
/// `KeyReused` if the key was used for something else.
async fn replay_of<S: CheckoutPorts>(
    ctx: &Ctx,
    ports: &S,
    request: Option<&IdempotencyRequest>,
) -> Result<Option<OrderId>, CommandError> {
    let Some(request) = request else {
        return Ok(None);
    };
    let earlier: Option<_> =
        earlier_result::<_, CommandError>(ports.idempotency(), ctx.actor(), CHECKOUT, request)
            .await?;
    Ok(earlier.map(OrderId::from_uuid))
}

/// What a replay returns: the order as it is now.
async fn replay<S: CheckoutPorts>(
    ports: &S,
    order: OrderId,
) -> Result<Idempotent<Order>, CommandError> {
    match ports.orders().get(order).await? {
        Some(order) => Ok(Idempotent::replayed(order)),
        // Records only point at orders written in the same batch; orders aren't deleted.
        None => Err(RepoError::Backend(format!(
            "idempotency record points at missing order {order}"
        ))
        .into()),
    }
}
