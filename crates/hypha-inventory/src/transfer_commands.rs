//! Transfers: drafting one between two locations, shipping it whole, receiving it in
//! parts. Shipping and receiving write the transfer and the stock it moves in one batch.

use std::collections::HashMap;

use hypha_core::{
    Attempts, Clock, Ctx, HasBatch, HasIdempotency, IdGenerator, IdempotencyRecord,
    IdempotencyRequest, Idempotent, LocationId, Page, PageRequest, Quantity, RepoError, Scope,
    StageIdempotency, Timestamp, TransferId, VariantId, earlier_id,
};

use crate::{
    CommandError, InventoryError, InventoryItem, StockSource, Transfer, TransferArrival,
    TransferDraft, TransferFilter, TransferLineDraft, TransferSummary,
    repo::{
        HasInventoryItems, HasLocations, HasTransfers, InventoryItemRepo, LocationRepo,
        StageInventoryItems, StageTransfers, TransferRepo,
    },
};

/// The operation names shipping and receiving's idempotency records carry.
pub const SHIP_TRANSFER: &str = "ship_transfer";
pub const RECEIVE_TRANSFER: &str = "receive_transfer";

/// What shipping and receiving read and write, and a batch that stages all of it.
pub trait TransferPorts:
    HasTransfers
    + HasInventoryItems
    + HasIdempotency
    + HasBatch<Batch: StageTransfers + StageInventoryItems + StageIdempotency>
{
}

impl<T> TransferPorts for T where
    T: HasTransfers
        + HasInventoryItems
        + HasIdempotency
        + HasBatch<Batch: StageTransfers + StageInventoryItems + StageIdempotency>
        + ?Sized
{
}

/// A new transfer, as a draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateTransfer {
    pub origin: LocationId,
    pub destination: LocationId,
    pub expected_at: Option<Timestamp>,
    pub reference: Option<String>,
    pub note: Option<String>,
    pub lines: Vec<TransferLineDraft>,
}

/// Drafts a transfer from `origin` to `destination`, both existing locations
/// (`location_not_found`), each line a variant with a stock record
/// (`variant_not_stocked`: there's nothing at the origin to move otherwise).
pub async fn create_transfer<S: HasTransfers + HasLocations + HasInventoryItems>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    input: CreateTransfer,
) -> Result<Transfer, CommandError> {
    ctx.require(Scope::InventoryWrite)?;
    let draft = TransferDraft {
        origin: input.origin,
        destination: input.destination,
        expected_at: input.expected_at,
        reference: input.reference,
        note: input.note,
        lines: input.lines,
    };
    draft.check()?;
    check_draft(ports, &draft).await?;
    // Numbers are never handed out twice, so only a draft that will be written takes one.
    let number = ports.transfers().next_number().await?;
    let mut transfer = Transfer::new(TransferId::from_uuid(ids.new_uuid()), number, draft)?;
    ports.transfers().insert(&mut transfer).await?;
    Ok(transfer)
}

/// Changes only the fields that are `Some`; `Some(None)` clears an optional one.
/// `lines`, when given, replaces them all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateTransfer {
    pub origin: Option<LocationId>,
    pub destination: Option<LocationId>,
    pub expected_at: Option<Option<Timestamp>>,
    pub reference: Option<Option<String>>,
    pub note: Option<Option<String>>,
    pub lines: Option<Vec<TransferLineDraft>>,
}

/// Changes a draft (`transfer_not_draft` once shipped), with `create_transfer`'s rules.
pub async fn update_transfer<S: HasTransfers + HasLocations + HasInventoryItems>(
    ctx: &Ctx,
    ports: &S,
    id: TransferId,
    input: UpdateTransfer,
) -> Result<Transfer, CommandError> {
    ctx.require(Scope::InventoryWrite)?;
    let mut transfer = load(ports, id).await?;
    let mut draft = transfer.draft();
    if let Some(origin) = input.origin {
        draft.origin = origin;
    }
    if let Some(destination) = input.destination {
        draft.destination = destination;
    }
    if let Some(expected_at) = input.expected_at {
        draft.expected_at = expected_at;
    }
    if let Some(reference) = input.reference {
        draft.reference = reference;
    }
    if let Some(note) = input.note {
        draft.note = note;
    }
    if let Some(lines) = input.lines {
        draft.lines = lines;
    }
    // The state first: a shipped transfer is refused as such, whatever the input.
    transfer.ensure_draft()?;
    check_draft(ports, &draft).await?;
    transfer.update(draft)?;
    ports.transfers().update(&mut transfer).await?;
    Ok(transfer)
}

