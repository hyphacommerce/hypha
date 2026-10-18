use std::collections::{HashMap, HashSet};

use hypha_core::{
    Attempts, AuthError, Clock, Ctx, ErrorCode, FulfillmentId, HasBatch, HasIdempotency,
    IdGenerator, IdempotencyError, IdempotencyRecord, IdempotencyRequest, Idempotent, LocationId,
    OrderId, OrderLineId, Quantity, QuantityError, RepoError, Scope, StageIdempotency, VariantId,
    earlier_id,
};
use hypha_fulfillment::{
    Fulfillment, FulfillmentError, FulfillmentLine, FulfillmentRepo, HasFulfillments,
    NewFulfillment, StageFulfillments, TrackingInput,
};
use hypha_inventory::{
    HasInventoryItems, HasLocations, InventoryError, InventoryItem, InventoryItemRepo,
    LocationRepo, ReservationRef, StageInventoryItems,
};
use hypha_orders::{CancelReason, HasOrders, Order, OrderError, OrderRepo, StageOrders};
use hypha_payments::{
    HasPayments, NewPayment, Payment, PaymentError, PaymentRepo, PaymentStatus, StagePayments,
    financial_status,
};
use thiserror::Error;

/// The operation name `fulfill`'s idempotency records carry.
pub const FULFILL: &str = "fulfill";

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Order(#[from] OrderError),
    #[error(transparent)]
    Fulfillment(#[from] FulfillmentError),
    #[error(transparent)]
    Inventory(#[from] InventoryError),
    #[error(transparent)]
    Quantity(#[from] QuantityError),
    #[error(transparent)]
    Idempotency(#[from] IdempotencyError),
    #[error(transparent)]
    Payment(#[from] PaymentError),
    #[error("order {0} not found")]
    OrderNotFound(OrderId),
    #[error("fulfilment {0} not found")]
    FulfillmentNotFound(FulfillmentId),
    #[error("location {0} not found")]
    LocationNotFound(LocationId),
    /// Cancelling an order with standing fulfilments: these, to cancel first (FU7).
    #[error("order {order} has {} standing fulfilments; cancel them first", .fulfillments.len())]
    HasFulfillments {
        order: OrderId,
        fulfillments: Vec<FulfillmentId>,
    },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Order(e) => e.code(),
            CommandError::Fulfillment(e) => e.code(),
            CommandError::Inventory(e) => e.code(),
            CommandError::Quantity(e) => e.code(),
            CommandError::Idempotency(e) => e.code(),
            CommandError::Payment(e) => e.code(),
            CommandError::OrderNotFound(_) => "order_not_found",
            CommandError::FulfillmentNotFound(_) => "fulfillment_not_found",
            CommandError::LocationNotFound(_) => "location_not_found",
            CommandError::HasFulfillments { .. } => "order_has_fulfillments",
        }
    }
}

/// Everything the order actions read and write, and a batch that stages all of it.
/// Anything with all of them is `OrderActionPorts` (the blanket impl below).
pub trait OrderActionPorts:
    HasOrders
    + HasFulfillments
    + HasInventoryItems
    + HasLocations
    + HasIdempotency
    + HasPayments
    + HasBatch<
        Batch: StageOrders
                   + StageFulfillments
                   + StageInventoryItems
                   + StageIdempotency
                   + StagePayments,
    >
{
}

impl<T> OrderActionPorts for T where
    T: HasOrders
        + HasFulfillments
        + HasInventoryItems
        + HasLocations
        + HasIdempotency
        + HasPayments
        + HasBatch<
            Batch: StageOrders
                       + StageFulfillments
                       + StageInventoryItems
                       + StageIdempotency
                       + StagePayments,
        > + ?Sized
{
}

/// A line to fulfil and how many of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FulfilLine {
    pub line: OrderLineId,
    pub quantity: u32,
}

pub struct Fulfil {
    pub order: OrderId,
    /// Where the units leave from (design note 0018, FU5).
    pub location: LocationId,
    pub lines: Vec<FulfilLine>,
    pub tracking: Vec<TrackingInput>,
    /// A retry with the same key returns the fulfilment the first request recorded.
    pub idempotency: Option<IdempotencyRequest>,
}

/// A fulfilment and its order, as they are after the action: what a response shows and
/// what the buyer's email is made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fulfilled {
    pub fulfillment: Fulfillment,
    pub order: Order,
}

/// Records that `lines` of the order left `location` (design note 0018, section 3).
///
/// Refuses a cancelled order (`order_cancelled`), a line the order doesn't have
/// (`unknown_order_line`), a line listed twice, more than a line has left to ship
/// (`over_fulfilled`) and a location that doesn't exist. Otherwise writes, in one batch:
/// the fulfilment, the order with its lines' counts and status, each tracked item with
/// the units taken out of its reservation and its stock (deleted when it was retired and
/// nothing is reserved on it any more), and the idempotency key.
///
/// When another request changed the order or an item in between, it runs again on
/// fresh data, up to `MAX_ATTEMPTS` times, so a concurrent fulfilment of the same lines
/// shows up as `over_fulfilled`.
pub async fn fulfill<S: OrderActionPorts>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    clock: &impl Clock,
    input: Fulfil,
) -> Result<Idempotent<Fulfilled>, CommandError> {
    ctx.require(Scope::FulfillmentsWrite)?;
    let lines = input
        .lines
        .iter()
        .map(|l| Ok((l.line, Quantity::new(l.quantity)?)))
        .collect::<Result<Vec<_>, QuantityError>>()?;
    let tracking = TrackingInput::parse_all(&input.tracking)?;
    if let Some(earlier) = replay_of(ctx, ports, input.idempotency.as_ref()).await? {
        return replay(ports, earlier).await;
    }
    let id = FulfillmentId::from_uuid(ids.new_uuid());
    let attempt = Attempt {
        ctx,
        id,
        input: &input,
        lines: &lines,
        tracking: &tracking,
    };

    let mut attempts = Attempts::first();
    loop {
        let e = match attempt.run(ports, clock).await {
            Err(e) => e,
            done => return done,
        };
        // A request with the same key may have committed meanwhile, and this attempt
        // failed because of it: its fulfilment is this request's answer too.
        if let Some(earlier) = replay_of(ctx, ports, input.idempotency.as_ref()).await? {
            return replay(ports, earlier).await;
        }
        if !(lost_a_race(&e) && attempts.another()) {
            return Err(e);
        }
    }
}

