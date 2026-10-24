use hypha_cart::{
    Cart, CartError, CartPorts, CartReading, CartRepo, CartStatus, DiscountReading, LineStatus,
    PricedCart, PricedLine, SelectedShipping, ShippingProblem, StageCarts, StoredQuote,
    discounts::{
        Application, Class, Share, StageDiscounts, UsageClaim,
        json::{target_json, value_json},
        keys as discount_keys,
    },
    group_offers, read_cart,
};
use hypha_core::{
    Attempts, AuthError, CartId, Clock, Ctx, CustomerId, ErrorCode, HasBatch, HasIdempotency,
    IdGenerator, IdempotencyError, IdempotencyRecord, IdempotencyRequest, Idempotent, Money,
    OrderId, OrderLineId, PaymentId, PaymentMethodId, RepoError, Scope, StageIdempotency,
    VariantId, Version, Versioned, earlier_id,
};
use hypha_customers::{
    Channel, ConsentChange, ConsentSource, ConsentState, Customer, CustomerRepo, OptInLevel,
    StageCustomers, repo::keys as customer_keys,
};
use hypha_inventory::{
    InventoryItem, ReservationRef, StageInventoryItems, StockContext, reserve_lines,
};
use hypha_orders::{
    DiscountClass, DiscountShare, HasOrders, NewOrder, Order, OrderDiscount, OrderError, OrderLine,
    OrderRepo, OrderShippingLine, OrderTax, OrderTaxSource, StageOrders, repo::keys as order_keys,
};
use hypha_payments::{
    HasPaymentMethods, HasPayments, NewPayment, NextAction, Payment, PaymentError, PaymentMethod,
    PaymentProviders, PaymentRepo, PaymentStatus, StagePayments, financial_status, newest_payment,
};
use hypha_shipping::{OfferOutcome, RateProvider};
use hypha_tax::{TaxLine, TaxSource};
use thiserror::Error;

use crate::start::{Payer, refresh, release, restart, start};

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
    #[error(transparent)]
    Payment(#[from] PaymentError),
    /// Recording the buyer's marketing consent.
    #[error(transparent)]
    Customer(#[from] hypha_customers::CustomerError),
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
    /// Lines need shipping and the cart has no address.
    #[error("cart {0} needs a shipping address")]
    ShippingAddressRequired(CartId),
    /// A group of lines that need shipping has no option chosen.
    #[error("choose how to ship cart {0}")]
    ShippingOptionRequired(CartId),
    /// The chosen option can't be charged now: gone, disabled, not offered on renewal.
    #[error("the cart's shipping option can't be used now: {}", .0.code())]
    ShippingUnavailable(ShippingProblem),
    /// Something to pay and no payment method chosen.
    #[error("choose how to pay for cart {0}")]
    PaymentMethodRequired(CartId),
    /// The chosen method isn't offered to the cart (any more): deleted, disabled, not in
    /// its market or channel, or of a provider this build doesn't have.
    #[error("payment method {0} isn't offered for this cart")]
    PaymentMethodUnavailable(PaymentMethodId),
    #[error("payment method {0} not found")]
    PaymentMethodNotFound(PaymentMethodId),
    /// A manual method (cash on delivery, bank transfer) has nothing to pay before the
    /// order: place it.
    #[error("payment method {0} is paid after the order: place the order")]
    PaymentMethodOffline(PaymentMethodId),
    /// The cart's total is zero: no payment to start.
    #[error("cart {0} has nothing to pay")]
    NothingToPay(CartId),
    /// The cart's money is committed already: check out instead of starting again (or
    /// choose that payment's method again, if another was chosen).
    #[error("cart {cart}'s payment {payment} is committed: check out")]
    CartPaymentCommitted { cart: CartId, payment: PaymentId },
    /// An online method and no payment started for the cart with it.
    #[error("start paying for cart {0} before checking out")]
    PaymentRequired(CartId),
    /// The cart's payment isn't committed yet: the shopper hasn't finished, or the
    /// provider hasn't said (it will: checkout completes then).
    #[error("cart {cart}'s payment {payment} isn't committed yet")]
    PaymentPending { cart: CartId, payment: PaymentId },
    /// A discount's last use (or the buyer's one use) went to another order since the cart
    /// was priced: nothing was placed. `total` is the cart's total now and `codes` its
    /// codes' states, for the buyer to see before trying again.
    #[error("the cart's discounts changed: its total is now {} {}", .total.amount(), .total.currency())]
    DiscountsChanged {
        total: Money,
        codes: Vec<hypha_cart::discounts::CodeOutcome>,
    },
    /// The cart's payment was declined or released: start paying again.
    #[error("cart {cart}'s payment {payment} is {status}: start paying again")]
    PaymentFailed {
        cart: CartId,
        payment: PaymentId,
        status: PaymentStatus,
    },
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
            CommandError::Payment(e) => e.code(),
            CommandError::Customer(e) => e.code(),
            CommandError::CartNotFound(_) => "cart_not_found",
            CommandError::CartCompleted { .. } => "cart_completed",
            CommandError::EmptyCart(_) => "cart_empty",
            CommandError::MissingEmail(_) => "cart_email_missing",
            CommandError::Unavailable { .. } => "cart_unavailable",
            CommandError::TotalChanged { .. } => "total_changed",
            CommandError::ShippingAddressRequired(_) => "shipping_address_required",
            CommandError::ShippingOptionRequired(_) => "shipping_option_required",
            CommandError::ShippingUnavailable(_) => "shipping_option_unavailable",
            CommandError::PaymentMethodRequired(_) => "payment_method_required",
            CommandError::PaymentMethodUnavailable(_) => "payment_method_unavailable",
            CommandError::PaymentMethodNotFound(_) => "payment_method_not_found",
            CommandError::PaymentMethodOffline(_) => "payment_method_offline",
            CommandError::NothingToPay(_) => "cart_nothing_to_pay",
            CommandError::CartPaymentCommitted { .. } => "cart_payment_committed",
            CommandError::PaymentRequired(_) => "payment_required",
            CommandError::PaymentPending { .. } => "payment_pending",
            CommandError::PaymentFailed { .. } => "payment_failed",
            CommandError::DiscountsChanged { .. } => "discounts_changed",
        }
    }
}

