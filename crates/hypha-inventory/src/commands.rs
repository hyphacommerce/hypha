use std::collections::{HashMap, HashSet};

use hypha_core::{
    Address, AddressParts, AuthError, Ctx, ErrorCode, IdGenerator, InventoryItemId, LocationId,
    MarketId, Quantity, RepoError, Scope, VariantId, report_duplicate, retry_on_conflict,
};
use thiserror::Error;

use crate::{
    InventoryError, InventoryItem, Location, ReservationRef,
    repo::{InventoryItemRepo, LocationRepo, keys},
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Inventory(#[from] InventoryError),
    #[error("location {0} not found")]
    LocationNotFound(LocationId),
    #[error("inventory item {0} not found")]
    InventoryItemNotFound(InventoryItemId),
    #[error("variant {variant} already has inventory item {item}")]
    InventoryItemExists {
        variant: VariantId,
        item: InventoryItemId,
    },
    /// Every line that couldn't be reserved, so a buyer sees all of them at once.
    #[error("not enough stock for {} line(s)", lines.len())]
    OutOfStock { lines: Vec<Shortage> },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Inventory(e) => e.code(),
            CommandError::LocationNotFound(_) => "location_not_found",
            CommandError::InventoryItemNotFound(_) => "inventory_item_not_found",
            CommandError::InventoryItemExists { .. } => "inventory_item_exists",
            CommandError::OutOfStock { .. } => "out_of_stock",
        }
    }
}

/// One line that couldn't be reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortage {
    pub variant: VariantId,
    pub requested: u32,
    /// Total available in the market's locations.
    pub available: i64,
}

// ---------- locations ----------

pub struct CreateLocation {
    pub name: String,
    /// The markets whose buyers this location's stock is sold to. Markets are linked by
    /// ID only (a leaf crate can't depend on `hypha-markets`); an unknown one never
    /// matches a cart.
    pub markets: Vec<MarketId>,
    /// Lower goes first when several locations could fill a line.
    pub priority: u32,
}

pub async fn create_location<L: LocationRepo, I: IdGenerator>(
    ctx: &Ctx,
    locations: &L,
    ids: &I,
    input: CreateLocation,
) -> Result<Location, CommandError> {
    ctx.require(Scope::InventoryWrite)?;

    let mut location = Location::new(
        LocationId::from_uuid(ids.new_uuid()),
        input.name,
        input.markets,
        input.priority,
    )?;
    locations.insert(&mut location).await?;
    Ok(location)
}

/// Every location, by priority (then ID).
pub async fn list_locations<L: LocationRepo>(
    ctx: &Ctx,
    locations: &L,
) -> Result<Vec<Location>, CommandError> {
    ctx.require(Scope::InventoryRead)?;
    Ok(locations.list().await?)
}

/// The name a store's first location gets.
pub const DEFAULT_LOCATION_NAME: &str = "Default";

/// Creates a location named `DEFAULT_LOCATION_NAME`, serving no market yet, if the store
/// has none (design note 0018): a fulfilment leaves from a location, so a store that
/// never set up stock can ship too. Hosts call it at startup, as the system; returns the
/// new location, or `None` when there was one already. Two instances starting at once
/// may each create one; the merchant renames or keeps both.
pub async fn ensure_default_location<L: LocationRepo, I: IdGenerator>(
    ctx: &Ctx,
    locations: &L,
    ids: &I,
) -> Result<Option<Location>, CommandError> {
    ctx.require(Scope::InventoryWrite)?;
    if !locations.list().await?.is_empty() {
        return Ok(None);
    }
    let input = CreateLocation {
        name: DEFAULT_LOCATION_NAME.to_string(),
        markets: Vec::new(),
        priority: 0,
    };
    create_location(ctx, locations, ids, input).await.map(Some)
}

pub async fn get_location<L: LocationRepo>(
    ctx: &Ctx,
    locations: &L,
    id: LocationId,
) -> Result<Location, CommandError> {
    ctx.require(Scope::InventoryRead)?;
    load_location(locations, id).await
}

/// Changes only the fields that are `Some`.
#[derive(Default)]
pub struct UpdateLocation {
    pub name: Option<String>,
    pub markets: Option<Vec<MarketId>>,
    pub priority: Option<u32>,
    /// `Some(None)` removes the address.
    pub address: Option<Option<AddressParts>>,
}

