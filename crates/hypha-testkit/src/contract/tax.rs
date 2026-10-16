use std::collections::HashMap;

use hypha_core::{
    CountryCode, ProductId, RepoError, TaxClassId, TaxRate, TaxRegionId, Version, Versioned,
};
use hypha_tax::{TaxClass, TaxClassRepo, TaxRegion, TaxRegionRepo, repo::keys};
use uuid::Uuid;

use super::scenario;

// ---------- fixtures ----------

fn class_id(n: u128) -> TaxClassId {
    TaxClassId::from_uuid(Uuid::from_u128(n))
}

fn region_id(n: u128) -> TaxRegionId {
    TaxRegionId::from_uuid(Uuid::from_u128(n))
}

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn class(n: u128) -> TaxClass {
    TaxClass::new(class_id(n), format!("Class {n}")).unwrap()
}

fn rate(ppm: u32) -> TaxRate {
    TaxRate::new(ppm).unwrap()
}

fn region(n: u128, country: &str) -> TaxRegion {
    TaxRegion::new(
        region_id(n),
        CountryCode::new(country).unwrap(),
        rate(250_000),
    )
}

// ---------- classes ----------

/// Runs every `TaxClassRepo` scenario, each on a fresh repo from `make`.
pub async fn tax_class_repo<R: TaxClassRepo>(make: impl AsyncFn() -> R) {
    scenario("tax class: insert stores every field at version 1");
    class_round_trips(&make().await).await;
    scenario("tax class: unique ID");
    class_keys(&make().await).await;
    scenario("tax class: products are assigned, moved and queried in batches");
    class_assignments(&make().await).await;
    scenario("tax class: list_all returns every class, oldest first");
    class_list_all(&make().await).await;
}

async fn class_list_all<R: TaxClassRepo>(repo: &R) {
    assert_eq!(repo.list_all().await, Ok(vec![]));
    for n in [2, 1] {
        repo.insert(&mut class(n)).await.unwrap();
    }
    assert_eq!(
        repo.list_all().await,
        Ok(vec![
            repo.get(class_id(1)).await.unwrap().unwrap(),
            repo.get(class_id(2)).await.unwrap().unwrap(),
        ])
    );
}

async fn class_round_trips<R: TaxClassRepo>(repo: &R) {
    let mut c = class(1);
    repo.insert(&mut c).await.unwrap();
    assert_eq!(c.version(), Version::new(1));
    assert_eq!(repo.get(c.id()).await, Ok(Some(c)));
    assert_eq!(repo.get(class_id(99)).await, Ok(None));
}

async fn class_keys<R: TaxClassRepo>(repo: &R) {
    repo.insert(&mut class(1)).await.unwrap();
    assert_eq!(
        repo.insert(&mut class(1)).await,
        Err(RepoError::Duplicate(keys::TAX_CLASS_ID))
    );
}

async fn class_assignments<R: TaxClassRepo>(repo: &R) {
    let mut books = class(1);
    let mut food = class(2);
    repo.insert(&mut books).await.unwrap();
    repo.insert(&mut food).await.unwrap();

    assert_eq!(repo.classes_for(&[pid(1)]).await, Ok(HashMap::new()));

    repo.assign(&[pid(1), pid(2), pid(3)], Some(books.id()))
        .await
        .unwrap();
    // Assigning to another class moves the product.
    repo.assign(&[pid(2)], Some(food.id())).await.unwrap();
    // Back to no class; a product without one is not an error.
    repo.assign(&[pid(3), pid(9)], None).await.unwrap();

    assert_eq!(
        repo.classes_for(&[pid(1), pid(2), pid(3), pid(4)]).await,
        Ok(HashMap::from([(pid(1), books.id()), (pid(2), food.id())]))
    );
    assert_eq!(repo.classes_for(&[]).await, Ok(HashMap::new()));
}

// ---------- regions ----------

/// Runs every `TaxRegionRepo` scenario, each on a fresh repo from `make`.
pub async fn tax_region_repo<R: TaxRegionRepo>(make: impl AsyncFn() -> R) {
    scenario("tax region: insert stores every field at version 1");
    region_round_trips(&make().await).await;
    scenario("tax region: update stores rates and bumps the version");
    region_update(&make().await).await;
    scenario("tax region: unique ID and one region per country");
    region_keys(&make().await).await;
    scenario("tax region: stale and missing updates fail");
    region_stale(&make().await).await;
    scenario("tax region: countries stay unique on update");
    region_country_on_update(&make().await).await;
    scenario("tax region: list_all returns every region with its rates, oldest first");
    region_list_all(&make().await).await;
}