/// Everything checkout reads and writes: the cart's ports, orders, and a batch that
/// can stage every aggregate checkout touches. Anything with all of them is
/// `CheckoutPorts` (the blanket impl below).
pub trait CheckoutPorts:
    CartPorts
    + HasOrders
    + HasPaymentMethods
    + HasPayments
    + HasBatch<
        Batch: StageCarts
                   + StageOrders
                   + StageInventoryItems
                   + StageCustomers
                   + StageIdempotency
                   + StagePayments
                   + StageDiscounts,
    >
{
}

impl<T> CheckoutPorts for T where
    T: CartPorts
        + HasOrders
        + HasPaymentMethods
        + HasPayments
        + HasBatch<
            Batch: StageCarts
                       + StageOrders
                       + StageInventoryItems
                       + StageCustomers
                       + StageIdempotency
                       + StagePayments
                       + StageDiscounts,
        > + ?Sized
{
}

/// What checkout placed: the order, its payment (none when there was nothing to pay), and,
/// for a manual method, what the shopper does next (its instructions). `next_action` is
/// `None` without a payment, for one paid online (paid before the order), and once the
/// payment isn't open any more (a replay after staff recorded it); `Some(Err(…))` when a
/// manual provider couldn't start, which leaves the order placed and the payment pending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub order: Order,
    pub payment: Option<Payment>,
    pub next_action: Option<Result<NextAction, PaymentError>>,
}

pub struct Checkout {
    pub cart: CartId,
    /// The total the buyer saw; if the cart now costs something else, checkout fails
    /// with `total_changed` instead of charging a different amount.
    pub expected_total: Option<Money>,
    /// A retry with the same key returns the order the first request placed.
    pub idempotency: Option<IdempotencyRequest>,
    /// Where the storefront wants the buyer back after a payment provider's page (a
    /// redirect), passed to the provider.
    pub return_url: Option<String>,
}

