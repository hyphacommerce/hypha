use std::collections::{HashMap, HashSet};

use hypha_core::{
    Attempts, AuthError, Clock, Ctx, Currency, ErrorCode, HasBatch, HasIdempotency, IdGenerator,
    IdempotencyError, IdempotencyRecord, IdempotencyRequest, Idempotent, InventoryItemId,
    LocationId, MoneyError, PurchaseOrderId, Quantity, QuantityError, ReceiptId, RepoError, Scope,
    StageIdempotency, Timestamp, UnitCost, VariantId, earlier_id, money::div_round,
};
use hypha_inventory::{
    HasInventoryItems, HasLocations, InventoryError, InventoryItem, InventoryItemRepo,
    LocationRepo, StageInventoryItems, StockSource, repo::keys as inventory_keys,
};
use hypha_purchasing::{
    HasPurchaseOrders, HasReceipts, HasSupplierVariants, NewReceipt, PurchaseOrder,
    PurchaseOrderRepo, PurchasingError, Receipt, ReceiptBasis, ReceiptLine, ReceiptRepo,
    ReceiptState, ReceivedQuantities, Revaluation, StagePurchaseOrders, StageReceipts,
    UpdatePurchaseOrder, VariantInfo,
};
use thiserror::Error;

/// The operation name `receive`'s idempotency records carry.
pub const RECEIVE: &str = "receive";

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Purchasing(#[from] PurchasingError),
    /// The order's own command refused (an update's lines or costs).
    #[error(transparent)]
    PurchaseOrder(#[from] hypha_purchasing::CommandError),
    #[error(transparent)]
    Inventory(#[from] InventoryError),
    #[error(transparent)]
    Quantity(#[from] QuantityError),
    #[error(transparent)]
    Idempotency(#[from] IdempotencyError),
    #[error("purchase order {0} not found")]
    PurchaseOrderNotFound(PurchaseOrderId),
    #[error("receipt {0} not found")]
    ReceiptNotFound(ReceiptId),
    #[error("location {0} not found")]
    LocationNotFound(LocationId),
    /// Received goods carry a cost, kept in the store's base currency: it must be chosen
    /// first.
    #[error("choose the store's base currency before receiving")]
    StoreCurrencyRequired,
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Purchasing(e) => e.code(),
            CommandError::PurchaseOrder(e) => e.code(),
            CommandError::Inventory(e) => e.code(),
            CommandError::Quantity(e) => e.code(),
            CommandError::Idempotency(e) => e.code(),
            CommandError::PurchaseOrderNotFound(_) => "purchase_order_not_found",
            CommandError::ReceiptNotFound(_) => "receipt_not_found",
            CommandError::LocationNotFound(_) => "location_not_found",
            CommandError::StoreCurrencyRequired => "store_currency_required",
        }
    }
}

/// Everything receiving reads and writes, and a batch that stages all of it. Anything
/// with all of them is `ReceivingPorts` (the blanket impl below).
pub trait ReceivingPorts:
    HasPurchaseOrders
    + HasReceipts
    + HasInventoryItems
    + HasLocations
    + HasIdempotency
    + HasBatch<Batch: StagePurchaseOrders + StageReceipts + StageInventoryItems + StageIdempotency>
{
}

impl<T> ReceivingPorts for T where
    T: HasPurchaseOrders
        + HasReceipts
        + HasInventoryItems
        + HasLocations
        + HasIdempotency
        + HasBatch<
            Batch: StagePurchaseOrders + StageReceipts + StageInventoryItems + StageIdempotency,
        > + ?Sized
{
}

/// One variant's units arriving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiveLine {
    pub variant: VariantId,
    /// Into stock.
    pub accepted: u32,
    /// Refused: recorded, no stock.
    pub rejected: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receive {
    pub purchase_order: PurchaseOrderId,
    /// Where the goods arrived; the order's destination when `None`.
    pub location: Option<LocationId>,
    pub lines: Vec<ReceiveLine>,
    /// Take more than a line has outstanding (a supplier's extra units).
    pub allow_over: bool,
    /// The lines' variants the catalogue no longer has (the host reads its products):
    /// their units are counted on the order and the receipt but go into no stock, so no
    /// stock record is made for a variant that's gone.
    pub removed_variants: Vec<VariantId>,
    pub note: Option<String>,
    /// A retry with the same key returns the receipt the first request recorded.
    pub idempotency: Option<IdempotencyRequest>,
}

/// A receipt and its order as they are after it: what a response shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    pub receipt: Receipt,
    pub purchase_order: PurchaseOrder,
    /// Variants that had no stock record, given one now, untracked so whether they sell
    /// doesn't change: the merchant may turn tracking on. Empty on a replay.
    pub new_items: Vec<VariantId>,
}

/// A receipt taken back and its order after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelledReceipt {
    pub receipt: Receipt,
    pub purchase_order: PurchaseOrder,
}

