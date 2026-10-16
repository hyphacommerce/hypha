use hypha_cart::{
    Cart, CartError, CartPorts, CartReading, CartRepo, CartStatus, LineStatus, PricedCart,
    PricedLine, SelectedShipping, ShippingProblem, StageCarts, StoredQuote, group_offers,
    read_cart,
};
use hypha_core::{
    Attempts, AuthError, CartId, Clock, Ctx, CustomerId, ErrorCode, HasBatch, HasIdempotency,
    IdGenerator, IdempotencyError, IdempotencyRecord, IdempotencyRequest, Idempotent, Money,
    OrderId, OrderLineId, RepoError, Scope, StageIdempotency, VariantId, Version, Versioned,
    earlier_id,
};
use hypha_customers::{Customer, CustomerRepo, StageCustomers, repo::keys as customer_keys};
use hypha_inventory::{
    InventoryItem, ReservationRef, StageInventoryItems, StockContext, reserve_lines,
};
use hypha_orders::{
    HasOrders, NewOrder, Order, OrderError, OrderLine, OrderRepo, OrderShippingLine, OrderTax,
    OrderTaxSource, StageOrders, repo::keys as order_keys,
};
use hypha_shipping::{OfferOutcome, RateProvider};
use hypha_tax::{TaxLine, TaxSource};
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
    /// Lines need shipping and the cart has no address (design note 0015, section 7).
    #[error("cart {0} needs a shipping address")]
    ShippingAddressRequired(CartId),
    /// A group of lines that need shipping has no option chosen.
    #[error("choose how to ship cart {0}")]
    ShippingOptionRequired(CartId),
    /// The chosen option can't be charged now: gone, disabled, not offered on renewal.
    #[error("the cart's shipping option can't be used now: {}", .0.code())]
    ShippingUnavailable(ShippingProblem),
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
            CommandError::ShippingAddressRequired(_) => "shipping_address_required",
            CommandError::ShippingOptionRequired(_) => "shipping_option_required",
            CommandError::ShippingUnavailable(_) => "shipping_option_unavailable",
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
pub async fn checkout<S: CheckoutPorts, I: IdGenerator, C: Clock, P: RateProvider>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    clock: &C,
    rates: &P,
    input: Checkout,
) -> Result<Idempotent<Order>, CommandError> {
    ctx.require(Scope::Checkout)?;
    if let Some(order) = replay_of(ctx, ports, input.idempotency.as_ref()).await? {
        return replay(ports, order).await;
    }

    let mut attempts = Attempts::first();
    // A quote renewed by an attempt that then lost a race: the next attempt uses it
    // rather than asking the provider again, if the cart hasn't changed since.
    let mut renewed = None;
    loop {
        let e = match attempt(ctx, ports, ids, clock, rates, &input, &mut renewed).await {
            Err(e) => e,
            placed => return placed,
        };
        // A request with the same key may have placed the order meanwhile: then this
        // attempt failed because of it (the cart is completed, its version moved on, or
        // the key is taken), and that order is this request's answer too.
        if let Some(order) = replay_of(ctx, ports, input.idempotency.as_ref()).await? {
            return replay(ports, order).await;
        }
        if !(lost_a_race(&e) && attempts.another()) {
            return Err(e);
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

async fn attempt<S: CheckoutPorts, I: IdGenerator, C: Clock, P: RateProvider>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    clock: &C,
    rates: &P,
    input: &Checkout,
    renewed: &mut Option<Renewal>,
) -> Result<Idempotent<Order>, CommandError> {
    let mut cart = ports
        .carts()
        .get(input.cart)
        .await?
        // Another channel's cart reads as missing (design note 0020).
        .filter(|cart| cart.visible_in(ctx.channel()))
        .ok_or(CommandError::CartNotFound(input.cart))?;
    if let CartStatus::Completed { order } = cart.status() {
        return Err(CommandError::CartCompleted {
            cart: input.cart,
            order,
        });
    }

    let reading = read_cart(ports, &cart).await?;
    // The cart's own problems first: renewing a quote can't fix them, and their codes
    // are what clients act on.
    if reading.priced.lines.is_empty() {
        return Err(CommandError::EmptyCart(cart.id()));
    }
    let email = reading
        .priced
        .email
        .clone()
        .ok_or(CommandError::MissingEmail(cart.id()))?;
    let unavailable: Vec<VariantId> = reading
        .priced
        .unavailable_lines()
        .map(|l| l.variant)
        .collect();
    if !unavailable.is_empty() {
        return Err(CommandError::Unavailable { lines: unavailable });
    }
    let reading =
        renew_expired_quote(ports, rates, clock, &mut cart, reading, input, renewed).await?;
    let CartReading { priced, stock, .. } = reading;
    check_shipping(&cart, &priced)?;
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

    let shipping_lines = shipping_lines(&priced, &lines)?;

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

    // The buyer (0007, O10): the linked customer, at the email they have now (the cart
    // keeps a copy from when it was linked, which goes stale if they change it); else
    // the customer with the cart's email; else a new one created with the order.
    let mut new_customer = None;
    let (customer, email) = match priced.customer {
        Some(linked) => match ports.customers().get(linked).await? {
            Some(buyer) => (linked, buyer.email().clone()),
            // Customers aren't deleted, so this is a broken link; the cart's copy will do.
            None => (linked, email),
        },
        None => match ports.customers().get_by_email(&email).await? {
            Some(existing) => (existing.id(), email),
            None => {
                let guest = Customer::new(CustomerId::from_uuid(ids.new_uuid()), email.clone());
                let id = guest.id();
                new_customer = Some(guest);
                (id, email)
            }
        },
    };

    let mut order = Order::place(NewOrder {
        id: OrderId::from_uuid(ids.new_uuid()),
        number: ports.orders().next_number().await?,
        placed_at: clock.now(),
        cart: cart.id(),
        channel: cart.channel(),
        market: priced.market,
        currency: priced.currency,
        country: priced.country,
        taxes_included: priced.taxes_included,
        customer,
        email,
        lines,
        shipping_address: priced.shipping_address.clone(),
        shipping_lines,
        subtotal: priced.subtotal,
        shipping_total: priced.shipping_total,
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

    // If another request with the same key committed first, `checkout` replays it.
    ports.commit(batch).await?;
    Ok(Idempotent::fresh(order))
}

/// Lines that need shipping need an address and, in every group, an option priced now.
fn check_shipping(cart: &Cart, priced: &PricedCart) -> Result<(), CommandError> {
    if priced.shipping_groups.is_empty() {
        return Ok(());
    }
    if priced.shipping_address.is_none() {
        return Err(CommandError::ShippingAddressRequired(cart.id()));
    }
    for group in &priced.shipping_groups {
        if let Some(problem) = &group.problem {
            return Err(CommandError::ShippingUnavailable(problem.clone()));
        }
        if group.selected.is_none() {
            return Err(CommandError::ShippingOptionRequired(cart.id()));
        }
    }
    Ok(())
}

/// A quote renewed for the cart at `version`. Only that version may reuse it: a cart
/// changed since (other lines, another address, a fresh choice) needs its own quote.
struct Renewal {
    version: Version,
    selection: SelectedShipping,
}

/// A provider's quote holds 15 minutes (design note 0015, SH9). Past that, the provider
/// is asked again: the new quote replaces the old on the cart, and a changed price
/// fails with `total_changed` when the buyer confirmed a total, after storing the new
/// quote, so the client shows the new total and retries with it.
async fn renew_expired_quote<S: CheckoutPorts, C: Clock, P: RateProvider>(
    ports: &S,
    rates: &P,
    clock: &C,
    cart: &mut Cart,
    reading: CartReading,
    input: &Checkout,
    carried: &mut Option<Renewal>,
) -> Result<CartReading, CommandError> {
    let now = clock.now();
    let Some(choice) = cart.shipping().cloned() else {
        return Ok(reading);
    };
    let Some(old) = choice.quote.as_ref().filter(|q| q.expires_at() <= now) else {
        return Ok(reading);
    };
    // Nothing ships any more: the old choice is moot, and `check_shipping` passes.
    if reading.priced.shipping_groups.is_empty() {
        return Ok(reading);
    }
    let version = cart.version();
    let selection = match carried.take().filter(|r| r.version == version) {
        Some(earlier) => earlier.selection,
        None => match ask_again(cart, &reading, rates, &choice, now).await? {
            Some(selection) => selection,
            // The option is built-in now: its price follows the cart without a quote.
            None => return Ok(reading),
        },
    };
    let amount = selection.quote.as_ref().map_or(old.amount, |q| q.amount);
    cart.select_shipping(Some(selection.clone()))
        .map_err(hypha_cart::CommandError::from)?;
    *carried = Some(Renewal { version, selection });
    let renewed = read_cart(ports, cart).await?;
    if amount != old.amount
        && let Some(expected) = input.expected_total
        && expected != renewed.priced.total
    {
        // Kept, so the buyer's next try sees (and confirms) the new price. Losing a race
        // here retries the whole checkout, which renews again for the changed cart.
        ports.carts().update(cart).await?;
        return Err(CommandError::TotalChanged {
            expected,
            actual: renewed.priced.total,
        });
    }
    Ok(renewed)
}

/// The chosen option's new quote, or `None` when it's built-in now. Only reasons the
/// option can't be offered become `shipping_option_unavailable` (the buyer picks again);
/// anything else (storage, money overflow) is reported as itself.
async fn ask_again<P: RateProvider>(
    cart: &Cart,
    reading: &CartReading,
    rates: &P,
    choice: &SelectedShipping,
    now: hypha_core::Timestamp,
) -> Result<Option<SelectedShipping>, CommandError> {
    let unavailable =
        || CommandError::ShippingUnavailable(ShippingProblem::OptionUnavailable(choice.option));
    let offers = match group_offers(cart, reading, rates).await {
        Ok(offers) => offers,
        // No zone serves the country any more: to the buyer, the option is gone.
        Err(hypha_cart::CommandError::NoShippingToCountry(_)) => return Err(unavailable()),
        Err(hypha_cart::CommandError::Cart(CartError::ShippingAddressRequired)) => {
            return Err(CommandError::ShippingAddressRequired(cart.id()));
        }
        Err(e) => return Err(e.into()),
    };
    let offer = offers
        .iter()
        .flat_map(|g| &g.offers)
        .find_map(|o| match o {
            OfferOutcome::Offered(offer) if offer.option == choice.option => Some(offer),
            _ => None,
        })
        .ok_or_else(unavailable)?;
    let Some(source) = &offer.quote else {
        return Ok(None);
    };
    let amount = hypha_core::Price::new(offer.amount).map_err(hypha_cart::CommandError::from)?;
    Ok(Some(SelectedShipping {
        option: choice.option,
        quote: Some(StoredQuote {
            amount,
            provider: source.provider.clone(),
            service: source.service.clone(),
            reference: source.reference.clone(),
            quoted_at: now,
        }),
    }))
}

/// The groups' choices as the order keeps them (design note 0015, SH12), each naming the
/// order lines it ships. `check_shipping` made sure every group has one.
fn shipping_lines(
    priced: &PricedCart,
    lines: &[OrderLine],
) -> Result<Vec<OrderShippingLine>, CommandError> {
    priced
        .shipping_groups
        .iter()
        .filter_map(|group| group.selected.as_ref().map(|selected| (group, selected)))
        .map(|(group, selected)| {
            // Prices are never negative, so this holds; a failure is reported, not
            // dropped (a missing line would fail the order's totals check instead).
            let amount =
                hypha_core::Price::new(selected.amount).map_err(hypha_cart::CommandError::from)?;
            Ok(OrderShippingLine {
                group: group.id,
                lines: lines
                    .iter()
                    .filter(|l| group.lines.contains(&l.variant))
                    .map(|l| l.id)
                    .collect(),
                option: selected.option,
                code: selected.code.clone(),
                name: selected.name.clone(),
                provider: selected
                    .quote
                    .as_ref()
                    .map(|q| q.provider.as_str().to_string()),
                service: selected.quote.as_ref().map(|q| q.service.clone()),
                reference: selected.quote.as_ref().and_then(|q| q.reference.clone()),
                amount,
                estimate_days: selected.estimate.map(|e| (e.min_days(), e.max_days())),
                tax: selected.tax.map(order_tax),
            })
        })
        .collect()
}

fn order_tax(t: TaxLine) -> OrderTax {
    OrderTax {
        rate: t.rate,
        class: t.class,
        amount: t.amount,
        source: match t.source {
            TaxSource::RateTable { region } => OrderTaxSource::RateTable { region },
        },
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
        tax: tax.map(order_tax),
        fulfilled_quantity: 0,
    })
}

/// The order an earlier request with this key placed, if this caller sent it before;
/// `KeyReused` if the key was used for something else.
async fn replay_of<S: HasIdempotency>(
    ctx: &Ctx,
    ports: &S,
    request: Option<&IdempotencyRequest>,
) -> Result<Option<OrderId>, CommandError> {
    earlier_id(
        ports.idempotency(),
        ctx.actor(),
        CHECKOUT,
        request,
        OrderId::from_uuid,
    )
    .await
}

/// What a replay returns: the order as it is now.
async fn replay<S: HasOrders>(
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