/// Turns a cart into an order.
///
/// Refuses a cart that's empty, without an email, with any line that can't be bought now,
/// whose total isn't `expected_total`, or with something to pay and no payment method
/// offered to it (`payment_method_required`, `payment_method_unavailable`). An online
/// method's money must be committed first: the cart's payment started with it
/// (`start_cart_payment`; none is `payment_required`) is `authorized`, `processing` or
/// `paid`, its provider asked first while it's `pending` (still pending: `payment_pending`;
/// declined or released: `payment_failed`), and of the cart's total (`total_changed`). Then
/// reserves stock for every line (the order line ID is the reservation's reference) and
/// writes, in one batch: the order, its payment (the online one attached, or a manual
/// method's new pending one, its instructions copied), the order's payment status, the cart
/// marked completed, the changed inventory items, a new customer when the buyer is a guest
/// without one, and the idempotency key. A manual method's provider is asked to start after
/// the commit (`Placed::next_action`).
///
/// A cart checked out already is answered with its order (replayed): the shopper's
/// browser and the provider's webhook may both complete it. When an online payment's
/// money is committed and the order can't be placed (a line sold out meanwhile, the
/// total changed), the money is given back through the provider before the error is
/// answered.
///
/// When another request changed something in between (stock, the cart, a customer with
/// the same email), the commit fails and checkout runs again on fresh data, up to
/// `MAX_ATTEMPTS` times; a real shortage or change then shows up as its own error.
pub async fn checkout<
    S: CheckoutPorts,
    I: IdGenerator,
    C: Clock,
    P: RateProvider,
    Q: PaymentProviders,
>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    clock: &C,
    rates: &P,
    payments: &Q,
    input: Checkout,
) -> Result<Idempotent<Placed>, CommandError> {
    ctx.require(Scope::Checkout)?;
    run_checkout(ctx, ports, ids, clock, rates, payments, input).await
}

/// Completes the checkout a committed cart payment is for: what the host runs after a
/// provider reports a cart's payment `authorized`, `processing` or `paid`
/// (`payments:write`, the reporter's scope), so the order exists though the shopper never
/// came back. `None` when there's nothing to complete: the payment isn't a cart's, is
/// attached to its order already, or isn't committed. The same command as the shopper's
/// checkout: whichever comes second gets the order.
#[allow(clippy::too_many_arguments)]
pub async fn complete_paid_checkout<
    S: CheckoutPorts,
    I: IdGenerator,
    C: Clock,
    P: RateProvider,
    Q: PaymentProviders,
>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    clock: &C,
    rates: &P,
    payments: &Q,
    payment: PaymentId,
) -> Result<Option<Placed>, CommandError> {
    ctx.require(Scope::PaymentsWrite)?;
    let Some(found) = ports.payments().get(payment).await? else {
        return Ok(None);
    };
    let (Some(cart), None) = (found.cart(), found.order()) else {
        return Ok(None);
    };
    if !found.status().committed() {
        return Ok(None);
    }
    let input = Checkout {
        cart,
        expected_total: None,
        idempotency: None,
        return_url: None,
    };
    let placed = run_checkout(ctx, ports, ids, clock, rates, payments, input).await?;
    // The cart was checked out with another payment (another method, another tab): this
    // one's money is a second collection, and goes back.
    if placed.value.payment.as_ref().map(Payment::id) != Some(payment) {
        if let Some(stray) = ports.payments().get(payment).await?
            && stray.order().is_none()
            && stray.status().committed()
        {
            release(ports, payments, clock, stray).await;
        }
        return Ok(None);
    }
    Ok(Some(placed.value))
}

async fn run_checkout<
    S: CheckoutPorts,
    I: IdGenerator,
    C: Clock,
    P: RateProvider,
    Q: PaymentProviders,