async fn region_list_all<R: TaxRegionRepo>(repo: &R) {
    assert_eq!(repo.list_all().await, Ok(vec![]));
    let mut hr = region(2, "HR");
    hr.set_class_rate(class_id(7), rate(50_000));
    repo.insert(&mut hr).await.unwrap();
    repo.insert(&mut region(1, "DE")).await.unwrap();
    assert_eq!(
        repo.list_all().await,
        Ok(vec![
            repo.get(region_id(1)).await.unwrap().unwrap(),
            repo.get(region_id(2)).await.unwrap().unwrap(),
        ])
    );
}

async fn region_round_trips<R: TaxRegionRepo>(repo: &R) {
    // Class rate order and the 0% and 100% limits must survive.
    let mut r = region(1, "HR");
    r.set_class_rate(class_id(2), rate(50_000));
    r.set_class_rate(class_id(1), TaxRate::ZERO);
    r.set_class_rate(class_id(3), rate(TaxRate::MAX));
    repo.insert(&mut r).await.unwrap();
    assert_eq!(r.version(), Version::new(1));
    assert_eq!(repo.get(r.id()).await, Ok(Some(r.clone())));
    assert_eq!(repo.get_by_country(r.country()).await, Ok(Some(r)));

    assert_eq!(repo.get(region_id(99)).await, Ok(None));
    assert_eq!(
        repo.get_by_country(CountryCode::new("DE").unwrap()).await,
        Ok(None)
    );
}

async fn region_update<R: TaxRegionRepo>(repo: &R) {
    let mut r = region(1, "HR");
    r.set_class_rate(class_id(1), rate(50_000));
    r.set_class_rate(class_id(2), rate(130_000));
    repo.insert(&mut r).await.unwrap();

    r.set_default_rate(rate(200_000));
    r.remove_class_rate(class_id(1)).unwrap();
    r.set_class_rate(class_id(3), rate(0));
    repo.update(&mut r).await.unwrap();

    assert_eq!(r.version(), Version::new(2));
    assert_eq!(repo.get(r.id()).await, Ok(Some(r)));
}

async fn region_keys<R: TaxRegionRepo>(repo: &R) {
    repo.insert(&mut region(1, "HR")).await.unwrap();

    assert_eq!(
        repo.insert(&mut region(1, "DE")).await,
        Err(RepoError::Duplicate(keys::TAX_REGION_ID))
    );
    assert_eq!(
        repo.insert(&mut region(2, "HR")).await,
        Err(RepoError::Duplicate(keys::TAX_REGION_COUNTRY))
    );
    assert_eq!(
        repo.get(region_id(2)).await,
        Ok(None),
        "failed insert stored nothing"
    );
}

async fn region_stale<R: TaxRegionRepo>(repo: &R) {
    let mut r = region(1, "HR");
    repo.insert(&mut r).await.unwrap();
    let mut stale = r.clone();
    r.set_default_rate(rate(130_000));
    repo.update(&mut r).await.unwrap();

    stale.set_class_rate(class_id(1), rate(50_000));
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(r.id()).await, Ok(Some(r)));

    assert_eq!(
        repo.update(&mut region(9, "DE")).await,
        Err(RepoError::StaleVersion)
    );
}

/// Regions don't change country through a command, but a restored snapshot can, and the
/// adapter must refuse a country another region has.
async fn region_country_on_update<R: TaxRegionRepo>(repo: &R) {
    repo.insert(&mut region(1, "HR")).await.unwrap();
    let mut serbia = region(2, "RS");
    repo.insert(&mut serbia).await.unwrap();

    let mut moved = serbia.snapshot();
    moved.country = CountryCode::new("HR").unwrap();
    let mut moved = TaxRegion::restore(moved).unwrap();

    assert_eq!(
        repo.update(&mut moved).await,
        Err(RepoError::Duplicate(keys::TAX_REGION_COUNTRY))
    );
    assert_eq!(repo.get(region_id(2)).await, Ok(Some(serbia)));
}
