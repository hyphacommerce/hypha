use hypha_core::{
    InventoryItemId, LocationId, MarketId, Quantity, RepoError, VariantId, Version, Versioned,
};
use hypha_inventory::{
    InventoryItem, InventoryItemRepo, Location, LocationRepo, MAX_STOCK, ReservationRef, repo::keys,
};
use uuid::Uuid;

use super::scenario;

// ---------- fixtures ----------

fn location_id(n: u128) -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(n))
}

fn item_id(n: u128) -> InventoryItemId {
    InventoryItemId::from_uuid(Uuid::from_u128(n))
}

fn market(n: u128) -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn location(n: u128, markets: &[u128], priority: u32) -> Location {
    Location::new(
        location_id(n),
        format!("Location {n}"),
        markets.iter().map(|m| market(*m)).collect(),
        priority,
    )
    .unwrap()
}

fn item(n: u128, variant: u128) -> InventoryItem {
    InventoryItem::new(item_id(n), vid(variant), true, false)
}

fn reference(s: &str) -> ReservationRef {
    ReservationRef::new(s).unwrap()
}

fn qty(n: u32) -> Quantity {
    Quantity::new(n).unwrap()
}

// ---------- locations ----------

/// Runs every `LocationRepo` scenario, each on a fresh repo from `make`.
pub async fn location_repo<R: LocationRepo>(make: impl AsyncFn() -> R) {
    scenario("location: insert stores every field at version 1");
    location_round_trips(&make().await).await;
    scenario("location: update stores every field and bumps the version");
    location_update(&make().await).await;
    scenario("location: unique ID");
    location_keys(&make().await).await;
    scenario("location: stale and missing updates fail");
    location_stale(&make().await).await;
    scenario("location: serving lists a market's locations by priority, then ID");
    location_serving(&make().await).await;
    scenario("location: an address round trips, every field, and can be removed");
    location_address(&make().await).await;
}

async fn location_round_trips<R: LocationRepo>(repo: &R) {
    // Market order, no markets at all, and the largest priority must survive.
    let mut l = location(1, &[3, 1, 2], u32::MAX);
    repo.insert(&mut l).await.unwrap();
    assert_eq!(l.version(), Version::new(1));
    assert_eq!(repo.get(l.id()).await, Ok(Some(l)));

    let mut empty = location(2, &[], 0);
    repo.insert(&mut empty).await.unwrap();
    assert_eq!(repo.get(empty.id()).await, Ok(Some(empty)));

    assert_eq!(repo.get(location_id(99)).await, Ok(None));
}

async fn location_update<R: LocationRepo>(repo: &R) {
    let mut l = location(1, &[1, 2], 5);
    repo.insert(&mut l).await.unwrap();

    l.set_name("Split".to_string()).unwrap();
    l.set_markets(vec![market(3)]).unwrap();
    l.set_priority(0);
    repo.update(&mut l).await.unwrap();

    assert_eq!(l.version(), Version::new(2));
    assert_eq!(repo.get(l.id()).await, Ok(Some(l)));
    assert_eq!(repo.serving(market(1)).await, Ok(vec![]));
}

async fn location_keys<R: LocationRepo>(repo: &R) {
    repo.insert(&mut location(1, &[1], 0)).await.unwrap();
    assert_eq!(
        repo.insert(&mut location(1, &[2], 0)).await,
        Err(RepoError::Duplicate(keys::LOCATION_ID))
    );
}

async fn location_stale<R: LocationRepo>(repo: &R) {
    let mut l = location(1, &[1], 0);
    repo.insert(&mut l).await.unwrap();
    let mut stale = l.clone();
    l.set_priority(3);
    repo.update(&mut l).await.unwrap();

    stale.set_markets(vec![]).unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(l.id()).await, Ok(Some(l)));

    assert_eq!(
        repo.update(&mut location(9, &[], 0)).await,
        Err(RepoError::StaleVersion)
    );
}

async fn location_serving<R: LocationRepo>(repo: &R) {
    // Inserted out of order; 3 and 4 tie on priority, so the ID decides.
    let mut far = location(1, &[1], 9);
    let mut near = location(2, &[1, 2], 0);
    let mut tie_b = location(4, &[1], 5);
    let mut tie_a = location(3, &[1], 5);
    let mut elsewhere = location(5, &[2], 0);
    for l in [&mut far, &mut near, &mut tie_b, &mut tie_a, &mut elsewhere] {
        repo.insert(l).await.unwrap();
    }

    assert_eq!(
        repo.serving(market(1)).await,
        Ok(vec![near.clone(), tie_a, tie_b, far])
    );
    assert_eq!(repo.serving(market(2)).await, Ok(vec![near, elsewhere]));
    assert_eq!(repo.serving(market(9)).await, Ok(vec![]));
}

// ---------- items ----------