>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    clock: &C,
    rates: &P,
    payments: &Q,
    input: Checkout,
) -> Result<Idempotent<Placed>, CommandError> {
    if let Some(order) = replay_of(ctx, ports, input.idempotency.as_ref()).await? {
        return replay(ports, payments, order).await;
    }

    let mut attempts = Attempts::first();
    // A quote renewed by an attempt that then lost a race: the next attempt uses it
    // rather than asking the provider again, if the cart hasn't changed since.
    let mut renewed = None;
    loop {
        let e = match attempt(
            ctx,
            ports,
            ids,
            clock,
            rates,
            payments,
            &input,
            &mut renewed,
        )
        .await
        {
            Err(e) => e,
            Ok(placed) => {
                let (order, mut payment, settings, replaced) = placed.value;
                // The cart's other payments, voided with the order: their providers are
                // asked to release them (best effort: money one takes anyway is a
                // second collection, given back by the provider's report).
                for old in replaced {
                    crate::start::release_pending(payments, old).await;
                }
                let next_action = match &mut payment {
                    // After the commit: nothing here may fail the checkout. Only a
                    // manual method starts now; an online one was paid before.
                    Some(p) if p.provider().is_manual() => {
                        Some(start(ports, payments, &payer_of(&order), p, settings, None).await)
                    }
                    _ => None,
                };
                return Ok(Idempotent::fresh(Placed {
                    order,
                    payment,
                    next_action,
                }));
            }
        };
        // Checked out already (by the provider's webhook, another tab, or a request with
        // the same key): that order is this request's answer too.
        if let CommandError::CartCompleted { order, .. } = e {
            return replay(ports, payments, order).await;
        }
        if let Some(order) = replay_of(ctx, ports, input.idempotency.as_ref()).await? {
            return replay(ports, payments, order).await;
        }
        if !(lost_a_race(&e) && attempts.another()) {
            // A checkout racing this one (the provider's webhook, another tab) may have
            // placed the order after this one read the cart, and then this one fails on
            // what that order took (the last of the stock): its order is the answer.
            if let Some(order) = completed_order(ports, input.cart).await {
                return replay(ports, payments, order).await;
            }
            if e.gives_money_back() {
                give_back(ports, payments, clock, input.cart).await;
            }
            return Err(e);
        }
    }
}

/// The order `cart` became, if it's checked out now. A storage failure reads as not
/// checked out (the caller answers its own error).
async fn completed_order<S: CartPorts>(ports: &S, cart: CartId) -> Option<OrderId> {
    match ports.carts().get(cart).await {
        Ok(Some(cart)) => match cart.status() {
            CartStatus::Completed { order } => Some(order),
            CartStatus::Open => None,
        },
        _ => None,
    }
}

/// What one attempt placed: the order, its payment, the payment method's provider
/// settings (for a manual method's `start`), and the cart's other pending payments it
/// voided (for their providers to release).
type Attempted = (Order, Option<Payment>, serde_json::Value, Vec<Payment>);

/// Errors that mean another request committed first, so a fresh attempt can succeed:
/// a changed cart, item or payment, a guest's customer created meanwhile, or the same
/// cart checked out (the next attempt then answers with its order).
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

impl CommandError {
    /// Whether this error means the order can't be placed as the shopper paid for it, so
    /// money the cart's payment committed goes back: a line sold out, the total changed,
    /// the cart can't be checked out as it is. Not for errors that may pass (storage, a
    /// race, a shipping option whose provider is down: the provider's report delivered
    /// again, or the shopper choosing again, completes it) or that are about the payment
    /// itself.
    pub fn gives_money_back(&self) -> bool {
        match self {
            CommandError::Cart(hypha_cart::CommandError::Repo(_))
            | CommandError::Inventory(hypha_inventory::CommandError::Repo(_))
            | CommandError::ShippingUnavailable(_) => false,
            CommandError::Cart(_)
            | CommandError::Inventory(_)
            | CommandError::Order(_)
            | CommandError::EmptyCart(_)
            | CommandError::MissingEmail(_)
            | CommandError::Unavailable { .. }
            | CommandError::TotalChanged { .. }
            | CommandError::DiscountsChanged { .. }
            | CommandError::ShippingAddressRequired(_)
            | CommandError::ShippingOptionRequired(_)
            | CommandError::PaymentMethodRequired(_)
            | CommandError::PaymentMethodUnavailable(_) => true,
            CommandError::Auth(_)
            | CommandError::Repo(_)
            | CommandError::Idempotency(_)
            | CommandError::Payment(_)
            | CommandError::Customer(_)
            | CommandError::CartNotFound(_)
            | CommandError::CartCompleted { .. }
            | CommandError::PaymentMethodNotFound(_)
            | CommandError::PaymentMethodOffline(_)
            | CommandError::NothingToPay(_)
            | CommandError::CartPaymentCommitted { .. }
            | CommandError::PaymentRequired(_)
            | CommandError::PaymentPending { .. }
            | CommandError::PaymentFailed { .. } => false,
        }
    }
}