/// One try of `fulfill`, on fresh data; every try records the same fulfilment ID.
struct Attempt<'a> {
    ctx: &'a Ctx,
    id: FulfillmentId,
    input: &'a Fulfil,
    lines: &'a [(OrderLineId, Quantity)],
    tracking: &'a [hypha_fulfillment::Tracking],
}

impl Attempt<'_> {
    async fn run<S: OrderActionPorts>(
        &self,
        ports: &S,
        clock: &impl Clock,
    ) -> Result<Idempotent<Fulfilled>, CommandError> {
        let mut order = load_order(ports, self.input.order).await?;
        let location = self.input.location;
        if ports.locations().get(location).await?.is_none() {
            return Err(CommandError::LocationNotFound(location));
        }
        let mut stock = Stock::load(ports, &order).await?;
        // The fulfilment's own rules first (each line once), then the order's. Each line
        // notes whether its units come off stock, for taking it back later.
        let mut fulfillment = Fulfillment::new(NewFulfillment {
            id: self.id,
            order: order.id(),
            location,
            lines: self
                .lines
                .iter()
                .map(|&(line, quantity)| FulfillmentLine {
                    line,
                    quantity,
                    stock_taken: stock.tracks(&order, line),
                })
                .collect(),
            tracking: self.tracking.to_vec(),
        })?;
        order.fulfil(self.lines)?;

        for &(line, quantity) in self.lines {
            stock.change(&order, line, |item, reference| {
                item.fulfil(reference, location, quantity)
            })?;
        }

        let mut batch = ports.batch();
        batch.insert_fulfillment(&mut fulfillment);
        batch.update_order(&mut order);
        stock.stage(&mut batch)?;
        if let Some(request) = &self.input.idempotency {
            let record = IdempotencyRecord::new(
                self.ctx.actor(),
                request,
                FULFILL,
                self.id.as_uuid(),
                clock.now(),
            )?;
            batch.record_idempotency(record);
        }
        ports.commit(batch).await?;
        Ok(Idempotent::fresh(Fulfilled { fulfillment, order }))
    }
}