/// Records goods arriving against an ordered purchase order.
///
/// Refuses an order that isn't ordered (`purchase_order_not_ordered`), a variant the
/// order doesn't have (`unknown_purchase_order_line`), a line listed twice or without a
/// unit, more than a line has outstanding unless `allow_over` (`over_received`, with
/// each such line's outstanding), a location that doesn't exist, and a store without a
/// base currency. Otherwise writes, in one batch: the receipt (each line with its landed
/// unit cost in the store's currency, at the order's rate), the order with its counts
/// (closed once every line is accounted for), each accepted variant's stock with its
/// moving average and a `received` row (a variant with no stock record gets one,
/// untracked; a removed variant's units go into no stock), and the idempotency key.
/// Units over what a line spreads its extra costs over come in at their unit cost
/// alone, so the stock never carries more of them than was paid.
///
/// When another request changed the order or an item in between, it runs again on fresh
/// data, up to `MAX_ATTEMPTS` times, so a concurrent receipt of the same units shows up
/// as `over_received`.
pub async fn receive<S: ReceivingPorts>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    clock: &impl Clock,
    store_currency: Option<Currency>,
    input: Receive,
) -> Result<Idempotent<Received>, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    if let Some(earlier) = replay_of(ctx, ports, input.idempotency.as_ref()).await? {
        return replay(ports, earlier).await;
    }
    let store = store_currency.ok_or(CommandError::StoreCurrencyRequired)?;
    let id = ReceiptId::from_uuid(ids.new_uuid());
    let mut attempts = Attempts::first();
    loop {
        let e = match receive_once(ctx, ports, ids, clock, store, id, &input).await {
            Err(e) => e,
            done => return done,
        };
        // A request with the same key may have committed meanwhile, and this attempt
        // failed because of it: its receipt is this request's answer too.
        if let Some(earlier) = replay_of(ctx, ports, input.idempotency.as_ref()).await? {
            return replay(ports, earlier).await;
        }
        if !(lost_a_race(&e) && attempts.another()) {
            return Err(e);
        }
    }
}