/// Gives back what the cart's committed payments without an order hold (`release`). A
/// pending one's provider is asked first: the shopper may have paid before its report.
/// Never fails: the shopper's error is what the caller answers; what couldn't be done is
/// logged, and the provider's report gives it back then.
async fn give_back<S: HasPayments, Q: PaymentProviders>(
    ports: &S,
    providers: &Q,
    clock: &impl Clock,
    cart: CartId,
) {
    let payments = match ports.payments().list_for_cart(cart).await {
        Ok(payments) => payments,
        Err(e) => {
            tracing::warn!(%cart, error = %e, "checkout: couldn't read the cart's payments to give their money back");
            return;
        }
    };
    for mut payment in payments {
        if payment.order().is_some() {
            continue;
        }
        if payment.status() == PaymentStatus::Pending
            && let Err(e) = refresh(ports, providers, clock, &mut payment).await
        {
            tracing::warn!(payment = %payment.id(), error = %e, "checkout: couldn't ask where a payment is before giving money back");
            continue;
        }
        if payment.status().committed() {
            release(ports, providers, clock, payment).await;
        }
    }
}

/// Who pays for `order`, for its provider.
fn payer_of(order: &Order) -> Payer {
    Payer {
        cart: order.cart(),
        order: Some((order.id(), order.number().value())),
        channel: order.channel(),
        market: order.market(),
        country: order.country(),
        email: order.email().clone(),
    }
}

/// A cart ready to be checked out: every check passed, priced now, with the method that
/// pays for it (`None`: nothing to pay).
pub(crate) struct Ready {
    pub cart: Cart,
    pub reading: CartReading,
    pub method: Option<PaymentMethod>,
}

/// Checkout's checks (the cart's lines, email, shipping, total and method), shared with
/// `start_cart_payment`, which runs them before any money moves. `renewed` carries a
/// quote renewed by an earlier attempt; a renewal changes `cart`, which the caller
/// writes.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn ready<S: CheckoutPorts, C: Clock, P: RateProvider, Q: PaymentProviders>(
    ctx: &Ctx,
    ports: &S,
    clock: &C,
    rates: &P,
    payments: &Q,
    id: CartId,
    expected_total: Option<Money>,
    renewed: &mut Option<Renewal>,
) -> Result<Ready, CommandError> {
    let mut cart = ports
        .carts()
        .get(id)
        .await?
        // Another channel's cart reads as missing.
        .filter(|cart| cart.visible_in(ctx.channel()))
        .ok_or(CommandError::CartNotFound(id))?;
    if let CartStatus::Completed { order } = cart.status() {
        return Err(CommandError::CartCompleted { cart: id, order });
    }

    let reading = read_cart(ports, &cart).await?;
    // The cart's own problems first: renewing a quote can't fix them, and their codes
    // are what clients act on.
    if reading.priced.lines.is_empty() {
        return Err(CommandError::EmptyCart(cart.id()));
    }
    if reading.priced.email.is_none() {
        return Err(CommandError::MissingEmail(cart.id()));
    }
    let unavailable: Vec<VariantId> = reading
        .priced
        .unavailable_lines()
        .map(|l| l.variant)
        .collect();
    if !unavailable.is_empty() {
        return Err(CommandError::Unavailable { lines: unavailable });
    }
    let reading = renew_expired_quote(
        ports,
        rates,
        clock,
        &mut cart,
        reading,
        expected_total,
        renewed,
    )
    .await?;
    check_shipping(&cart, &reading.priced)?;
    if let Some(expected) = expected_total
        && expected != reading.priced.total
    {
        return Err(CommandError::TotalChanged {
            expected,
            actual: reading.priced.total,
        });
    }
    let method = crate::payment::checkout_method(ports, payments, &cart, &reading.priced).await?;
    Ok(Ready {
        cart,
        reading,
        method,
    })
}