/// Runs every `InventoryItemRepo` scenario, each on a fresh repo from `make`.
pub async fn inventory_item_repo<R: InventoryItemRepo>(make: impl AsyncFn() -> R) {
    scenario("inventory item: insert stores every field at version 1");
    item_round_trips(&make().await).await;
    scenario("inventory item: update stores levels and reservations and bumps the version");
    item_update(&make().await).await;
    scenario("inventory item: unique ID and one item per variant");
    item_keys(&make().await).await;
    scenario("inventory item: stale and missing updates fail");
    item_stale(&make().await).await;
    scenario("inventory item: found by variant, one or many at a time");
    item_by_variant(&make().await).await;
    scenario("inventory item: update_many writes every item, or none if one is stale");
    item_update_many(&make().await).await;
    scenario("inventory item: update_many with the same item twice fails");
    item_update_many_twice(&make().await).await;
    scenario("inventory item: one item per variant on update too");
    item_variant_on_update(&make().await).await;
}

/// Levels at the stock limits and reservations split across locations.
fn stocked_item(n: u128, variant: u128) -> InventoryItem {
    let mut i = item(n, variant);
    i.set_continue_selling(true);
    i.set_on_hand(location_id(2), 3, None).unwrap();
    i.set_on_hand(location_id(1), 1, None).unwrap();
    i.set_on_hand(location_id(3), MAX_STOCK, None).unwrap();
    i.set_on_hand(location_id(4), -MAX_STOCK, None).unwrap();
    // Location 2 has the whole line; then location 1 runs short and continues selling.
    let near = location(2, &[1], 0);
    let small = location(1, &[1], 0);
    i.reserve(&reference("order-1"), qty(3), std::slice::from_ref(&near))
        .unwrap();
    i.reserve(&reference("order-2"), qty(4), &[small]).unwrap();
    i
}

async fn item_round_trips<R: InventoryItemRepo>(repo: &R) {
    let mut i = stocked_item(1, 1);
    repo.insert(&mut i).await.unwrap();
    assert_eq!(i.version(), Version::new(1));
    assert_eq!(repo.get(i.id()).await, Ok(Some(i)));

    let mut untracked = item(2, 2);
    untracked.set_tracked(false);
    repo.insert(&mut untracked).await.unwrap();
    assert_eq!(repo.get(untracked.id()).await, Ok(Some(untracked)));

    assert_eq!(repo.get(item_id(99)).await, Ok(None));
}

async fn item_update<R: InventoryItemRepo>(repo: &R) {
    let mut i = stocked_item(1, 1);
    repo.insert(&mut i).await.unwrap();

    i.set_tracked(false);
    i.set_continue_selling(false);
    i.adjust_on_hand(location_id(2), -2).unwrap();
    i.set_on_hand(location_id(5), 7, None).unwrap();
    i.release(&reference("order-1"));
    repo.update(&mut i).await.unwrap();

    assert_eq!(i.version(), Version::new(2));
    assert_eq!(repo.get(i.id()).await, Ok(Some(i)));
}

async fn item_keys<R: InventoryItemRepo>(repo: &R) {
    repo.insert(&mut item(1, 1)).await.unwrap();
    assert_eq!(
        repo.insert(&mut item(1, 2)).await,
        Err(RepoError::Duplicate(keys::INVENTORY_ITEM_ID))
    );
    assert_eq!(
        repo.insert(&mut item(2, 1)).await,
        Err(RepoError::Duplicate(keys::INVENTORY_ITEM_VARIANT))
    );
    assert_eq!(
        repo.get(item_id(2)).await,
        Ok(None),
        "failed insert stored nothing"
    );
}

async fn item_stale<R: InventoryItemRepo>(repo: &R) {
    let mut i = item(1, 1);
    repo.insert(&mut i).await.unwrap();
    let mut stale = i.clone();
    i.set_on_hand(location_id(1), 5, None).unwrap();
    repo.update(&mut i).await.unwrap();

    stale.set_on_hand(location_id(1), 9, None).unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(i.id()).await, Ok(Some(i)));

    assert_eq!(
        repo.update(&mut item(9, 9)).await,
        Err(RepoError::StaleVersion)
    );
}

async fn item_by_variant<R: InventoryItemRepo>(repo: &R) {
    let mut a = stocked_item(1, 1);
    let mut b = item(2, 2);
    repo.insert(&mut a).await.unwrap();
    repo.insert(&mut b).await.unwrap();

    assert_eq!(repo.get_by_variant(vid(1)).await, Ok(Some(a.clone())));
    assert_eq!(repo.get_by_variant(vid(9)).await, Ok(None));

    let mut found = repo.for_variants(&[vid(2), vid(9), vid(1)]).await.unwrap();
    found.sort_by_key(|i| i.id().as_uuid());
    assert_eq!(found, vec![a, b]);
    assert_eq!(repo.for_variants(&[]).await, Ok(vec![]));
}