pub async fn update_location<L: LocationRepo>(
    ctx: &Ctx,
    locations: &L,
    id: LocationId,
    input: UpdateLocation,
) -> Result<Location, CommandError> {
    ctx.require(Scope::InventoryWrite)?;

    let mut location = load_location(locations, id).await?;
    if let Some(name) = input.name {
        location.set_name(name)?;
    }
    if let Some(markets) = input.markets {
        location.set_markets(markets)?;
    }
    if let Some(priority) = input.priority {
        location.set_priority(priority);
    }
    if let Some(address) = input.address {
        let address = address.map(Address::new).transpose();
        location.set_address(address.map_err(InventoryError::from)?);
    }
    locations.update(&mut location).await?;
    Ok(location)
}

async fn load_location<L: LocationRepo>(
    locations: &L,
    id: LocationId,
) -> Result<Location, CommandError> {
    locations
        .get(id)
        .await?
        .ok_or(CommandError::LocationNotFound(id))
}

// ---------- items ----------

pub struct CreateInventoryItem {
    /// Linked by ID only, like tax classes link to products: the inventory crate can't
    /// check that the variant exists.
    pub variant: VariantId,
    pub tracked: bool,
    pub continue_selling: bool,
}

pub async fn create_inventory_item<R: InventoryItemRepo, I: IdGenerator>(
    ctx: &Ctx,
    items: &R,
    ids: &I,
    input: CreateInventoryItem,
) -> Result<InventoryItem, CommandError> {
    ctx.require(Scope::InventoryWrite)?;

    if let Some(exists) = item_exists(items, input.variant).await? {
        return Err(exists);
    }
    let mut item = InventoryItem::new(
        InventoryItemId::from_uuid(ids.new_uuid()),
        input.variant,
        input.tracked,
        input.continue_selling,
    );
    let written = items.insert(&mut item).await;
    report_duplicate(
        written,
        keys::INVENTORY_ITEM_VARIANT,
        item_exists(items, input.variant),
    )
    .await?;
    Ok(item)
}

/// `InventoryItemExists` naming `variant`'s item, if it has one.
async fn item_exists<R: InventoryItemRepo>(
    items: &R,
    variant: VariantId,
) -> Result<Option<CommandError>, CommandError> {
    Ok(items
        .get_by_variant(variant)
        .await?
        .map(|existing| CommandError::InventoryItemExists {
            variant,
            item: existing.id(),
        }))
}

pub async fn get_inventory_item<R: InventoryItemRepo>(
    ctx: &Ctx,
    items: &R,
    id: InventoryItemId,
) -> Result<InventoryItem, CommandError> {
    ctx.require(Scope::InventoryRead)?;
    load_item(items, id).await
}

/// The variant's item, or `None` if stock was never set up for it, which means
/// untracked (design note 0004, I3).
pub async fn get_variant_inventory<R: InventoryItemRepo>(
    ctx: &Ctx,
    items: &R,
    variant: VariantId,
) -> Result<Option<InventoryItem>, CommandError> {
    ctx.require(Scope::InventoryRead)?;
    Ok(items.get_by_variant(variant).await?)
}

/// The items of these variants, in one read; a variant without one is left out
/// (untracked, as for `get_variant_inventory`).
pub async fn get_variants_inventory<R: InventoryItemRepo>(
    ctx: &Ctx,
    items: &R,
    variants: &[VariantId],
) -> Result<Vec<InventoryItem>, CommandError> {
    ctx.require(Scope::InventoryRead)?;
    Ok(items.for_variants(variants).await?)
}

/// Changes only the fields that are `Some`.
#[derive(Default)]
pub struct UpdateInventoryItem {
    pub tracked: Option<bool>,
    pub continue_selling: Option<bool>,
}

pub async fn update_inventory_item<R: InventoryItemRepo>(
    ctx: &Ctx,
    items: &R,
    id: InventoryItemId,
    input: UpdateInventoryItem,
) -> Result<InventoryItem, CommandError> {
    ctx.require(Scope::InventoryWrite)?;

    let mut item = load_item(items, id).await?;
    if let Some(tracked) = input.tracked {
        item.set_tracked(tracked);
    }
    if let Some(continue_selling) = input.continue_selling {
        item.set_continue_selling(continue_selling);
    }
    items.update(&mut item).await?;
    Ok(item)
}

pub struct SetOnHand {
    pub item: InventoryItemId,
    pub location: LocationId,
    pub quantity: i64,
    /// The count the caller last saw; the change is refused if it moved since (I10).
    pub expected: Option<i64>,
}