/// Takes back a fulfilment recorded by mistake (design note 0018, section 3): its lines
/// count as unfulfilled again, and the units that came off stock when it shipped are
/// back on hand at its location (and reserved there for their lines while tracked);
/// lines that took no stock put none back. One batch, retried like `fulfill`.
/// Cancelling it again changes nothing.
pub async fn cancel_fulfillment<S: OrderActionPorts>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: FulfillmentId,
) -> Result<Fulfilled, CommandError> {
    ctx.require(Scope::FulfillmentsWrite)?;
    let mut attempts = Attempts::first();
    loop {
        match cancel_fulfillment_once(ports, clock, id).await {
            Err(e) if lost_a_race(&e) && attempts.another() => continue,
            done => return done,
        }
    }
}

async fn cancel_fulfillment_once<S: OrderActionPorts>(
    ports: &S,
    clock: &impl Clock,
    id: FulfillmentId,
) -> Result<Fulfilled, CommandError> {
    let mut fulfillment = ports
        .fulfillments()
        .get(id)
        .await?
        .ok_or(CommandError::FulfillmentNotFound(id))?;
    let mut order = load_order(ports, fulfillment.order()).await?;
    if !fulfillment.stands() {
        return Ok(Fulfilled { fulfillment, order });
    }
    let lines: Vec<(OrderLineId, Quantity)> = fulfillment
        .lines()
        .iter()
        .map(|l| (l.line, l.quantity))
        .collect();
    order.unfulfil(&lines)?;
    let location = fulfillment.location();
    let mut stock = Stock::load(ports, &order).await?;
    for shipped in fulfillment.lines().iter().filter(|l| l.stock_taken) {
        stock.change(&order, shipped.line, |item, reference| {
            item.restock(reference, location, shipped.quantity)?;
            Ok(true)
        })?;
    }
    fulfillment.cancel(clock.now());

    let mut batch = ports.batch();
    batch.update_fulfillment(&mut fulfillment);
    batch.update_order(&mut order);
    stock.stage(&mut batch)?;
    ports.commit(batch).await?;
    Ok(Fulfilled { fulfillment, order })
}

pub struct CancelOrder {
    pub order: OrderId,
    pub reason: CancelReason,
}

/// An order and its payments, newest first, as they are after an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderPayments {
    pub order: Order,
    pub payments: Vec<Payment>,
}

/// Cancels an order (design note 0018, FU7): every line's reservation is released, and
/// its pending payments are voided (design note 0027, PA8: cash never collected, a
/// transfer never sent), in the same batch as the order, whose payment status follows
/// (`voided` once every payment is). Refused while fulfilments stand
/// (`order_has_fulfillments`, listing them). A paid payment stays paid until refunds
/// exist. Cancelling it again changes nothing, and says so (`false`), so only the
/// request that cancelled it tells the buyer.
pub async fn cancel_order<S: OrderActionPorts>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    input: CancelOrder,
) -> Result<(OrderPayments, bool), CommandError> {
    ctx.require(Scope::OrdersWrite)?;
    let mut attempts = Attempts::first();
    loop {
        match cancel_order_once(ports, clock, &input).await {
            Err(e) if lost_a_race(&e) && attempts.another() => continue,
            done => return done,
        }
    }
}

