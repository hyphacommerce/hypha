use hypha_core::{
    Currency, Money, OptionId, OptionValueId, Price, ProductId, RepoError, VariantId, Version,
    Versioned,
};
use hypha_products::{
    Handle, NewVariant, OptionValue, Product, ProductOption, ProductRepo, ProductStatus, repo::keys,
};
use uuid::Uuid;

use super::scenario;

/// Runs every `ProductRepo` scenario, each on a fresh repo from `make`.
pub async fn product_repo<R: ProductRepo>(make: impl AsyncFn() -> R) {
    scenario("product: insert stores every field at version 1");
    insert_round_trips(&make().await).await;
    scenario("product: lookups of unknown products find nothing");
    unknown_lookups(&make().await).await;
    scenario("product: get_many, get_by_handle and get_by_variant");
    lookups(&make().await).await;
    scenario("product: update stores changes and bumps the version");
    update_round_trips(&make().await).await;
    scenario("product: unique keys on insert");
    unique_keys_on_insert(&make().await).await;
    scenario("product: variant IDs stay unique on update");
    variant_key_on_update(&make().await).await;
    scenario("product: update from a stale copy fails");
    stale_update(&make().await).await;
    scenario("product: update of a missing product is stale");
    missing_update(&make().await).await;
}

// ---------- fixtures ----------

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn oid(n: u128) -> OptionId {
    OptionId::from_uuid(Uuid::from_u128(n))
}

fn val(n: u128) -> OptionValueId {
    OptionValueId::from_uuid(Uuid::from_u128(n))
}

fn eur(amount: i64) -> Price {
    Price::new(Money::new(amount, Currency::Eur).unwrap()).unwrap()
}

/// Simple product `n` with the given variant ID.
fn simple_with_variant(n: u128, handle: &str, variant: VariantId) -> Product {
    Product::new_simple(
        pid(n),
        Handle::new(handle).unwrap(),
        format!("Product {n}"),
        variant,
        eur(1000),
    )
}

/// Simple product `n`; its variant is `n * 100 + 1`.
fn simple(n: u128, handle: &str) -> Product {
    simple_with_variant(n, handle, vid(n * 100 + 1))
}

/// Configurable product `n` with Size (S, M) and Color (Red, Blue) and two variants:
/// `n*100+1` S/Red with a SKU, `n*100+2` M/Blue without one.
fn shirt(n: u128, handle: &str) -> Product {
    let b = n * 100;
    let size = ProductOption::new(
        oid(b + 10),
        "Size".to_string(),
        vec![
            OptionValue::new(val(b + 11), "S".to_string()),
            OptionValue::new(val(b + 12), "M".to_string()),
        ],
    );
    let color = ProductOption::new(
        oid(b + 20),
        "Color".to_string(),
        vec![
            OptionValue::new(val(b + 21), "Red".to_string()),
            OptionValue::new(val(b + 22), "Blue".to_string()),
        ],
    );
    let mut p = Product::new_configurable(
        pid(n),
        Handle::new(handle).unwrap(),
        "Shirt".to_string(),
        vec![size, color],
        NewVariant {
            id: vid(b + 1),
            sku: Some("SHIRT-S-RED".to_string()),
            price: eur(1500),
            selections: vec![val(b + 11), val(b + 21)],
        },
    )
    .unwrap();
    p.add_variant(NewVariant {
        id: vid(b + 2),
        sku: None,
        price: eur(1700),
        selections: vec![val(b + 12), val(b + 22)],
    })
    .unwrap();
    p
}

fn sorted(mut products: Vec<Product>) -> Vec<Product> {
    products.sort_by_key(|p| p.id().as_uuid());
    products
}

// ---------- scenarios ----------

async fn insert_round_trips<R: ProductRepo>(repo: &R) {
    for mut product in [simple(1, "mug"), shirt(2, "shirt")] {
        repo.insert(&mut product).await.unwrap();
        assert_eq!(product.version(), Version::new(1));
        assert_eq!(repo.get(product.id()).await, Ok(Some(product)));
    }
}