/// Sets the count at a location. Retried on a concurrent change (a checkout reserving
/// the same item), re-checking `expected` against fresh data each time.
pub async fn set_on_hand<R: InventoryItemRepo, L: LocationRepo>(
    ctx: &Ctx,
    items: &R,
    locations: &L,
    input: SetOnHand,
) -> Result<InventoryItem, CommandError> {
    ctx.require(Scope::InventoryWrite)?;

    load_location(locations, input.location).await?;
    let input = &input;
    retry_on_conflict(
        move || async move {
            let mut item = load_item(items, input.item).await?;
            item.set_on_hand(input.location, input.quantity, input.expected)?;
            items.update(&mut item).await?;
            Ok(item)
        },
        lost_a_race,
    )
    .await
}

pub struct AdjustOnHand {
    pub item: InventoryItemId,
    pub location: LocationId,
    /// Negative to remove stock.
    pub delta: i64,
}

/// Adds to (or removes from) the count at a location, e.g. a delivery arrived.
/// Retried on a concurrent change; adding is safe to repeat on fresh data.
pub async fn adjust_on_hand<R: InventoryItemRepo, L: LocationRepo>(
    ctx: &Ctx,
    items: &R,
    locations: &L,
    input: AdjustOnHand,
) -> Result<InventoryItem, CommandError> {
    ctx.require(Scope::InventoryWrite)?;

    load_location(locations, input.location).await?;
    let input = &input;
    retry_on_conflict(
        move || async move {
            let mut item = load_item(items, input.item).await?;
            item.adjust_on_hand(input.location, input.delta)?;
            items.update(&mut item).await?;
            Ok(item)
        },
        lost_a_race,
    )
    .await
}

async fn load_item<R: InventoryItemRepo>(
    items: &R,
    id: InventoryItemId,
) -> Result<InventoryItem, CommandError> {
    items
        .get(id)
        .await?
        .ok_or(CommandError::InventoryItemNotFound(id))
}

// ---------- reservations ----------

pub struct ReserveStock {
    /// The buyer's market: only locations serving it supply the stock.
    pub market: MarketId,
    pub lines: Vec<ReserveLine>,
}

pub struct ReserveLine {
    /// Identifies this reservation (later the order line), so a retry is a no-op (I8).
    pub reference: String,
    pub variant: VariantId,
    pub quantity: u32,
}

/// Reserves stock for every line, **all or nothing** (design note 0004, section 3).
///
/// Reads the items and the market's locations without locks, allocates in domain code,
/// then writes every changed item in one `update_many`. If another checkout changed one
/// of them in between, the write fails with `StaleVersion` and the whole attempt runs
/// again on fresh data, up to `MAX_ATTEMPTS` times. A real shortage then shows up as
/// `OutOfStock` (listing every short line), not as a conflict.
///
/// Variants without an item, or with tracking off, need no stock and reserve nothing.
pub async fn reserve_stock<R: InventoryItemRepo, L: LocationRepo>(
    ctx: &Ctx,
    items: &R,
    locations: &L,
    input: ReserveStock,
) -> Result<(), CommandError> {
    ctx.require(Scope::InventoryWrite)?;

    let lines = input
        .lines
        .iter()
        .map(|l| {
            Ok((
                ReservationRef::new(&l.reference)?,
                l.variant,
                Quantity::new(l.quantity)?,
            ))
        })
        .collect::<Result<Vec<_>, InventoryError>>()?;
    reject_duplicate_lines(
        lines
            .iter()
            .map(|(reference, variant, _)| (reference, *variant)),
    )?;
    let variants = &distinct(lines.iter().map(|(_, variant, _)| *variant));
    let lines = &lines;
    let market = input.market;

    retry_on_conflict(
        move || async move {
            let serving = locations.serving(market).await?;
            let mut stock = by_variant(items.for_variants(variants).await?);
            let changed = reserve_lines(&mut stock, &serving, lines)?;
            write_changed(items, &mut stock, &changed).await
        },
        lost_a_race,
    )
    .await
}

/// Reserves every line on `stock` (items by variant) from the `serving` locations, in
/// memory, and returns the variants whose items changed; the caller writes those. A
/// variant without an item is untracked and skipped, and a reference already reserved
/// changes nothing (I8).
///
/// All or nothing for the caller: if any line is short, the result is `OutOfStock`
/// listing every short line, and `stock` may be partly changed, so the caller discards
/// it. `reserve_stock` and checkout both reserve through this.
pub fn reserve_lines(
    stock: &mut HashMap<VariantId, InventoryItem>,
    serving: &[Location],
    lines: &[(ReservationRef, VariantId, Quantity)],
) -> Result<HashSet<VariantId>, CommandError> {
    let mut changed = HashSet::new();
    let mut short = Vec::new();
    for (reference, variant, quantity) in lines {
        let Some(item) = stock.get_mut(variant) else {
            continue;
        };
        match item.reserve(reference, *quantity, serving) {
            Ok(true) => {
                changed.insert(*variant);
            }
            Ok(false) => {}
            Err(InventoryError::OutOfStock {
                requested,
                available,
            }) => short.push(Shortage {
                variant: *variant,
                requested,
                available,
            }),
            Err(e) => return Err(e.into()),
        }
    }
    if !short.is_empty() {
        return Err(CommandError::OutOfStock { lines: short });
    }
    Ok(changed)
}