async fn cancel_order_once<S: OrderActionPorts>(
    ports: &S,
    clock: &impl Clock,
    input: &CancelOrder,
) -> Result<(OrderPayments, bool), CommandError> {
    let mut order = load_order(ports, input.order).await?;
    let now = clock.now();
    match order.cancel(now, input.reason) {
        Ok(true) => {}
        Ok(false) => {
            let payments = ports.payments().list_for_order(order.id()).await?;
            return Ok((OrderPayments { order, payments }, false));
        }
        Err(OrderError::HasFulfillments(id)) => {
            let fulfillments = ports
                .fulfillments()
                .list_for_order(id)
                .await?
                .iter()
                .filter(|f| f.stands())
                .map(Fulfillment::id)
                .collect();
            return Err(CommandError::HasFulfillments {
                order: id,
                fulfillments,
            });
        }
        Err(e) => return Err(e.into()),
    }
    let mut stock = Stock::load(ports, &order).await?;
    let lines: Vec<OrderLineId> = order.lines().iter().map(|l| l.id).collect();
    for line in lines {
        stock.change(&order, line, |item, reference| Ok(item.release(reference)))?;
    }
    let mut payments = ports.payments().list_for_order(order.id()).await?;
    let mut voided = Vec::new();
    for (i, payment) in payments.iter_mut().enumerate() {
        if payment.status() == PaymentStatus::Pending && payment.void(now)? {
            voided.push(i);
        }
    }
    // Only voiding moves the payment status: an order without payments keeps its own.
    if !voided.is_empty() {
        order.set_financial_status(financial_status(order.total(), &payments)?);
    }

    let mut batch = ports.batch();
    batch.update_order(&mut order);
    for i in voided {
        batch.update_payment(&mut payments[i]);
    }
    stock.stage(&mut batch)?;
    ports.commit(batch).await?;
    Ok((OrderPayments { order, payments }, true))
}

/// Recording money staff received by hand (design note 0027, §5).
pub struct MarkPaid {
    pub order: OrderId,
    /// What staff typed: a bank reference, the courier's receipt.
    pub reference: Option<String>,
}

/// Records the order's pending payment as paid, with `reference` (design note 0027,
/// §5): the order's payment status follows (`paid`, recording `order.paid`), in one
/// batch. An order with no open payment (placed before payments, or with nothing to pay
/// at checkout) gets a `manual` payment of its total, created and paid on the spot, so
/// this works for every order.
///
/// Marking a paid order again changes nothing, even when two requests mark it at once
/// (the loser of the race reloads and finds it paid). A cancelled order is refused
/// (`order_cancelled`).
pub async fn mark_paid<S: OrderActionPorts>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    clock: &impl Clock,
    input: MarkPaid,
) -> Result<OrderPayments, CommandError> {
    ctx.require(Scope::OrdersWrite)?;
    let mut attempts = Attempts::first();
    loop {
        match mark_paid_once(ports, ids, clock, &input).await {
            Err(e) if lost_a_race(&e) && attempts.another() => continue,
            done => return done,
        }
    }
}

async fn mark_paid_once<S: OrderActionPorts>(
    ports: &S,
    ids: &impl IdGenerator,
    clock: &impl Clock,
    input: &MarkPaid,
) -> Result<OrderPayments, CommandError> {
    let mut order = load_order(ports, input.order).await?;
    let mut payments = ports.payments().list_for_order(order.id()).await?;
    if order.financial_status() == hypha_core::FinancialStatus::Paid {
        return Ok(OrderPayments { order, payments });
    }
    order.ensure_not_cancelled()?;
    let now = clock.now();
    // The newest open one: what checkout started.
    let open = payments.iter().position(|p| p.status().is_open());
    let (index, created) = match open {
        Some(i) => (i, false),
        None => {
            let id = hypha_core::PaymentId::from_uuid(ids.new_uuid());
            let payment =
                Payment::new(NewPayment::recorded_by_hand(id, order.id(), order.total()))?;
            payments.insert(0, payment);
            (0, true)
        }
    };
    payments[index].pay(now, input.reference.clone())?;
    order.set_financial_status(financial_status(order.total(), &payments)?);

    let mut batch = ports.batch();
    if created {
        batch.insert_payment(&mut payments[index]);
    } else {
        batch.update_payment(&mut payments[index]);
    }
    batch.update_order(&mut order);
    ports.commit(batch).await?;
    Ok(OrderPayments { order, payments })
}

/// Another request committed first (the order, the fulfilment or an item changed), so
/// a fresh attempt can succeed or report the real problem.
fn lost_a_race(e: &CommandError) -> bool {
    matches!(e, CommandError::Repo(RepoError::StaleVersion))
}

async fn load_order<S: HasOrders>(ports: &S, id: OrderId) -> Result<Order, CommandError> {
    ports
        .orders()
        .get(id)
        .await?
        .ok_or(CommandError::OrderNotFound(id))
}