async fn receive_once<S: ReceivingPorts>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    clock: &impl Clock,
    store: Currency,
    id: ReceiptId,
    input: &Receive,
) -> Result<Idempotent<Received>, CommandError> {
    let mut po = load_order(ports, input.purchase_order).await?;
    // What each line's units cost landed, before the counts change: the extra costs are
    // spread over what was ordered, not over what has arrived.
    let landed_per_line: HashMap<VariantId, Option<UnitCost>> = po
        .lines()
        .iter()
        .map(|l| l.variant())
        .zip(po.landed_costs(store, po.exchange_rate())?)
        .collect();
    let mut landed = HashMap::new();
    for line in &input.lines {
        let full = landed_per_line.get(&line.variant).copied().flatten();
        let bare = po.bare_landed_cost(line.variant, store, po.exchange_rate())?;
        let bearing = po.extra_bearing(line.variant, line.accepted);
        landed.insert(line.variant, blended(full, bare, bearing, line.accepted)?);
    }
    let extras = po.extras_per_unit(store, po.exchange_rate())?;
    let counted: Vec<ReceivedQuantities> = input
        .lines
        .iter()
        .map(|l| ReceivedQuantities {
            variant: l.variant,
            accepted: l.accepted,
            rejected: l.rejected,
        })
        .collect();
    let now = clock.now();
    po.record_receipt(&counted, input.allow_over, now)?;
    for line in &counted {
        if let Some(Some(extra)) = extras.get(&line.variant) {
            po.add_extra_applied(line.variant, line.accepted, *extra)?;
        }
    }
    let Some(location) = input.location.or(po.destination()) else {
        return Err(PurchasingError::DestinationRequired.into());
    };
    if ports.locations().get(location).await?.is_none() {
        return Err(CommandError::LocationNotFound(location));
    }
    let mut receipt = Receipt::new(NewReceipt {
        id,
        purchase_order: po.id(),
        location,
        lines: input
            .lines
            .iter()
            .map(|l| ReceiptLine {
                variant: l.variant,
                accepted: l.accepted,
                rejected: l.rejected,
                landed_unit_cost: landed.get(&l.variant).copied().flatten(),
            })
            .collect(),
        exchange_rate: po.exchange_rate(),
        note: input.note.clone(),
    })?;

    let source = StockSource::Receipt {
        purchase_order: po.id(),
        receipt: id,
    };
    let arriving: Vec<&ReceiptLine> = receipt
        .lines()
        .iter()
        .filter(|l| l.accepted > 0 && !input.removed_variants.contains(&l.variant))
        .collect();
    let variants: Vec<VariantId> = arriving.iter().map(|l| l.variant).collect();
    let mut items: HashMap<VariantId, InventoryItem> = ports
        .inventory_items()
        .for_variants(&variants)
        .await?
        .into_iter()
        .map(|item| (item.variant(), item))
        .collect();
    let mut new_items = Vec::new();
    for line in &arriving {
        let item = match items.get_mut(&line.variant) {
            Some(item) => item,
            None => {
                new_items.push(line.variant);
                let item_id = InventoryItemId::from_uuid(ids.new_uuid());
                items
                    .entry(line.variant)
                    .or_insert_with(|| InventoryItem::new(item_id, line.variant, false, false))
            }
        };
        let quantity = Quantity::new(line.accepted)?;
        item.receive(location, quantity, line.landed_unit_cost, source)?;
    }

    let mut batch = ports.batch();
    batch.update_purchase_order(&mut po);
    batch.insert_receipt(&mut receipt);
    for variant in &variants {
        let Some(item) = items.get_mut(variant) else {
            continue;
        };
        if new_items.contains(variant) {
            batch.insert_inventory_item(item);
        } else {
            batch.update_inventory_item(item);
        }
    }
    if let Some(request) = &input.idempotency {
        let record =
            IdempotencyRecord::new(ctx.actor(), request, RECEIVE, id.as_uuid(), clock.now())?;
        batch.record_idempotency(record);
    }
    ports.commit(batch).await?;
    Ok(Idempotent::fresh(Received {
        receipt,
        purchase_order: po,
        new_items,
    }))
}

/// Takes back a receipt recorded by mistake: its counts come off the order's lines (a
/// closed order opens again), and its accepted units come off stock at its location
/// (which may go below zero), their value out of each item's moving average. When the
/// extra costs changed since it arrived, what the rest of the order's stock carries of
/// them is brought up to date too (`store_currency` is the store's). One batch, retried
/// like `receive`. Refused for a receipt taken back already (`receipt_cancelled`) or on
/// a cancelled order.
pub async fn cancel_receipt<S: ReceivingPorts>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    store_currency: Option<Currency>,
    id: ReceiptId,
) -> Result<CancelledReceipt, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let mut attempts = Attempts::first();
    loop {
        match cancel_receipt_once(ports, clock, store_currency, id).await {
            Err(e) if lost_a_race(&e) && attempts.another() => continue,
            done => return done,
        }
    }
}