pub struct ReleaseStock {
    pub lines: Vec<ReleaseLine>,
}

pub struct ReleaseLine {
    pub reference: String,
    pub variant: VariantId,
}

/// Gives back what `reserve_stock` reserved for each line, all or nothing, retried
/// like `reserve_stock`. Releasing something that isn't reserved is not an error, so a
/// retried or duplicate release is harmless. A retired item left with nothing reserved
/// is deleted (design note 0016, Q5), as every path that releases stock does.
pub async fn release_stock<R: InventoryItemRepo>(
    ctx: &Ctx,
    items: &R,
    input: ReleaseStock,
) -> Result<(), CommandError> {
    ctx.require(Scope::InventoryWrite)?;

    let lines = input
        .lines
        .iter()
        .map(|l| Ok((ReservationRef::new(&l.reference)?, l.variant)))
        .collect::<Result<Vec<_>, InventoryError>>()?;
    reject_duplicate_lines(
        lines
            .iter()
            .map(|(reference, variant)| (reference, *variant)),
    )?;
    let variants = &distinct(lines.iter().map(|(_, variant)| *variant));
    let lines = &lines;

    retry_on_conflict(
        move || async move {
            let mut stock = by_variant(items.for_variants(variants).await?);
            let mut changed = HashSet::new();
            for (reference, variant) in lines {
                if let Some(item) = stock.get_mut(variant)
                    && item.release(reference)
                {
                    changed.insert(*variant);
                }
            }
            // The removed variants' items go once spent; their variants are gone, so
            // nothing reserves them again meanwhile.
            let spent: Vec<VariantId> = changed
                .iter()
                .filter(|v| stock.get(v).is_some_and(InventoryItem::is_spent))
                .copied()
                .collect();
            for variant in &spent {
                changed.remove(variant);
            }
            write_changed(items, &mut stock, &changed).await?;
            for variant in &spent {
                if let Some(mut item) = stock.remove(variant) {
                    item.delete()?;
                    items.delete(&mut item).await?;
                }
            }
            Ok(())
        },
        lost_a_race,
    )
    .await
}

/// A request may name each (reference, variant) once: a second line would be silently
/// swallowed by the idempotency path (review plan B5).
fn reject_duplicate_lines<'a>(
    lines: impl Iterator<Item = (&'a ReservationRef, VariantId)>,
) -> Result<(), InventoryError> {
    let mut seen = HashSet::new();
    for (reference, variant) in lines {
        if !seen.insert((reference, variant)) {
            return Err(InventoryError::DuplicateLine {
                reference: reference.clone(),
                variant,
            });
        }
    }
    Ok(())
}

fn distinct(variants: impl Iterator<Item = VariantId>) -> Vec<VariantId> {
    let mut seen = HashSet::new();
    variants.filter(|v| seen.insert(*v)).collect()
}

fn by_variant(items: Vec<InventoryItem>) -> HashMap<VariantId, InventoryItem> {
    items.into_iter().map(|i| (i.variant(), i)).collect()
}

/// One atomic write of the items that changed (none: no write at all).
async fn write_changed<R: InventoryItemRepo>(
    items: &R,
    stock: &mut HashMap<VariantId, InventoryItem>,
    changed: &HashSet<VariantId>,
) -> Result<(), CommandError> {
    let mut write: Vec<InventoryItem> = changed
        .iter()
        .filter_map(|variant| stock.remove(variant))
        .collect();
    if write.is_empty() {
        return Ok(());
    }
    // A stable order, so adapters always touch rows in the same sequence.
    write.sort_by_key(|i| i.id().as_uuid());
    items.update_many(&mut write).await?;
    Ok(())
}

/// A stock write lost a race (design note 0004, section 3): the attempt runs again on
/// fresh data, up to `MAX_ATTEMPTS` times.
fn lost_a_race(e: &CommandError) -> bool {
    matches!(e, CommandError::Repo(RepoError::StaleVersion))
}
