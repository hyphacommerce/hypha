use hypha_categories::{Category, CategoryRepo, HasCategories, NewCategory};
use hypha_channels::{Channel, ChannelKind, ChannelRepo, HasChannels};
use hypha_core::{
    AssetId, CategoryId, ChannelId, Currency, Keyset, Money, OptionId, OptionValueId, PageLimit,
    PageRequest, Price, ProductId, RepoError, SortKey, VariantId, Version, Versioned,
};
use hypha_products::{
    Handle, HasProducts, NewVariant, OptionValue, Product, ProductFilter, ProductFilterInput,
    ProductOption, ProductRepo, ProductSort, ProductStatus, VariantPatch, repo::keys,
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
    scenario("product: list by text in the title, handle or a SKU, ignoring case");
    list_by_text(&make().await).await;
    scenario("product: every sort orders by its key, then ID, and pages across ties");
    list_sorted(&make().await).await;
    scenario("product: list by several statuses, vendor, product type and tag");
    list_by_details(&make().await).await;
    scenario("product: media and variant media round trip, in order");
    media_round_trips(&make().await).await;
    scenario("product: variant weight and requires_shipping round trip");
    shipping_facts_round_trip(&make().await).await;
    scenario("product: description, vendor, product type and tags round trip");
    content_round_trips(&make().await).await;
    scenario("product: ids_of_variants finds each owner once");
    ids_of_variants(&make().await).await;
    scenario("product: get_by_sku matches the exact SKU");
    sku_lookup(&make().await).await;
    scenario("product: SKUs stay unique across products, on insert and update");
    sku_key(&make().await).await;
}

/// Runs the scenarios of product lists filtered by other domains' membership (design
/// note 0024, AU5): categories and channels, as the adapter stores them.
pub async fn product_list<S>(make: impl AsyncFn() -> S)
where
    S: HasProducts + HasCategories + HasChannels,
{
    scenario("product list: by category and by channel, alone, together, and unknown");
    by_membership(&make().await).await;
}

async fn by_membership<S: HasProducts + HasCategories + HasChannels>(ports: &S) {
    let repo = ports.products();
    for n in 1..=4 {
        repo.insert(&mut simple(n, &format!("p{n}"))).await.unwrap();
    }
    let cat = CategoryId::from_uuid(Uuid::from_u128(50));
    let mut category = Category::new(NewCategory {
        id: cat,
        name: "Kitchen".to_string(),
        handle: Handle::new("kitchen").unwrap(),
        description: None,
        parent: None,
        position: 0,
        visible: true,
    })
    .unwrap();
    ports.categories().insert(&mut category).await.unwrap();
    ports
        .categories()
        .add_products(cat, &[pid(1), pid(3)])
        .await
        .unwrap();
    let chan = ChannelId::from_uuid(Uuid::from_u128(60));
    let mut channel = Channel::new(chan, "Market stall", ChannelKind::Custom).unwrap();
    ports.channels().insert(&mut channel).await.unwrap();
    ports
        .channels()
        .publish(chan, &[pid(3), pid(4)])
        .await
        .unwrap();

    let found = async |category, channel| {
        let filter = ProductFilter::new(ProductFilterInput {
            category,
            channel,
            ..ProductFilterInput::default()
        })
        .unwrap();
        all_pages(repo, &filter, ProductSort::Oldest, 1).await
    };
    assert_eq!(found(Some(cat), None).await, [pid(1), pid(3)]);
    assert_eq!(found(None, Some(chan)).await, [pid(3), pid(4)]);
    assert_eq!(found(Some(cat), Some(chan)).await, [pid(3)]);
    let unknown = CategoryId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        found(Some(unknown), None).await,
        Vec::<ProductId>::new(),
        "an unknown category matches nothing"
    );
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
    Price::new(Money::new(amount, Currency::EUR).unwrap()).unwrap()
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
    let usd = Price::new(Money::new(2000, Currency::USD).unwrap()).unwrap();
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

fn page(after: Option<u128>, limit: u32) -> PageRequest<Keyset<ProductId>> {
    PageRequest {
        after: after.map(|n| Keyset {
            key: SortKey::Id,
            id: pid(n),
        }),
        limit: PageLimit::new(limit).unwrap(),
    }
}