async fn cancel_receipt_once<S: ReceivingPorts>(
    ports: &S,
    clock: &impl Clock,
    store: Option<Currency>,
    id: ReceiptId,
) -> Result<CancelledReceipt, CommandError> {
    let mut receipt = ports
        .receipts()
        .get(id)
        .await?
        .ok_or(CommandError::ReceiptNotFound(id))?;
    receipt.cancel(clock.now())?;
    let mut po = load_order(ports, receipt.purchase_order()).await?;
    // What the order's stock carries of the extra costs, before and after this
    // receipt's units go: they leave at the receipt's own landed cost.
    let others: Vec<ReceiptBasis> = recorded(ports, po.id())
        .await?
        .into_iter()
        .filter(|r| r.id() != id)
        .map(|r| r.basis())
        .collect();
    let mut before = HashMap::new();
    if let Some(store) = store {
        let mut all = others.clone();
        all.push(receipt.basis());
        before = po.extra_due(store, &all)?;
        for line in receipt.lines().iter().filter(|l| l.accepted > 0) {
            let bare = po.bare_landed_cost(line.variant, store, receipt.exchange_rate())?;
            let (Some(landed), Some(bare)) = (line.landed_unit_cost, bare) else {
                continue;
            };
            let extra = i128::from(landed.scaled()) - i128::from(bare.scaled());
            po.take_extra_applied(line.variant, line.accepted, extra)?;
            if let Some(due) = before.get_mut(&line.variant) {
                *due -= i128::from(line.accepted) * extra;
            }
        }
    }
    po.take_back_receipt(&receipt.quantities())?;

    let source = StockSource::Receipt {
        purchase_order: po.id(),
        receipt: id,
    };
    let taken: Vec<ReceiptLine> = receipt
        .lines()
        .iter()
        .filter(|l| l.accepted > 0)
        .copied()
        .collect();
    let variants: Vec<VariantId> = taken.iter().map(|l| l.variant).collect();
    // A variant removed since has no stock record left to take its units off.
    let mut items: HashMap<VariantId, InventoryItem> = ports
        .inventory_items()
        .for_variants(&variants)
        .await?
        .into_iter()
        .map(|item| (item.variant(), item))
        .collect();
    let mut touched = HashSet::new();
    for line in &taken {
        if let Some(item) = items.get_mut(&line.variant) {
            let quantity = Quantity::new(line.accepted)?;
            item.unreceive(receipt.location(), quantity, line.landed_unit_cost, source)?;
            touched.insert(line.variant);
        }
    }
    if let Some(store) = store {
        let changes = po.revalue(store, &others, &before)?;
        load_more(ports, &mut items, &changes).await?;
        let late = StockSource::PurchaseOrder(po.id());
        touched.extend(revalue_stock(&mut items, &changes, store, late)?);
    }

    let mut batch = ports.batch();
    batch.update_purchase_order(&mut po);
    batch.update_receipt(&mut receipt);
    // Only what changed: staging the rest would bump versions checkouts race on.
    for (variant, item) in &mut items {
        if touched.contains(variant) {
            batch.update_inventory_item(item);
        }
    }
    ports.commit(batch).await?;
    Ok(CancelledReceipt {
        receipt,
        purchase_order: po,
    })
}

