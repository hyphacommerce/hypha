//! Asset usages (design note 0014, section 6): written in a batch with their owner,
//! replaced wholesale per owner, and a reference the storage enforces both ways.

use hypha_core::{
    AssetId, Currency, HasBatch, Money, PageLimit, PageRequest, Price, ProductId, RepoError,
    VariantId, Version, Versioned,
};
use hypha_media::{
    AssetRepo, AssetUsage, AssetUsageRepo, HasAssetUsages, HasAssets, MimeType, StageAssetUsages,
    UsageOwner, UsageRole, repo::keys,
};
use hypha_products::{Handle, HasProducts, Product, ProductRepo, StageProducts};
use uuid::Uuid;

use super::{
    media::{aid, ready},
    scenario,
};

/// Runs every usage scenario, each on fresh ports from `make`.
pub async fn asset_usages<S>(make: impl AsyncFn() -> S)
where
    S: HasBatch + HasAssets + HasAssetUsages + HasProducts,
    S::Batch: StageAssetUsages + StageProducts,
{
    scenario("usages: replacing makes exactly these the owner's usages");
    replaces_wholesale(&make().await).await;
    scenario("usages: pages by owner, last first, after a cursor");
    pages(&make().await).await;
    scenario("usages: a missing asset fails the commit and writes nothing");
    missing_asset_writes_nothing(&make().await).await;
    scenario("usages: an asset listed twice for one owner fails the commit");
    listed_twice_writes_nothing(&make().await).await;
    scenario("usages: an asset in use can't be deleted");
    used_asset_stays(&make().await).await;
    scenario("usages: a stale product writes no usages");
    stale_owner_writes_no_usages(&make().await).await;
}

fn product(n: u128) -> Product {
    Product::new_simple(
        ProductId::from_uuid(Uuid::from_u128(n)),
        Handle::new(&format!("p{n}")).unwrap(),
        "Product".to_string(),
        VariantId::from_uuid(Uuid::from_u128(n * 100)),
        Price::new(Money::new(1000, Currency::EUR).unwrap()).unwrap(),
    )
    .unwrap()
}

fn owner(n: u128) -> UsageOwner {
    UsageOwner::product(ProductId::from_uuid(Uuid::from_u128(n)))
}

fn usage(asset: u128, owner: UsageOwner, role: UsageRole) -> AssetUsage {
    AssetUsage {
        asset: aid(asset),
        owner,
        role,
    }
}

async fn insert_assets<S: HasAssets>(ports: &S, ids: &[u128]) {
    for n in ids {
        ports
            .assets()
            .insert(&mut ready(*n, MimeType::Png))
            .await
            .unwrap();
    }
}

async fn replace<S>(
    ports: &S,
    owner: &UsageOwner,
    usages: &[(AssetId, UsageRole)],
) -> Result<(), RepoError>
where
    S: HasBatch,
    S::Batch: StageAssetUsages,
{
    let mut batch = ports.batch();
    batch.replace_asset_usages(owner, usages);
    ports.commit(batch).await
}

async fn usages_of<S: HasAssetUsages>(ports: &S, asset: u128) -> Vec<AssetUsage> {
    let page = PageRequest {
        after: None,
        limit: PageLimit::new(100).unwrap(),
    };
    ports
        .asset_usages()
        .list_usages(aid(asset), page)
        .await
        .unwrap()
        .items
}