/// The cart's committed payment an online `method` pays with, its provider asked first
/// while it's pending.
async fn committed_payment<S: HasPayments, Q: PaymentProviders>(
    ports: &S,
    providers: &Q,
    clock: &impl Clock,
    cart: CartId,
    method: &PaymentMethod,
    total: Money,
) -> Result<Payment, CommandError> {
    let payments = ports.payments().list_for_cart(cart).await?;
    let open = || payments.iter().filter(|p| p.order().is_none());
    if let Some(other) = open().find(|p| p.status().committed() && p.method() != Some(method.id()))
    {
        // Paid with another method than the one chosen now: choose it again.
        return Err(CommandError::CartPaymentCommitted {
            cart,
            payment: other.id(),
        });
    }
    let mut payment = open()
        .find(|p| p.method() == Some(method.id()))
        .cloned()
        .ok_or(CommandError::PaymentRequired(cart))?;
    if payment.status() == PaymentStatus::Pending {
        refresh(ports, providers, clock, &mut payment).await?;
    }
    match payment.status() {
        status if status.committed() => {}
        PaymentStatus::Pending => {
            return Err(CommandError::PaymentPending {
                cart,
                payment: payment.id(),
            });
        }
        status => {
            return Err(CommandError::PaymentFailed {
                cart,
                payment: payment.id(),
                status,
            });
        }
    }
    if payment.amount() != total {
        return Err(CommandError::TotalChanged {
            expected: payment.amount(),
            actual: total,
        });
    }
    Ok(payment)
}

#[allow(clippy::too_many_arguments)]
async fn attempt<
    S: CheckoutPorts,
    I: IdGenerator,
    C: Clock,
    P: RateProvider,
    Q: PaymentProviders,
