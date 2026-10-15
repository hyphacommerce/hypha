use hypha_core::{
    AssetId, Currency, Money, OptionId, OptionValueId, PageLimit, PageRequest, Price, ProductId,
    RepoError, VariantId, Version, Versioned,
};
use hypha_products::{
    Handle, NewVariant, OptionValue, Product, ProductOption, ProductRepo, ProductStatus,
    VariantPatch, repo::keys,
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
    scenario("product: handles stay unique on update");
    handle_key_on_update(&make().await).await;
    scenario("product: update from a stale copy fails");
    stale_update(&make().await).await;
    scenario("product: update of a missing product is stale");
    missing_update(&make().await).await;
    scenario("product: list pages newest first, after a cursor, by status");
    list_pages(&make().await).await;
    scenario("product: media and variant media round trip, in order");
    media_round_trips(&make().await).await;
    scenario("product: get_by_sku matches the exact SKU");
    sku_lookup(&make().await).await;
    scenario("product: SKUs stay unique across products, on insert and update");
    sku_key(&make().await).await;
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
    .unwrap()
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

/// A shirt with Size (S, M, L) and three variants: S at 15.00 EUR, M at 17.00 EUR and
/// L at 20.00 USD, so its prices span two currencies.
fn shirt_in_two_currencies(n: u128) -> Product {
    let b = n * 100;
    let size = ProductOption::new(
        oid(b + 10),
        "Size".to_string(),
        vec![
            OptionValue::new(val(b + 11), "S".to_string()),
            OptionValue::new(val(b + 12), "M".to_string()),
            OptionValue::new(val(b + 13), "L".to_string()),
        ],
    );
    let variant = |v: u128, price: Price| NewVariant {
        id: vid(b + v),
        sku: None,
        price,
        selections: vec![val(b + 10 + v)],
    };
    let mut p = Product::new_configurable(
        pid(n),
        Handle::new("shirt").unwrap(),
        "Shirt".to_string(),
        vec![size],
        variant(1, eur(1500)),
    )
    .unwrap();
    p.add_variant(variant(2, eur(1700))).unwrap();
    let usd = Price::new(Money::new(2000, Currency::Usd).unwrap()).unwrap();
    p.add_variant(variant(3, usd)).unwrap();
    p
}

fn aid(n: u128) -> AssetId {
    AssetId::from_uuid(Uuid::from_u128(n))
}

fn with_sku(sku: &str) -> VariantPatch {
    VariantPatch {
        sku: Some(Some(sku.to_string())),
        ..VariantPatch::default()
    }
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

/// No command renames a handle yet, but a restored snapshot can carry another one, and
/// the adapter must still refuse to store a taken handle.
async fn handle_key_on_update<R: ProductRepo>(repo: &R) {
    repo.insert(&mut simple(1, "mug")).await.unwrap();
    let mut cup = simple(2, "cup");
    repo.insert(&mut cup).await.unwrap();

    let mut renamed = cup.snapshot();
    renamed.handle = "mug".to_string();
    let mut renamed = Product::restore(renamed).unwrap();

    assert_eq!(
        repo.update(&mut renamed).await,
        Err(RepoError::Duplicate(keys::HANDLE))
    );
    assert_eq!(repo.get(pid(2)).await, Ok(Some(cup)));
}

fn page(after: Option<u128>, limit: u32) -> PageRequest<ProductId> {
    PageRequest {
        after: after.map(pid),
        limit: PageLimit::new(limit).unwrap(),
    }
}

async fn listed<R: ProductRepo>(
    repo: &R,
    status: Option<ProductStatus>,
    after: Option<u128>,
    limit: u32,
) -> (Vec<ProductId>, bool) {
    let page = repo.list(status, page(after, limit)).await.unwrap();
    (page.items.iter().map(|p| p.id).collect(), page.has_more)
}

async fn list_pages<R: ProductRepo>(repo: &R) {
    assert_eq!(listed(repo, None, None, 10).await, (vec![], false));

    // Products 1 to 5, inserted out of order; 2 and 4 active (updated, so version 2).
    // 5 has variants priced in two currencies.
    for n in [3, 1, 5, 2, 4] {
        let mut p = if n == 5 {
            let mut p = shirt_in_two_currencies(n);
            p.set_media(vec![aid(2), aid(1)]).unwrap();
            p
        } else {
            simple(n, &format!("p{n}"))
        };
        repo.insert(&mut p).await.unwrap();
        if n % 2 == 0 {
            p.set_status(ProductStatus::Active).unwrap();
            repo.update(&mut p).await.unwrap();
        }
    }

    // Summaries of the stored products.
    let first = repo.list(None, page(None, 2)).await.unwrap();
    assert_eq!(
        first.items,
        vec![
            repo.get(pid(5)).await.unwrap().unwrap().summary(),
            repo.get(pid(4)).await.unwrap().unwrap().summary(),
        ]
    );
    assert_eq!(
        (
            first.items[0].variant_count,
            first.items[0].price_ranges.len(),
            first.items[0].featured_media,
            first.items[1].featured_media,
        ),
        (3, 2, Some(aid(2)), None)
    );
    assert!(first.has_more);

    assert_eq!(
        listed(repo, None, Some(4), 2).await,
        (vec![pid(3), pid(2)], true)
    );
    assert_eq!(listed(repo, None, Some(2), 2).await, (vec![pid(1)], false));
    // Exactly `limit` left: no more after them.
    assert_eq!(
        listed(repo, None, Some(3), 2).await,
        (vec![pid(2), pid(1)], false)
    );
    assert_eq!(listed(repo, None, Some(1), 2).await, (vec![], false));
    // A cursor needn't be an existing product (it may have been deleted since).
    assert_eq!(listed(repo, None, Some(99), 1).await, (vec![pid(5)], true));

    let active = Some(ProductStatus::Active);
    assert_eq!(
        listed(repo, active, None, 10).await,
        (vec![pid(4), pid(2)], false)
    );
    assert_eq!(listed(repo, active, None, 1).await, (vec![pid(4)], true));
    assert_eq!(
        listed(repo, active, Some(4), 1).await,
        (vec![pid(2)], false)
    );
    assert_eq!(
        listed(repo, Some(ProductStatus::Draft), Some(5), 10).await,
        (vec![pid(3), pid(1)], false)
    );
    assert_eq!(
        listed(repo, Some(ProductStatus::Archived), None, 10).await,
        (vec![], false)
    );
}

async fn media_round_trips<R: ProductRepo>(repo: &R) {
    let mut p = shirt(1, "shirt");
    p.set_media(vec![aid(3), aid(1), aid(2)]).unwrap();
    p.set_variant_media(vid(101), Some(aid(1))).unwrap();
    repo.insert(&mut p).await.unwrap();
    assert_eq!(repo.get(p.id()).await, Ok(Some(p.clone())));

    // Reordered, one item gone (its variant shows nothing now), another variant set.
    p.set_media(vec![aid(2), aid(3)]).unwrap();
    p.set_variant_media(vid(102), Some(aid(3))).unwrap();
    repo.update(&mut p).await.unwrap();
    let stored = repo.get(p.id()).await.unwrap().unwrap();
    assert_eq!(stored.media(), &[aid(2), aid(3)]);
    assert_eq!(stored.variant(vid(101)).unwrap().media(), None);
    assert_eq!(stored, p);

    // The same asset on two products.
    let mut other = simple(2, "mug");
    other.set_media(vec![aid(2)]).unwrap();
    repo.insert(&mut other).await.unwrap();
    assert_eq!(repo.get(other.id()).await, Ok(Some(other)));
}

async fn sku_lookup<R: ProductRepo>(repo: &R) {
    let mut s = shirt(1, "shirt");
    repo.insert(&mut s).await.unwrap();
    repo.insert(&mut simple(2, "mug")).await.unwrap();

    assert_eq!(repo.get_by_sku("SHIRT-S-RED").await, Ok(Some(s)));
    assert_eq!(repo.get_by_sku("shirt-s-red").await, Ok(None));
    assert_eq!(repo.get_by_sku("SHIRT").await, Ok(None));
}

async fn sku_key<R: ProductRepo>(repo: &R) {
    let mut first = shirt(1, "shirt");
    repo.insert(&mut first).await.unwrap();

    // Product 2 inserted with product 1's SKU.
    let mut second = shirt(2, "shirt-2");
    assert_eq!(
        repo.insert(&mut second).await,
        Err(RepoError::Duplicate(keys::VARIANT_SKU))
    );
    second
        .update_variant(vid(201), with_sku("SHIRT-2"))
        .unwrap();
    repo.insert(&mut second).await.unwrap();

    // Product 2 updated to take it.
    let before = second.clone();
    second
        .update_variant(vid(202), with_sku("SHIRT-S-RED"))
        .unwrap();
    assert_eq!(
        repo.update(&mut second).await,
        Err(RepoError::Duplicate(keys::VARIANT_SKU))
    );
    assert_eq!(repo.get(pid(2)).await, Ok(Some(before)));

    // Moving a SKU between a product's own variants in one write is fine.
    first.update_variant(vid(101), with_sku("TEMP")).unwrap();
    first
        .update_variant(vid(102), with_sku("SHIRT-S-RED"))
        .unwrap();
    first
        .update_variant(vid(101), with_sku("SHIRT-S-RED-2"))
        .unwrap();
    repo.update(&mut first).await.unwrap();
    assert_eq!(repo.get(pid(1)).await, Ok(Some(first)));
}