/// Changes a purchase order (`hypha_purchasing::update_purchase_order`'s rules) and,
/// when goods arrived on it, revalues their stock for what the change does to the extra
/// costs they carry (a cost added, changed or removed, a line's quantity or cost that
/// changes how they're spread): each item's cost moves by its share of the difference
/// over the units on hand (`InventoryItem::revalue`, a `cost_adjustment` row caused by
/// the order). A closed order takes changes to its extra costs only. One batch, retried
/// on a lost race. `store_currency` is the store's; without one no stock arrived.
pub async fn update_purchase_order<S: ReceivingPorts + HasSupplierVariants>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    store_currency: Option<Currency>,
    variants: &[VariantInfo],
    id: PurchaseOrderId,
    input: UpdatePurchaseOrder,
) -> Result<PurchaseOrder, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let mut attempts = Attempts::first();
    loop {
        let change = Change::Update {
            variants,
            input: Box::new(input.clone()),
        };
        match revaluing(ports, clock.now(), store_currency, id, change).await {
            Err(e) if lost_a_race(&e) && attempts.another() => continue,
            done => return done,
        }
    }
}

/// Ends an ordered purchase order early (`PurchaseOrder::close`): what's outstanding is
/// cancelled, so the extra costs spread over fewer units and the stock that arrived is
/// revalued for its bigger share, in the same batch. Retried on a lost race.
pub async fn close_purchase_order<S: ReceivingPorts + HasSupplierVariants>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    store_currency: Option<Currency>,
    id: PurchaseOrderId,
) -> Result<PurchaseOrder, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let mut attempts = Attempts::first();
    loop {
        match revaluing(ports, clock.now(), store_currency, id, Change::Close).await {
            Err(e) if lost_a_race(&e) && attempts.another() => continue,
            done => return done,
        }
    }
}

/// What `revaluing` does to the order.
enum Change<'a> {
    Update {
        variants: &'a [VariantInfo],
        input: Box<UpdatePurchaseOrder>,
    },
    Close,
}

/// Loads the order, applies `change` and writes it, with its stock revalued for the
/// extra costs its received units carry now, in one batch.
async fn revaluing<S: ReceivingPorts + HasSupplierVariants>(
    ports: &S,
    now: Timestamp,
    store: Option<Currency>,
    id: PurchaseOrderId,
    change: Change<'_>,
) -> Result<PurchaseOrder, CommandError> {
    let mut po = load_order(ports, id).await?;
    let arrived = store.filter(|_| po.lines().iter().any(|l| l.received() > 0));
    let mut receipts = Vec::new();
    let mut before = HashMap::new();
    if let Some(store) = arrived {
        receipts = recorded(ports, id)
            .await?
            .iter()
            .map(Receipt::basis)
            .collect();
        before = po.extra_due(store, &receipts)?;
    }
    match change {
        Change::Update { variants, input } => {
            hypha_purchasing::apply_update(ports, now, store, variants, &mut po, *input).await?;
        }
        Change::Close => po.close(now)?,
    }
    let kept = kept_values(&po);
    let mut items = HashMap::new();
    let mut touched = HashSet::new();
    if let Some(store) = arrived {
        let changes = po.revalue(store, &receipts, &before)?;
        load_more(ports, &mut items, &changes).await?;
        touched = revalue_stock(&mut items, &changes, store, StockSource::PurchaseOrder(id))?;
    }
    if !po.is_changed() && touched.is_empty() && kept == kept_values(&po) {
        return Ok(po);
    }
    let mut batch = ports.batch();
    batch.update_purchase_order(&mut po);
    // Only the items revalued: an unchanged one keeps its version for checkouts.
    for (variant, item) in &mut items {
        if touched.contains(variant) {
            batch.update_inventory_item(item);
        }
    }
    ports.commit(batch).await?;
    Ok(po)
}

/// What each line keeps of the extra costs' value: revaluing may start one where there
/// was none, which the order's row must store though nothing else changed.
fn kept_values(po: &PurchaseOrder) -> Vec<Option<i64>> {
    po.lines().iter().map(|l| l.extra_applied()).collect()
}

/// Loads into `items` the stock records of the variants `changes` name that it lacks.
async fn load_more<S: HasInventoryItems>(
    ports: &S,
    items: &mut HashMap<VariantId, InventoryItem>,
    changes: &[Revaluation],
) -> Result<(), CommandError> {
    let missing: Vec<VariantId> = changes
        .iter()
        .map(|c| c.variant)
        .filter(|v| !items.contains_key(v))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    for item in ports.inventory_items().for_variants(&missing).await? {
        items.insert(item.variant(), item);
    }
    Ok(())
}