>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    clock: &C,
    rates: &P,
    payments: &Q,
    input: &Checkout,
    renewed: &mut Option<Renewal>,
) -> Result<Idempotent<Attempted>, CommandError> {
    let Ready {
        mut cart,
        reading,
        method,
    } = ready(
        ctx,
        ports,
        clock,
        rates,
        payments,
        input.cart,
        input.expected_total,
        renewed,
    )
    .await?;
    let CartReading {
        priced,
        stock,
        discounts,
        ..
    } = reading;
    let email = priced
        .email
        .clone()
        .ok_or(CommandError::MissingEmail(cart.id()))?;
    // An online method's money first: nothing is reserved or placed without it.
    let mut paid = match method.as_ref().filter(|m| !m.provider().is_manual()) {
        Some(online) => {
            Some(committed_payment(ports, payments, clock, cart.id(), online, priced.total).await?)
        }
        None => {
            if let Some(other) = ports
                .payments()
                .list_for_cart(cart.id())
                .await?
                .into_iter()
                .find(|p| p.order().is_none() && p.status().committed())
            {
                return Err(CommandError::CartPaymentCommitted {
                    cart: cart.id(),
                    payment: other.id(),
                });
            }
            None
        }
    };

    let lines = priced
        .lines
        .iter()
        .map(|line| order_line(line, OrderLineId::from_uuid(ids.new_uuid())))
        .collect::<Result<Vec<_>, _>>()?;

    let shipping_lines = shipping_lines(&priced, &lines)?;
    let order_discounts: Vec<OrderDiscount> = priced.discounts.iter().map(order_discount).collect();

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

    // The buyer: the linked customer, at the email they have now (the cart keeps a copy
    // from when it was linked, which goes stale if they change it); else the customer with
    // the cart's email; else a new one created with the order.
    let mut new_customer = None;
    let mut known_customer = None;
    let (customer, email) = match priced.customer {
        Some(linked) => match ports.customers().get(linked).await? {
            Some(buyer) => {
                let email = buyer.email().clone();
                known_customer = Some(buyer);
                (linked, email)
            }
            // A deleted customer's carts are unlinked, so this is a broken link; the
            // cart's copy will do.
            None => (linked, email),
        },
        None => match ports.customers().get_by_email(&email).await? {
            Some(existing) => {
                let id = existing.id();
                known_customer = Some(existing);
                (id, email)
            }
            None => {
                let guest = Customer::new(CustomerId::from_uuid(ids.new_uuid()), email.clone());
                let id = guest.id();
                new_customer = Some(guest);
                (id, email)
            }
        },
    };
    // The marketing box: ticked, the buyer subscribes from checkout, unless a newer consent
    // is stored; unticked never unsubscribes anyone. An erased customer can't consent: the
    // order still goes through.
    if priced.marketing_opt_in {
        let change = ConsentChange {
            state: ConsentState::Subscribed,
            opt_in_level: OptInLevel::Single,
            consented_at: None,
        };
        let now = clock.now();
        if let Some(guest) = &mut new_customer {
            guest.set_marketing_consent(Channel::Email, change, ConsentSource::Checkout, now)?;
        }
        if let Some(buyer) = &mut known_customer {
            let subscribed = !buyer.is_erased()
                && buyer.set_marketing_consent(
                    Channel::Email,
                    change,
                    ConsentSource::Checkout,
                    now,
                )?;
            if !subscribed {
                known_customer = None;
            }
        }
    } else {
        known_customer = None;
    }

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
        discounts: order_discounts,
        discount_total: priced.discount_total,
        subtotal: priced.subtotal,
        shipping_total: priced.shipping_total,
        tax_total: priced.tax_total,
        total: priced.total,
    })?;
    cart.complete(order.id())
        .map_err(hypha_cart::CommandError::from)?;
    let mut created = None;
    match &mut paid {
        Some(payment) => payment.attach(order.id())?,
        None => {
            created = method
                .as_ref()
                .map(|m| {
                    let id = hypha_core::PaymentId::from_uuid(ids.new_uuid());
                    Payment::new(NewPayment::by(
                        m,
                        id,
                        cart.id(),
                        Some(order.id()),
                        order.total(),
                    ))
                })
                .transpose()?;
        }
    }
    // The cart's other pending payments (another method started, another tab): voided
    // with the order, so money one of them takes later is a second collection, given
    // back.
    let kept = paid.as_ref().map(Payment::id);
    let now = clock.now();
    let mut replaced: Vec<Payment> = ports
        .payments()
        .list_for_cart(cart.id())
        .await?
        .into_iter()
        .filter(|p| {
            p.order().is_none() && p.status() == PaymentStatus::Pending && Some(p.id()) != kept
        })
        .collect();
    for old in &mut replaced {
        old.void(now)?;
    }
    let payment_status: Vec<Payment> = paid.iter().chain(created.iter()).cloned().collect();
    order.set_financial_status(financial_status(order.total(), &payment_status)?);

    let mut batch = ports.batch();
    batch.insert_order(&mut order);
    if let Some(payment) = &mut paid {
        batch.update_payment(payment);
    }
    if let Some(payment) = &mut created {
        batch.insert_payment(payment);
    }
    for old in &mut replaced {
        batch.update_payment(old);
    }
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
    if let Some(buyer) = &mut known_customer {
        batch.update_customer(buyer);
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

    for claim in usage_claims(&priced, &discounts, order.id(), customer, now) {
        batch.claim_discount_usage(claim);
    }

    // If another request with the same key committed first, `checkout` replays it.
    match ports.commit(batch).await {
        Ok(()) => {}
        // A discount's last use went elsewhere since this read: the buyer sees the cart
        // as it is now rather than paying another total.
        Err(RepoError::Duplicate(
            discount_keys::DISCOUNT_USAGE_LIMIT | discount_keys::DISCOUNT_PER_CUSTOMER,
        )) => {
            let fresh = ports
                .carts()
                .get(cart.id())
                .await?
                .ok_or(CommandError::CartNotFound(cart.id()))?;
            let now = read_cart(ports, &fresh).await?.priced;
            return Err(CommandError::DiscountsChanged {
                total: now.total,
                codes: now.discount_codes,
            });
        }
        Err(e) => return Err(e.into()),
    }
    let settings = method
        .map(|m| m.settings().clone())
        .unwrap_or(serde_json::Value::Null);
    Ok(Idempotent::fresh((
        order,
        paid.or(created),
        settings,
        replaced,
    )))
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
pub(crate) struct Renewal {
    version: Version,
    selection: SelectedShipping,
}

/// A provider's quote holds 15 minutes. Past that, the provider is asked again: the new
/// quote replaces the old on the cart, and a changed price fails with `total_changed` when
/// the buyer confirmed a total, after storing the new quote, so the client shows the new
/// total and retries with it.
async fn renew_expired_quote<S: CheckoutPorts, C: Clock, P: RateProvider>(
    ports: &S,
    rates: &P,
    clock: &C,
    cart: &mut Cart,
    reading: CartReading,
    expected_total: Option<Money>,
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
        && let Some(expected) = expected_total
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

/// The groups' choices as the order keeps them, each naming the order lines it ships.
/// `check_shipping` made sure every group has one.
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
                discounts: order_shares(&selected.discounts),
                total: selected.total,
                estimate_days: selected.estimate.map(|e| (e.min_days(), e.max_days())),
                tax: selected.tax.map(order_tax),
            })
        })
        .collect()
}