async fn item_update_many<R: InventoryItemRepo>(repo: &R) {
    let mut a = item(1, 1);
    let mut b = item(2, 2);
    repo.insert(&mut a).await.unwrap();
    repo.insert(&mut b).await.unwrap();

    // Both written, both versions bumped.
    a.set_on_hand(location_id(1), 1, None).unwrap();
    b.set_on_hand(location_id(1), 2, None).unwrap();
    let mut batch = vec![a, b];
    repo.update_many(&mut batch).await.unwrap();
    assert!(batch.iter().all(|i| i.version() == Version::new(2)));
    for i in &batch {
        assert_eq!(repo.get(i.id()).await, Ok(Some(i.clone())));
    }

    // One stale item: nothing is written, not even the fresh one.
    let [a, b] = <[InventoryItem; 2]>::try_from(batch).unwrap();
    let mut stale_b = b.clone();
    let mut newer_b = b;
    newer_b.set_on_hand(location_id(1), 20, None).unwrap();
    repo.update(&mut newer_b).await.unwrap();

    let mut changed_a = a.clone();
    changed_a.set_on_hand(location_id(1), 10, None).unwrap();
    stale_b.set_on_hand(location_id(1), 30, None).unwrap();
    let mut batch = vec![changed_a, stale_b];
    assert_eq!(
        repo.update_many(&mut batch).await,
        Err(RepoError::StaleVersion)
    );
    assert_eq!(repo.get(a.id()).await, Ok(Some(a.clone())), "a unchanged");
    assert_eq!(repo.get(newer_b.id()).await, Ok(Some(newer_b)));

    // A missing item is stale too; an empty batch is fine.
    let mut batch = vec![a, item(9, 9)];
    assert_eq!(
        repo.update_many(&mut batch).await,
        Err(RepoError::StaleVersion)
    );
    assert_eq!(repo.update_many(&mut []).await, Ok(()));
}

/// Caller precondition (each item at most once): two copies of one item in one
/// `update_many` fail and write nothing, instead of one silently winning.
async fn item_update_many_twice<R: InventoryItemRepo>(repo: &R) {
    let mut stored = item(1, 1);
    repo.insert(&mut stored).await.unwrap();
    let mut first = stored.clone();
    first.set_continue_selling(true);
    let second = stored.clone();

    assert!(repo.update_many(&mut [first, second]).await.is_err());
    assert_eq!(repo.get(stored.id()).await, Ok(Some(stored)));
}

/// Items don't move to another variant through a command, but a restored snapshot can,
/// and the adapter must refuse a variant another item has.
async fn item_variant_on_update<R: InventoryItemRepo>(repo: &R) {
    repo.insert(&mut item(1, 1)).await.unwrap();
    let mut other = item(2, 2);
    repo.insert(&mut other).await.unwrap();

    let mut moved = other.snapshot();
    moved.variant = vid(1);
    let mut moved = InventoryItem::restore(moved).unwrap();

    assert_eq!(
        repo.update(&mut moved).await,
        Err(RepoError::Duplicate(keys::INVENTORY_ITEM_VARIANT))
    );
    assert_eq!(repo.get(item_id(2)).await, Ok(Some(other)));
}

async fn location_address<R: LocationRepo>(repo: &R) {
    let full = hypha_core::Address::new(hypha_core::AddressParts {
        first_name: Some("Ana".to_string()),
        last_name: "Warehouse".to_string(),
        company: Some("Hypha d.o.o.".to_string()),
        line1: "Ilica 1".to_string(),
        line2: Some("Ulaz B".to_string()),
        city: "Zagreb".to_string(),
        province: Some("Grad Zagreb".to_string()),
        postal_code: Some("10000".to_string()),
        country: "HR".to_string(),
        phone: Some("+385 1 234 5678".to_string()),
    })
    .unwrap();
    let mut l = location(1, &[1], 0);
    l.set_address(Some(full));
    repo.insert(&mut l).await.unwrap();
    assert_eq!(repo.get(l.id()).await, Ok(Some(l.clone())));

    // Only the required fields, then none.
    let minimal = hypha_core::Address::new(hypha_core::AddressParts {
        first_name: None,
        last_name: "Store".to_string(),
        company: None,
        line1: "Main St 1".to_string(),
        line2: None,
        city: "Ljubljana".to_string(),
        province: None,
        postal_code: None,
        country: "SI".to_string(),
        phone: None,
    })
    .unwrap();
    l.set_address(Some(minimal));
    repo.update(&mut l).await.unwrap();
    assert_eq!(repo.get(l.id()).await, Ok(Some(l.clone())));
    l.set_address(None);
    repo.update(&mut l).await.unwrap();
    assert_eq!(repo.get(l.id()).await, Ok(Some(l)));
}