async fn replaces_wholesale<S>(ports: &S)
where
    S: HasBatch + HasAssets + HasAssetUsages,
    S::Batch: StageAssetUsages,
{
    insert_assets(ports, &[1, 2, 3]).await;
    let p1 = owner(1);
    replace(
        ports,
        &p1,
        &[
            (aid(1), UsageRole::featured()),
            (aid(2), UsageRole::gallery()),
        ],
    )
    .await
    .unwrap();
    // Another owner's usages are untouched by p1's.
    replace(ports, &owner(2), &[(aid(1), UsageRole::featured())])
        .await
        .unwrap();
    assert_eq!(
        usages_of(ports, 1).await,
        vec![
            usage(1, owner(2), UsageRole::featured()),
            usage(1, p1.clone(), UsageRole::featured())
        ]
    );

    replace(
        ports,
        &p1,
        &[
            (aid(2), UsageRole::featured()),
            (aid(3), UsageRole::gallery()),
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        usages_of(ports, 1).await,
        vec![usage(1, owner(2), UsageRole::featured())]
    );
    assert_eq!(
        usages_of(ports, 2).await,
        vec![usage(2, p1.clone(), UsageRole::featured())]
    );
    assert_eq!(
        usages_of(ports, 3).await,
        vec![usage(3, p1.clone(), UsageRole::gallery())]
    );

    replace(ports, &p1, &[]).await.unwrap();
    assert_eq!(usages_of(ports, 2).await, vec![]);
}

async fn pages<S>(ports: &S)
where
    S: HasBatch + HasAssets + HasAssetUsages,
    S::Batch: StageAssetUsages,
{
    insert_assets(ports, &[1]).await;
    let spore = UsageOwner::new("spore_lookbook", "lb_1").unwrap();
    for o in [owner(2), spore.clone(), owner(3), owner(1)] {
        replace(ports, &o, &[(aid(1), UsageRole::gallery())])
            .await
            .unwrap();
    }
    let listed = async |after: Option<UsageOwner>, limit| {
        let page = PageRequest {
            after,
            limit: PageLimit::new(limit).unwrap(),
        };
        let page = ports
            .asset_usages()
            .list_usages(aid(1), page)
            .await
            .unwrap();
        let owners: Vec<UsageOwner> = page.items.into_iter().map(|u| u.owner).collect();
        (owners, page.has_more)
    };
    assert_eq!(listed(None, 2).await, (vec![spore.clone(), owner(3)], true));
    assert_eq!(
        listed(Some(owner(3)), 2).await,
        (vec![owner(2), owner(1)], false)
    );
    assert_eq!(listed(Some(owner(1)), 2).await, (vec![], false));
    // Another asset's usages are its own.
    assert_eq!(usages_of(ports, 2).await, vec![]);
}

async fn missing_asset_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasAssets + HasAssetUsages + HasProducts,
    S::Batch: StageAssetUsages + StageProducts,
{
    insert_assets(ports, &[1]).await;
    let mut p = product(1);
    ports.products().insert(&mut p).await.unwrap();
    let stored = p.clone();

    p.set_media(vec![aid(1), aid(9)]).unwrap();
    let mut batch = ports.batch();
    batch.update_product(&mut p);
    batch.replace_asset_usages(
        &owner(1),
        &[
            (aid(1), UsageRole::featured()),
            (aid(9), UsageRole::gallery()),
        ],
    );
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Reference(keys::USAGE_ASSET))
    );
    assert_eq!(ports.products().get(stored.id()).await, Ok(Some(stored)));
    assert_eq!(usages_of(ports, 1).await, vec![]);
}

async fn listed_twice_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasAssets + HasAssetUsages,
    S::Batch: StageAssetUsages,
{
    insert_assets(ports, &[1]).await;
    let twice = [
        (aid(1), UsageRole::featured()),
        (aid(1), UsageRole::gallery()),
    ];
    assert!(replace(ports, &owner(1), &twice).await.is_err());
    assert_eq!(usages_of(ports, 1).await, vec![]);
}

async fn used_asset_stays<S>(ports: &S)
where
    S: HasBatch + HasAssets + HasAssetUsages,
    S::Batch: StageAssetUsages,
{
    insert_assets(ports, &[1]).await;
    replace(ports, &owner(1), &[(aid(1), UsageRole::featured())])
        .await
        .unwrap();
    assert_eq!(
        ports.assets().delete(aid(1)).await,
        Err(RepoError::Reference(keys::USAGE_ASSET))
    );
    assert!(ports.assets().get(aid(1)).await.unwrap().is_some());

    replace(ports, &owner(1), &[]).await.unwrap();
    assert_eq!(ports.assets().delete(aid(1)).await, Ok(()));
    assert_eq!(ports.assets().get(aid(1)).await, Ok(None));
}

async fn stale_owner_writes_no_usages<S>(ports: &S)
where
    S: HasBatch + HasAssets + HasAssetUsages + HasProducts,
    S::Batch: StageAssetUsages + StageProducts,
{
    insert_assets(ports, &[1]).await;
    let mut p = product(1);
    ports.products().insert(&mut p).await.unwrap();
    let mut stale = p.clone();
    p.rename("Renamed").unwrap();
    ports.products().update(&mut p).await.unwrap();

    stale.set_media(vec![aid(1)]).unwrap();
    let mut batch = ports.batch();
    batch.update_product(&mut stale);
    batch.replace_asset_usages(&owner(1), &[(aid(1), UsageRole::featured())]);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(usages_of(ports, 1).await, vec![]);

    // The fresh product commits with its usages, at the version staging promised.
    p.set_media(vec![aid(1)]).unwrap();
    let mut batch = ports.batch();
    batch.update_product(&mut p);
    batch.replace_asset_usages(&owner(1), &[(aid(1), UsageRole::featured())]);
    ports.commit(batch).await.unwrap();
    assert_eq!(p.version(), Version::new(3));
    assert_eq!(ports.products().get(p.id()).await, Ok(Some(p)));
    assert_eq!(
        usages_of(ports, 1).await,
        vec![usage(1, owner(1), UsageRole::featured())]
    );
}