async fn listed<R: ProductRepo>(
    repo: &R,
    status: Option<ProductStatus>,
    after: Option<u128>,
    limit: u32,
) -> (Vec<ProductId>, bool) {
    let filter = ProductFilter::new(ProductFilterInput {
        statuses: status.into_iter().collect(),
        ..ProductFilterInput::default()
    })
    .unwrap();
    let page = repo
        .list(&filter, ProductSort::Newest, page(after, limit))
        .await
        .unwrap();
    (page.items.iter().map(|p| p.id).collect(), page.has_more)
}

async fn list_by_text<R: ProductRepo>(repo: &R) {
    // 1: "Linen Shirt" (shirt), SKU LS-RED; 2: "Mug" (stoneware-mug); 3: "Čaša", SKU
    // 100%_OFF (characters LIKE would treat as wildcards); 4 is active.
    let mut shirt = simple(1, "shirt");
    shirt.rename("Linen Shirt").unwrap();
    shirt
        .update_variant(
            vid(101),
            VariantPatch {
                sku: Some(Some("LS-RED".to_string())),
                ..VariantPatch::default()
            },
        )
        .unwrap();
    let mut mug = simple(2, "stoneware-mug");
    mug.rename("Mug").unwrap();
    let mut glass = simple(3, "glass");
    glass.rename("Čaša").unwrap();
    glass
        .update_variant(
            vid(301),
            VariantPatch {
                sku: Some(Some("100%_OFF".to_string())),
                ..VariantPatch::default()
            },
        )
        .unwrap();
    let mut linen_mug = simple(4, "linen-mug");
    linen_mug.set_status(ProductStatus::Active).unwrap();
    for p in [&mut shirt, &mut mug, &mut glass, &mut linen_mug] {
        repo.insert(p).await.unwrap();
    }

    let found = async |status: Option<ProductStatus>, q: &str| {
        let filter = ProductFilter::new(ProductFilterInput {
            statuses: status.into_iter().collect(),
            q: Some(q.to_string()),
            ..ProductFilterInput::default()
        })
        .unwrap();
        let page = repo
            .list(&filter, ProductSort::Newest, page(None, 10))
            .await
            .unwrap();
        page.items.iter().map(|p| p.id).collect::<Vec<_>>()
    };
    assert_eq!(
        found(None, "LINEN").await,
        [pid(4), pid(1)],
        "title and handle"
    );
    assert_eq!(found(None, "ware-m").await, [pid(2)], "inside the handle");
    assert_eq!(found(None, "ls-red").await, [pid(1)], "a SKU");
    assert_eq!(found(None, "čAŠ").await, [pid(3)], "beyond ASCII");
    assert_eq!(found(None, "0%_").await, [pid(3)], "% and _ are plain text");
    assert_eq!(
        found(None, "_").await,
        [pid(3)],
        "only the SKU with an underscore"
    );
    assert_eq!(found(None, "cup").await, Vec::<ProductId>::new());
    assert_eq!(
        found(Some(ProductStatus::Active), "linen").await,
        [pid(4)],
        "with a status too"
    );
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
    let first = repo
        .list(
            &ProductFilter::default(),
            ProductSort::Newest,
            page(None, 2),
        )
        .await
        .unwrap();
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

/// Every page of `sort`'s list under `filter`, `limit` at a time, each continued from
/// the last item's position.
async fn all_pages<R: ProductRepo>(
    repo: &R,
    filter: &ProductFilter,
    sort: ProductSort,
    limit: u32,
) -> Vec<ProductId> {
    let mut found = Vec::new();
    let mut after = None;
    loop {
        let page = repo
            .list(
                filter,
                sort,
                PageRequest {
                    after: after.clone(),
                    limit: PageLimit::new(limit).unwrap(),
                },
            )
            .await
            .unwrap();
        found.extend(page.items.iter().map(|s| s.id));
        match page.items.last() {
            Some(last) if page.has_more => {
                after = Some(Keyset {
                    key: sort.key(last),
                    id: last.id,
                })
            }
            _ => return found,
        }
    }
}

const SORTS: [ProductSort; 6] = [
    ProductSort::Newest,
    ProductSort::Oldest,
    ProductSort::TitleAsc,
    ProductSort::TitleDesc,
    ProductSort::UpdatedDesc,
    ProductSort::UpdatedAsc,
];

async fn list_sorted<R: ProductRepo>(repo: &R) {
    // Titles that tie once lowercased ("apple", "banana"), one beyond ASCII, inserted
    // out of order; some updated, so their last change differs where the clock moves.
    let titles = [
        (3, "banana"),
        (1, "Apple"),
        (6, "apple"),
        (2, "Čaša"),
        (5, "cherry"),
        (4, "Banana"),
    ];
    for (n, title) in titles {
        let mut p = simple(n, &format!("p{n}"));
        p.rename(title).unwrap();
        repo.insert(&mut p).await.unwrap();
    }
    for n in [2, 6] {
        let mut p = repo.get(pid(n)).await.unwrap().unwrap();
        p.set_status(ProductStatus::Active).unwrap();
        repo.update(&mut p).await.unwrap();
    }

    let mut summaries = Vec::new();
    for (n, _) in titles {
        summaries.push(repo.get(pid(n)).await.unwrap().unwrap().summary());
    }
    let every = ProductFilter::default();
    for sort in SORTS {
        let mut expected = summaries.clone();
        expected.sort_by(|a, b| {
            Keyset::order(
                (&sort.key(a), &a.id),
                (&sort.key(b), &b.id),
                sort.direction(),
            )
        });
        let expected: Vec<ProductId> = expected.iter().map(|s| s.id).collect();
        for limit in [1, 2, 4, 10] {
            assert_eq!(
                all_pages(repo, &every, sort, limit).await,
                expected,
                "{sort:?} by {limit}"
            );
        }
    }
    // Spelled out for two, so the rule itself is pinned, not only agreement with `key`.
    assert_eq!(
        all_pages(repo, &every, ProductSort::TitleAsc, 2).await,
        [pid(1), pid(6), pid(3), pid(4), pid(5), pid(2)],
        "ties by ID; Č after the ASCII letters"
    );
    assert_eq!(
        all_pages(repo, &every, ProductSort::TitleDesc, 4).await,
        [pid(2), pid(5), pid(4), pid(3), pid(6), pid(1)]
    );

    // Filters apply to sorted lists too.
    let active = ProductFilter::new(ProductFilterInput {
        statuses: vec![ProductStatus::Active],
        ..ProductFilterInput::default()
    })
    .unwrap();
    assert_eq!(
        all_pages(repo, &active, ProductSort::TitleAsc, 1).await,
        [pid(6), pid(2)]
    );

    // A position from another sort is refused, not misread.
    assert!(matches!(
        repo.list(
            &every,
            ProductSort::TitleAsc,
            PageRequest {
                after: Some(Keyset {
                    key: SortKey::Id,
                    id: pid(1)
                }),
                limit: PageLimit::new(2).unwrap(),
            },
        )
        .await,
        Err(RepoError::Backend(_))
    ));
}

async fn list_by_details<R: ProductRepo>(repo: &R) {
    // 1: Acme mug tagged Gift; 2: acme mug, active; 3: archived, Other vendor, tags
    // gift-card and Kitchen; 4: no details.
    let details = [
        (1, Some("Acme"), Some("Mug"), vec!["Gift", "Kitchen"]),
        (2, Some("ACME"), Some("mug"), vec![]),
        (3, Some("Other"), Some("Bowl"), vec!["gift-card", "kitchen"]),
        (4, None, None, vec![]),
    ];
    for (n, vendor, product_type, tags) in details {
        let mut p = simple(n, &format!("p{n}"));
        p.set_vendor(vendor.map(str::to_string)).unwrap();
        p.set_product_type(product_type.map(str::to_string))
            .unwrap();
        p.set_tags(tags.into_iter().map(str::to_string).collect())
            .unwrap();
        match n {
            2 => p.set_status(ProductStatus::Active).unwrap(),
            3 => {
                p.set_status(ProductStatus::Active).unwrap();
                p.set_status(ProductStatus::Archived).unwrap();
            }
            _ => {}
        }
        repo.insert(&mut p).await.unwrap();
    }
    let found = async |input: ProductFilterInput| {
        let filter = ProductFilter::new(input).unwrap();
        all_pages(repo, &filter, ProductSort::Oldest, 10).await
    };
    let none = ProductFilterInput::default;
    let text = |t: &str| Some(t.to_string());
    assert_eq!(
        found(ProductFilterInput {
            statuses: vec![ProductStatus::Active, ProductStatus::Archived],
            ..none()
        })
        .await,
        [pid(2), pid(3)]
    );
    assert_eq!(
        found(ProductFilterInput {
            vendor: text("acme"),
            ..none()
        })
        .await,
        [pid(1), pid(2)],
        "vendor, ignoring case"
    );
    assert_eq!(
        found(ProductFilterInput {
            vendor: text("Ac"),
            ..none()
        })
        .await,
        Vec::<ProductId>::new(),
        "whole, not a part"
    );
    assert_eq!(
        found(ProductFilterInput {
            product_type: text("MUG"),
            statuses: vec![ProductStatus::Draft],
            ..none()
        })
        .await,
        [pid(1)]
    );
    assert_eq!(
        found(ProductFilterInput {
            tag: text("KITCHEN"),
            ..none()
        })
        .await,
        [pid(1), pid(3)]
    );
    assert_eq!(
        found(ProductFilterInput {
            tag: text("gift"),
            ..none()
        })
        .await,
        [pid(1)],
        "a whole tag, not gift-card"
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

async fn ids_of_variants<R: ProductRepo>(repo: &R) {
    // Shirt 1 has variants 101 and 102; product 2 has 201.
    repo.insert(&mut shirt(1, "shirt")).await.unwrap();
    repo.insert(&mut simple(2, "mug")).await.unwrap();
    repo.insert(&mut simple(3, "cup")).await.unwrap();
    let mut found = repo
        .ids_of_variants(&[vid(101), vid(102), vid(201), vid(999)])
        .await
        .unwrap();
    found.sort();
    assert_eq!(found, [pid(1), pid(2)]);
    assert_eq!(repo.ids_of_variants(&[]).await, Ok(vec![]));
}

async fn content_round_trips<R: ProductRepo>(repo: &R) {
    let mut p = shirt(1, "shirt");
    p.set_description(Some("<p>Soft <em>linen</em></p>"))
        .unwrap();
    p.set_vendor(Some("Acme".to_string())).unwrap();
    p.set_product_type(Some("Shirt".to_string())).unwrap();
    // Order is the merchant's, not alphabetical.
    p.set_tags(vec!["Summer".to_string(), "Linen".to_string()])
        .unwrap();
    repo.insert(&mut p).await.unwrap();
    assert_eq!(repo.get(p.id()).await, Ok(Some(p.clone())));
    let listed = repo
        .list(
            &ProductFilter::default(),
            ProductSort::Newest,
            page(None, 10),
        )
        .await
        .unwrap();
    assert_eq!(listed.items, vec![p.summary()]);

    p.set_description(None).unwrap();
    p.set_vendor(None).unwrap();
    p.set_tags(vec![]).unwrap();
    repo.update(&mut p).await.unwrap();
    assert_eq!(repo.get(p.id()).await, Ok(Some(p.clone())));
    let listed = repo
        .list(
            &ProductFilter::default(),
            ProductSort::Newest,
            page(None, 10),
        )
        .await
        .unwrap();
    assert_eq!(listed.items, vec![p.summary()]);
}

async fn shipping_facts_round_trip<R: ProductRepo>(repo: &R) {
    let mut p = shirt(1, "shirt");
    p.update_variant(
        vid(101),
        VariantPatch {
            weight: Some(Some(hypha_core::Weight::from_grams(u32::MAX))),
            ..VariantPatch::default()
        },
    )
    .unwrap();
    p.update_variant(
        vid(102),
        VariantPatch {
            weight: Some(Some(hypha_core::Weight::ZERO)),
            requires_shipping: Some(false),
            ..VariantPatch::default()
        },
    )
    .unwrap();
    repo.insert(&mut p).await.unwrap();
    assert_eq!(repo.get(p.id()).await, Ok(Some(p.clone())));

    p.update_variant(
        vid(101),
        VariantPatch {
            weight: Some(None),
            ..VariantPatch::default()
        },
    )
    .unwrap();
    repo.update(&mut p).await.unwrap();
    assert_eq!(repo.get(p.id()).await, Ok(Some(p)));
}