async fn unknown_lookups<R: ProductRepo>(repo: &R) {
    assert_eq!(repo.get(pid(99)).await, Ok(None));
    assert_eq!(repo.get_many(&[]).await, Ok(vec![]));
    assert_eq!(repo.get_many(&[pid(99)]).await, Ok(vec![]));
    assert_eq!(
        repo.get_by_handle(&Handle::new("nope").unwrap()).await,
        Ok(None)
    );
    assert_eq!(repo.get_by_variant(vid(9999)).await, Ok(None));
}

async fn lookups<R: ProductRepo>(repo: &R) {
    let mut mug = simple(1, "mug");
    let mut shirt = shirt(2, "shirt");
    repo.insert(&mut mug).await.unwrap();
    repo.insert(&mut shirt).await.unwrap();

    let many = repo
        .get_many(&[shirt.id(), pid(99), mug.id()])
        .await
        .unwrap();
    assert_eq!(sorted(many), sorted(vec![mug.clone(), shirt.clone()]));
    assert_eq!(
        repo.get_by_handle(&Handle::new("shirt").unwrap()).await,
        Ok(Some(shirt.clone()))
    );
    // The shirt's second variant, not just its first.
    assert_eq!(repo.get_by_variant(vid(202)).await, Ok(Some(shirt)));
    assert_eq!(repo.get_by_variant(vid(101)).await, Ok(Some(mug)));
}

async fn update_round_trips<R: ProductRepo>(repo: &R) {
    let mut p = shirt(1, "shirt");
    repo.insert(&mut p).await.unwrap();

    p.add_variant(NewVariant {
        id: vid(103),
        sku: Some("SHIRT-S-BLUE".to_string()),
        price: eur(1600),
        selections: vec![val(111), val(122)],
    })
    .unwrap();
    p.set_status(ProductStatus::Active).unwrap();
    repo.update(&mut p).await.unwrap();

    assert_eq!(p.version(), Version::new(2));
    assert_eq!(repo.get(p.id()).await, Ok(Some(p)));
}

async fn unique_keys_on_insert<R: ProductRepo>(repo: &R) {
    repo.insert(&mut simple(1, "mug")).await.unwrap();

    let mut same_id = simple_with_variant(1, "other", vid(901));
    assert_eq!(
        repo.insert(&mut same_id).await,
        Err(RepoError::Duplicate(keys::PRODUCT_ID))
    );

    assert_eq!(
        repo.insert(&mut simple(2, "mug")).await,
        Err(RepoError::Duplicate(keys::HANDLE))
    );
    assert_eq!(
        repo.get(pid(2)).await,
        Ok(None),
        "failed insert stored nothing"
    );

    // Product 3 reuses product 1's variant ID.
    assert_eq!(
        repo.insert(&mut simple_with_variant(3, "cup", vid(101)))
            .await,
        Err(RepoError::Duplicate(keys::VARIANT_ID))
    );
    assert_eq!(
        repo.get(pid(3)).await,
        Ok(None),
        "failed insert stored nothing"
    );
}

async fn variant_key_on_update<R: ProductRepo>(repo: &R) {
    repo.insert(&mut simple(1, "mug")).await.unwrap();
    let mut shirt = shirt(2, "shirt");
    repo.insert(&mut shirt).await.unwrap();
    let before = shirt.clone();

    // Add a variant whose ID the mug already uses.
    shirt
        .add_variant(NewVariant {
            id: vid(101),
            sku: None,
            price: eur(1600),
            selections: vec![val(211), val(222)],
        })
        .unwrap();

    assert_eq!(
        repo.update(&mut shirt).await,
        Err(RepoError::Duplicate(keys::VARIANT_ID))
    );
    assert_eq!(
        repo.get(pid(2)).await,
        Ok(Some(before)),
        "failed update stored nothing"
    );
}

async fn stale_update<R: ProductRepo>(repo: &R) {
    let mut p = simple(1, "mug");
    repo.insert(&mut p).await.unwrap();
    let mut first = p.clone();
    let mut second = p.clone();

    first.set_status(ProductStatus::Active).unwrap();
    repo.update(&mut first).await.unwrap();
    second.set_status(ProductStatus::Active).unwrap();

    assert_eq!(repo.update(&mut second).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(p.id()).await, Ok(Some(first)));
}

async fn missing_update<R: ProductRepo>(repo: &R) {
    assert_eq!(
        repo.update(&mut simple(1, "mug")).await,
        Err(RepoError::StaleVersion)
    );
}