/// The locations exist and every line's variant has a stock record.
async fn check_draft<S: HasLocations + HasInventoryItems>(
    ports: &S,
    draft: &TransferDraft,
) -> Result<(), CommandError> {
    for location in [draft.origin, draft.destination] {
        if ports.locations().get(location).await?.is_none() {
            return Err(CommandError::LocationNotFound(location));
        }
    }
    let variants: Vec<VariantId> = draft.lines.iter().map(|l| l.variant).collect();
    let stocked: Vec<VariantId> = ports
        .inventory_items()
        .for_variants(&variants)
        .await?
        .iter()
        .map(InventoryItem::variant)
        .collect();
    if let Some(missing) = variants.iter().find(|v| !stocked.contains(v)) {
        return Err(InventoryError::VariantNotStocked(*missing).into());
    }
    Ok(())
}

pub async fn get_transfer<S: HasTransfers>(
    ctx: &Ctx,
    ports: &S,
    id: TransferId,
) -> Result<Transfer, CommandError> {
    ctx.require(Scope::InventoryRead)?;
    load(ports, id).await
}

/// A page of transfers, newest first.
pub async fn list_transfers<S: HasTransfers>(
    ctx: &Ctx,
    ports: &S,
    filter: &TransferFilter,
    page: PageRequest<TransferId>,
) -> Result<Page<TransferSummary>, CommandError> {
    ctx.require(Scope::InventoryRead)?;
    Ok(ports.transfers().list(filter, page).await?)
}

/// Deletes a draft (`transfer_not_draft` once shipped or cancelled).
pub async fn delete_transfer<S: HasTransfers>(
    ctx: &Ctx,
    ports: &S,
    id: TransferId,
) -> Result<(), CommandError> {
    ctx.require(Scope::InventoryWrite)?;
    let mut transfer = load(ports, id).await?;
    transfer.ensure_draft()?;
    ports.transfers().delete(&mut transfer).await?;
    Ok(())
}

/// Calls off a draft (`transfer_not_draft` once shipped).
pub async fn cancel_transfer<S: HasTransfers>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: TransferId,
) -> Result<Transfer, CommandError> {
    ctx.require(Scope::InventoryWrite)?;
    let mut transfer = load(ports, id).await?;
    transfer.cancel(clock.now())?;
    ports.transfers().update(&mut transfer).await?;
    Ok(transfer)
}

/// Ships a draft whole: every line's units leave the origin (its count may go below
/// zero, as fulfilling can), each a `transfer_out` row, written with the transfer and the
/// idempotency key in one batch. A variant whose stock record is gone is
/// `variant_not_stocked`. When another request changed the transfer or an item in
/// between, it runs again on fresh data, up to `MAX_ATTEMPTS` times.
pub async fn ship_transfer<S: TransferPorts>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: TransferId,
    idempotency: Option<IdempotencyRequest>,
) -> Result<Idempotent<Transfer>, CommandError> {
    ctx.require(Scope::InventoryWrite)?;
    let key = idempotency.as_ref();
    if let Some(earlier) = replay(ctx, ports, SHIP_TRANSFER, key).await? {
        return Ok(earlier);
    }
    let mut attempts = Attempts::first();
    loop {
        let e = match ship_once(ctx, ports, clock, id, key).await {
            Ok(transfer) => return Ok(Idempotent::fresh(transfer)),
            Err(e) => e,
        };
        // A request with the same key may have committed meanwhile, failing this one.
        if let Some(earlier) = replay(ctx, ports, SHIP_TRANSFER, key).await? {
            return Ok(earlier);
        }
        if !(lost_a_race(&e) && attempts.another()) {
            return Err(e);
        }
    }
}

async fn ship_once<S: TransferPorts>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: TransferId,
    idempotency: Option<&IdempotencyRequest>,
) -> Result<Transfer, CommandError> {
    let mut transfer = load(ports, id).await?;
    let now = clock.now();
    transfer.ship(now)?;
    let source = StockSource::Transfer(id);
    let variants: Vec<VariantId> = transfer.lines().iter().map(|l| l.variant()).collect();
    let mut items = items_of(ports, &variants).await?;
    for line in transfer.lines() {
        let item = items
            .get_mut(&line.variant())
            .ok_or(InventoryError::VariantNotStocked(line.variant()))?;
        item.transfer_out(transfer.origin(), units(line.shipped())?, source)?;
    }
    commit(
        ctx,
        ports,
        &mut transfer,
        items,
        SHIP_TRANSFER,
        idempotency,
        now,
    )
    .await?;
    Ok(transfer)
}