/// The reservation reference of an order line (design note 0004, I8).
fn reference(line: OrderLineId) -> Result<ReservationRef, InventoryError> {
    ReservationRef::new(&line.to_string())
}

/// The inventory items of an order's variants, loaded once per attempt, and which of
/// them an action changed. A variant without an item is untracked: nothing to change.
struct Stock {
    items: HashMap<VariantId, InventoryItem>,
    changed: HashSet<VariantId>,
}

/// The inventory items of `order`'s variants that have one: what every order action
/// reads and changes, and what `order_view` reads.
pub(crate) async fn order_items<S: HasInventoryItems>(
    ports: &S,
    order: &Order,
) -> Result<Vec<InventoryItem>, RepoError> {
    let mut variants: Vec<VariantId> = order.lines().iter().map(|l| l.variant).collect();
    variants.sort_by_key(|v| v.as_uuid());
    variants.dedup();
    ports.inventory_items().for_variants(&variants).await
}

impl Stock {
    async fn load<S: HasInventoryItems>(ports: &S, order: &Order) -> Result<Self, RepoError> {
        let items = order_items(ports, order)
            .await?
            .into_iter()
            .map(|item| (item.variant(), item))
            .collect();
        Ok(Self {
            items,
            changed: HashSet::new(),
        })
    }

    /// Whether `line`'s units come off stock: its variant has an item, and it's tracked.
    fn tracks(&self, order: &Order, line: OrderLineId) -> bool {
        order
            .lines()
            .iter()
            .find(|l| l.id == line)
            .and_then(|l| self.items.get(&l.variant))
            .is_some_and(InventoryItem::tracked)
    }

    /// Runs `change` on the item of `line`'s variant with the line's reservation
    /// reference, if the variant has an item; `change` says whether anything changed.
    fn change(
        &mut self,
        order: &Order,
        line: OrderLineId,
        change: impl FnOnce(&mut InventoryItem, &ReservationRef) -> Result<bool, InventoryError>,
    ) -> Result<(), CommandError> {
        // The order checked the line exists before any stock is touched.
        let Some(variant) = order
            .lines()
            .iter()
            .find(|l| l.id == line)
            .map(|l| l.variant)
        else {
            return Err(OrderError::UnknownLine(line).into());
        };
        let Some(item) = self.items.get_mut(&variant) else {
            return Ok(());
        };
        if change(item, &reference(line)?)? {
            self.changed.insert(variant);
        }
        Ok(())
    }

    /// Stages every changed item: deleted when it's retired and nothing is reserved on
    /// it any more (design note 0016, Q5), updated otherwise.
    fn stage(mut self, batch: &mut impl StageInventoryItems) -> Result<(), CommandError> {
        for variant in &self.changed {
            let Some(item) = self.items.get_mut(variant) else {
                continue;
            };
            if item.is_spent() {
                item.delete()?;
                batch.delete_inventory_item(item);
            } else {
                batch.update_inventory_item(item);
            }
        }
        Ok(())
    }
}

/// The fulfilment an earlier request with this key recorded, if this caller sent it
/// before; `KeyReused` if the key was used for something else.
async fn replay_of<S: HasIdempotency>(
    ctx: &Ctx,
    ports: &S,
    request: Option<&IdempotencyRequest>,
) -> Result<Option<FulfillmentId>, CommandError> {
    earlier_id(
        ports.idempotency(),
        ctx.actor(),
        FULFILL,
        request,
        FulfillmentId::from_uuid,
    )
    .await
}

/// What a replay returns: the fulfilment and its order as they are now.
async fn replay<S: HasFulfillments + HasOrders>(
    ports: &S,
    id: FulfillmentId,
) -> Result<Idempotent<Fulfilled>, CommandError> {
    // Records only point at fulfilments written in the same batch, which aren't deleted.
    let Some(fulfillment) = ports.fulfillments().get(id).await? else {
        return Err(RepoError::Backend(format!(
            "idempotency record points at missing fulfilment {id}"
        ))
        .into());
    };
    let order = load_order(ports, fulfillment.order()).await?;
    Ok(Idempotent::replayed(Fulfilled { fulfillment, order }))
}