/// Puts each revaluation on its variant's stock (loaded into `items`); a variant whose
/// stock record is gone has nothing left to value. The variants whose cost changed.
fn revalue_stock(
    items: &mut HashMap<VariantId, InventoryItem>,
    changes: &[Revaluation],
    store: Currency,
    source: StockSource,
) -> Result<HashSet<VariantId>, CommandError> {
    let mut changed = HashSet::new();
    for change in changes {
        if let Some(item) = items.get_mut(&change.variant)
            && item.revalue(change.value, store, source)?
        {
            changed.insert(change.variant);
        }
    }
    Ok(changed)
}

/// The cost `accepted` units come in at when only `bearing` of them carry the extra
/// costs (`full`, the landed cost) and the rest their unit cost alone (`bare`): one
/// average per receipt line, so its stock value is what was paid.
fn blended(
    full: Option<UnitCost>,
    bare: Option<UnitCost>,
    bearing: u32,
    accepted: u32,
) -> Result<Option<UnitCost>, CommandError> {
    let (Some(full), Some(bare)) = (full, bare) else {
        return Ok(full);
    };
    if bearing >= accepted {
        return Ok(Some(full));
    }
    let value = i128::from(bearing) * i128::from(full.scaled())
        + i128::from(accepted - bearing) * i128::from(bare.scaled());
    let scaled = div_round(value, i128::from(accepted));
    let scaled = i64::try_from(scaled).map_err(|_| PurchasingError::from(MoneyError::Overflow))?;
    Ok(Some(
        UnitCost::new(scaled, full.currency()).map_err(PurchasingError::from)?,
    ))
}

/// The order's receipts that weren't taken back.
async fn recorded<S: HasReceipts>(
    ports: &S,
    po: PurchaseOrderId,
) -> Result<Vec<Receipt>, CommandError> {
    Ok(ports
        .receipts()
        .for_purchase_order(po)
        .await?
        .into_iter()
        .filter(|r| r.state() == ReceiptState::Recorded)
        .collect())
}

/// Lost a race: the order, the receipt or an item changed since it was read, or another
/// writer gave a variant its stock record first.
fn lost_a_race(e: &CommandError) -> bool {
    matches!(
        e,
        CommandError::Repo(
            RepoError::StaleVersion | RepoError::Duplicate(inventory_keys::INVENTORY_ITEM_VARIANT)
        )
    )
}

async fn load_order<S: HasPurchaseOrders>(
    ports: &S,
    id: PurchaseOrderId,
) -> Result<PurchaseOrder, CommandError> {
    ports
        .purchase_orders()
        .get(id)
        .await?
        .ok_or(CommandError::PurchaseOrderNotFound(id))
}

async fn replay_of<S: HasIdempotency>(
    ctx: &Ctx,
    ports: &S,
    request: Option<&IdempotencyRequest>,
) -> Result<Option<ReceiptId>, CommandError> {
    earlier_id(
        ports.idempotency(),
        ctx.actor(),
        RECEIVE,
        request,
        ReceiptId::from_uuid,
    )
    .await
}

/// What a replay returns: the receipt and its order as they are now.
async fn replay<S: HasReceipts + HasPurchaseOrders>(
    ports: &S,
    id: ReceiptId,
) -> Result<Idempotent<Received>, CommandError> {
    // Records only point at receipts written in the same batch, which aren't deleted.
    let Some(receipt) = ports.receipts().get(id).await? else {
        return Err(RepoError::Backend(format!(
            "idempotency record points at missing receipt {id}"
        ))
        .into());
    };
    let purchase_order = load_order(ports, receipt.purchase_order()).await?;
    Ok(Idempotent::replayed(Received {
        receipt,
        purchase_order,
        new_items: Vec::new(),
    }))
}