/// Records units arriving at the destination (`Transfer::receive`'s rules): accepted
/// units are a `transfer_in` row there, rejected ones (lost on the way) a `shrinkage` row
/// with no delta. One batch with the transfer and the idempotency key, retried like
/// `ship_transfer`. A variant whose stock record was removed since shipping has nothing
/// to put its units on: the transfer still counts them.
pub async fn receive_transfer<S: TransferPorts>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: TransferId,
    arrivals: &[TransferArrival],
    idempotency: Option<IdempotencyRequest>,
) -> Result<Idempotent<Transfer>, CommandError> {
    ctx.require(Scope::InventoryWrite)?;
    let key = idempotency.as_ref();
    if let Some(earlier) = replay(ctx, ports, RECEIVE_TRANSFER, key).await? {
        return Ok(earlier);
    }
    let mut attempts = Attempts::first();
    loop {
        let e = match receive_once(ctx, ports, clock, id, arrivals, key).await {
            Ok(transfer) => return Ok(Idempotent::fresh(transfer)),
            Err(e) => e,
        };
        // A request with the same key may have committed meanwhile, failing this one.
        if let Some(earlier) = replay(ctx, ports, RECEIVE_TRANSFER, key).await? {
            return Ok(earlier);
        }
        if !(lost_a_race(&e) && attempts.another()) {
            return Err(e);
        }
    }
}

async fn receive_once<S: TransferPorts>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: TransferId,
    arrivals: &[TransferArrival],
    idempotency: Option<&IdempotencyRequest>,
) -> Result<Transfer, CommandError> {
    let mut transfer = load(ports, id).await?;
    let now = clock.now();
    transfer.receive(arrivals, now)?;
    let source = StockSource::Transfer(id);
    let destination = transfer.destination();
    let variants: Vec<VariantId> = arrivals.iter().map(|a| a.variant).collect();
    let mut items = items_of(ports, &variants).await?;
    for arrival in arrivals {
        let Some(item) = items.get_mut(&arrival.variant) else {
            continue;
        };
        if arrival.accepted > 0 {
            item.transfer_in(destination, units(arrival.accepted)?, source)?;
        }
        if arrival.rejected > 0 {
            item.lose_in_transit(destination, units(arrival.rejected)?, source);
        }
    }
    commit(
        ctx,
        ports,
        &mut transfer,
        items,
        RECEIVE_TRANSFER,
        idempotency,
        now,
    )
    .await?;
    Ok(transfer)
}

/// The transfer a request with this key answered before, as it is now.
async fn replay<S: HasIdempotency + HasTransfers>(
    ctx: &Ctx,
    ports: &S,
    operation: &str,
    key: Option<&IdempotencyRequest>,
) -> Result<Option<Idempotent<Transfer>>, CommandError> {
    let earlier: Option<TransferId> = earlier_id::<_, _, CommandError>(
        ports.idempotency(),
        ctx.actor(),
        operation,
        key,
        TransferId::from_uuid,
    )
    .await?;
    match earlier {
        Some(id) => Ok(Some(Idempotent::replayed(load(ports, id).await?))),
        None => Ok(None),
    }
}

/// Lost a race: the transfer or an item changed since it was read.
fn lost_a_race(e: &CommandError) -> bool {
    matches!(e, CommandError::Repo(RepoError::StaleVersion))
}

/// The transfer, the items it moved and the idempotency key, in one batch.
async fn commit<S: TransferPorts>(
    ctx: &Ctx,
    ports: &S,
    transfer: &mut Transfer,
    mut items: HashMap<VariantId, InventoryItem>,
    operation: &str,
    idempotency: Option<&IdempotencyRequest>,
    now: Timestamp,
) -> Result<(), CommandError> {
    let mut batch = ports.batch();
    batch.update_transfer(transfer);
    for item in items.values_mut() {
        batch.update_inventory_item(item);
    }
    if let Some(request) = idempotency {
        let record = IdempotencyRecord::new(
            ctx.actor(),
            request,
            operation,
            transfer.id().as_uuid(),
            now,
        )?;
        batch.record_idempotency(record);
    }
    ports.commit(batch).await?;
    Ok(())
}

async fn items_of<S: HasInventoryItems>(
    ports: &S,
    variants: &[VariantId],
) -> Result<HashMap<VariantId, InventoryItem>, CommandError> {
    Ok(ports
        .inventory_items()
        .for_variants(variants)
        .await?
        .into_iter()
        .map(|item| (item.variant(), item))
        .collect())
}

fn units(n: u32) -> Result<Quantity, CommandError> {
    Ok(Quantity::new(n).map_err(InventoryError::from)?)
}

async fn load<S: HasTransfers>(ports: &S, id: TransferId) -> Result<Transfer, CommandError> {
    ports
        .transfers()
        .get(id)
        .await?
        .ok_or(CommandError::TransferNotFound(id))
}
