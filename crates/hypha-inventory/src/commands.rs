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
/// has none: a fulfilment leaves from a location, so a store that never set up stock can
/// ship too. Hosts call it at startup, as the system; returns the new location, or `None`
/// when there was one already. Two instances starting at once may each create one; the
/// merchant renames or keeps both.
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

/// The variant's item, or `None` if stock was never set up for it, which means untracked.
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
    /// The count the caller last saw; the change is refused if it moved since.
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

/// Reserves every line on `stock` (items by variant) from the `serving` locations, in
/// memory, and returns the variants whose items changed; the caller writes those. A variant
/// without an item is untracked and skipped, and a reference already reserved changes
/// nothing.
///
/// All or nothing for the caller: if any line is short, the result is `OutOfStock`
/// listing every short line, and `stock` may be partly changed, so the caller discards
/// it. Checkout reserves through this, in its own batch.
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

/// A stock write lost a race: the attempt runs again on fresh data, up to `MAX_ATTEMPTS`
/// times.
fn lost_a_race(e: &CommandError) -> bool {
    matches!(e, CommandError::Repo(RepoError::StaleVersion))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn market() -> MarketId {
        MarketId::from_uuid(Uuid::from_u128(1))
    }

    fn loc(n: u128) -> LocationId {
        LocationId::from_uuid(Uuid::from_u128(100 + n))
    }

    fn vid(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(200 + n))
    }

    /// Location 1 serves the market; location 3, which serves another, is left out.
    fn serving() -> Vec<Location> {
        vec![Location::new(loc(1), "Zagreb".to_string(), vec![market()], 0).unwrap()]
    }

    /// Tracked items, each variant's stock as (location, on hand).
    fn stock(items: &[(u128, &[(u128, i64)])]) -> HashMap<VariantId, InventoryItem> {
        items
            .iter()
            .map(|(n, levels)| {
                let id = InventoryItemId::from_uuid(Uuid::from_u128(300 + n));
                let mut item = InventoryItem::new(id, vid(*n), true, false);
                for (location, on_hand) in *levels {
                    item.set_on_hand(loc(*location), *on_hand, None).unwrap();
                }
                (vid(*n), item)
            })
            .collect()
    }

    fn line(
        reference: &str,
        variant: u128,
        quantity: u32,
    ) -> (ReservationRef, VariantId, Quantity) {
        (
            ReservationRef::new(reference).unwrap(),
            vid(variant),
            Quantity::new(quantity).unwrap(),
        )
    }

    fn shortage(variant: u128, requested: u32, available: i64) -> Shortage {
        Shortage {
            variant: vid(variant),
            requested,
            available,
        }
    }

    #[test]
    fn every_short_line_is_reported() {
        let mut items = stock(&[(1, &[(1, 5)]), (2, &[(1, 1)]), (3, &[])]);
        assert_eq!(
            reserve_lines(
                &mut items,
                &serving(),
                &[line("ol-1", 1, 2), line("ol-2", 2, 3), line("ol-3", 3, 1)]
            ),
            Err(CommandError::OutOfStock {
                lines: vec![shortage(2, 3, 1), shortage(3, 1, 0)]
            })
        );
    }

    #[test]
    fn two_lines_of_one_variant_share_its_stock() {
        let mut items = stock(&[(1, &[(1, 3)])]);
        assert_eq!(
            reserve_lines(
                &mut items,
                &serving(),
                &[line("ol-1", 1, 2), line("ol-2", 1, 2)]
            ),
            Err(CommandError::OutOfStock {
                lines: vec![shortage(1, 2, 1)]
            })
        );
    }

    #[test]
    fn variants_without_tracked_stock_need_nothing() {
        let mut items = stock(&[(1, &[])]);
        items.get_mut(&vid(1)).unwrap().set_tracked(false);
        // Variant 1 is untracked, variant 9 has no item at all.
        assert_eq!(
            reserve_lines(
                &mut items,
                &serving(),
                &[line("ol-1", 1, 100), line("ol-2", 9, 100)]
            ),
            Ok(HashSet::new())
        );
    }

    #[test]
    fn reserving_again_changes_nothing() {
        let mut items = stock(&[(1, &[(1, 2)])]);
        let lines = [line("ol-1", 1, 2)];
        assert_eq!(
            reserve_lines(&mut items, &serving(), &lines),
            Ok(HashSet::from([vid(1)]))
        );
        assert_eq!(
            reserve_lines(&mut items, &serving(), &lines),
            Ok(HashSet::new())
        );
        assert_eq!(items[&vid(1)].reservations().len(), 1);
    }

    #[test]
    fn stock_in_another_market_does_not_count() {
        let mut items = stock(&[(1, &[(1, 1), (3, 50)])]);
        assert_eq!(
            reserve_lines(&mut items, &serving(), &[line("ol-1", 1, 2)]),
            Err(CommandError::OutOfStock {
                lines: vec![shortage(1, 2, 1)]
            })
        );
    }
}