/// A discount applied to the cart as the order keeps it: copies, so the order reads the
/// same after the discount changes or goes.
fn order_discount(a: &Application) -> OrderDiscount {
    OrderDiscount {
        discount: a.discount,
        code: a.code.clone(),
        title: a.title.clone(),
        summary: a.summary.clone(),
        class: match a.class {
            Class::Product => DiscountClass::Product,
            Class::Order => DiscountClass::Order,
            Class::Shipping => DiscountClass::Shipping,
        },
        value: value_json(&a.value),
        target: target_json(&a.target),
        amount: a.amount,
    }
}

fn order_shares(shares: &[Share]) -> Vec<DiscountShare> {
    shares
        .iter()
        .map(|s| DiscountShare {
            // A cart has a handful of applications: far below `u16::MAX`.
            application: u16::try_from(s.application).unwrap_or(u16::MAX),
            amount: s.amount,
            units: s.units,
        })
        .collect()
}

/// One claim per discount the order got: the discount's counter under its limit, the
/// code's under its own, and a once-per-customer use. What it took off is the sum of its
/// rewards' amounts.
fn usage_claims(
    priced: &PricedCart,
    discounts: &DiscountReading,
    order: OrderId,
    customer: CustomerId,
    at: hypha_core::Timestamp,
) -> Vec<UsageClaim> {
    let mut claims: Vec<UsageClaim> = Vec::new();
    for a in &priced.discounts {
        if let Some(claim) = claims.iter_mut().find(|c| c.discount == a.discount) {
            claim.amount = claim.amount.checked_add(a.amount).unwrap_or(claim.amount);
            continue;
        }
        let Some(discount) = discounts.context.find(a.discount) else {
            continue;
        };
        let code = a.code.as_ref().map(|c| {
            let limit = discounts.context.code(c).and_then(|row| row.usage_limit);
            (c.clone(), limit)
        });
        claims.push(UsageClaim {
            discount: a.discount,
            code,
            order,
            customer,
            once_per_customer: discount.limits().once_per_customer,
            amount: a.amount,
            at,
        });
    }
    claims
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

/// An available line as the order keeps it: names, prices and tax copied.
fn order_line(line: &PricedLine, id: OrderLineId) -> Result<OrderLine, CommandError> {
    // Unavailable lines were refused above, so these always match; checked anyway.
    let unavailable = || CommandError::Unavailable {
        lines: vec![line.variant],
    };
    let LineStatus::Priced {
        unit_price,
        compare_at,
        price_list,
        subtotal,
        discounts,
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
        subtotal: *subtotal,
        discounts: order_shares(discounts),
        total: *total,
        tax: tax.map(order_tax),
        fulfilled_quantity: 0,
        compare_at_price: *compare_at,
        price_list: *price_list,
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

/// What a replay returns: the order and its newest payment as they are now, and, while
/// a manual method's payment is pending, its instructions again.
async fn replay<S: HasOrders + HasPayments + HasPaymentMethods, Q: PaymentProviders>(
    ports: &S,
    providers: &Q,
    order: OrderId,
) -> Result<Idempotent<Placed>, CommandError> {
    let Some(order) = ports.orders().get(order).await? else {
        // Records and completed carts only point at orders written in the same batch;
        // orders aren't deleted.
        return Err(RepoError::Backend(format!("checkout points at missing order {order}")).into());
    };
    let mut payment = newest_payment(ports, order.id()).await?;
    let next_action = match &mut payment {
        Some(p) if p.status() == PaymentStatus::Pending && p.provider().is_manual() => {
            Some(restart(ports, providers, &payer_of(&order), p).await?)
        }
        _ => None,
    };
    Ok(Idempotent::replayed(Placed {
        order,
        payment,
        next_action,
    }))
}
