//! Deleting products and assets: the row goes with its event in one write, and what only
//! names a deleted product goes with it.

use hypha_categories::{Category, CategoryRepo, HasCategories, NewCategory};
use hypha_channels::{Channel, ChannelKind, ChannelRepo, HasChannels};
use hypha_core::{
    CatalogId, CategoryId, ChannelId, Currency, HasBatch, HasOutbox, MarketId, Money, OutboxRepo,
    Price, ProductId, RepoError, StoredEvent, TaxClassId, VariantId,
};
use hypha_markets::{Catalog, CatalogContext, CatalogRepo, HasCatalogs};
use hypha_media::{
    AssetRepo, AssetUsageRepo, HasAssetUsages, HasAssets, MimeType, StageAssetUsages, UsageOwner,
    UsageRole,
};
use hypha_products::{Handle, HasProducts, Product, ProductRepo, StageProducts};
use hypha_tax::{HasTaxClasses, TaxClass, TaxClassRepo};
use uuid::Uuid;

use super::{
    media::{aid, ready},
    scenario,
};

/// Runs every deletion scenario, each on fresh ports from `make`.
pub async fn deletions<S>(make: impl AsyncFn() -> S)
where
    S: HasBatch
        + HasProducts
        + HasCatalogs
        + HasChannels
        + HasCategories
        + HasTaxClasses
        + HasAssets
        + HasAssetUsages
        + HasOutbox,
    S::Batch: StageProducts + StageAssetUsages,
{
    scenario("deletions: a product goes with its children, membership and tax class");
    deletes_a_product(&make().await).await;
    scenario("deletions: a stale or missing product deletes nothing");
    stale_product_deletes_nothing(&make().await).await;
    scenario("deletions: an asset goes with its event; a missing one writes none");
    deletes_an_asset(&make().await).await;
}

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn product(n: u128, handle: &str) -> Product {
    Product::new_simple(
        pid(n),
        Handle::new(handle).unwrap(),
        format!("Product {n}"),
        vid(n * 100 + 1),
        Price::new(Money::new(1000, Currency::EUR).unwrap()).unwrap(),
    )
    .unwrap()
}

async fn events<S: HasOutbox>(ports: &S) -> Vec<StoredEvent> {
    ports.outbox().events_after(0, 100).await.unwrap()
}

async fn deletes_a_product<S>(ports: &S)
where
    S: HasBatch
        + HasProducts
        + HasCatalogs
        + HasChannels
        + HasCategories
        + HasTaxClasses
        + HasAssets
        + HasOutbox,
    S::Batch: StageProducts + StageAssetUsages,
{
    let mut mug = product(1, "mug");
    mug.set_media(vec![aid(1)]).unwrap();
    ports
        .assets()
        .insert(&mut ready(1, MimeType::Png))
        .await
        .unwrap();
    ports.products().insert(&mut mug).await.unwrap();
    let mut cup = product(2, "cup");
    ports.products().insert(&mut cup).await.unwrap();
    let both = [pid(1), pid(2)];

    let catalog = CatalogId::from_uuid(Uuid::from_u128(10));
    let market = MarketId::from_uuid(Uuid::from_u128(11));
    let mut c = Catalog::new(catalog, CatalogContext::Market(market), None);
    ports.catalogs().insert(&mut c).await.unwrap();
    ports.catalogs().add_products(catalog, &both).await.unwrap();
    let channel = ChannelId::from_uuid(Uuid::from_u128(12));
    let mut ch = Channel::new(channel, "Shop", ChannelKind::Custom).unwrap();
    ports.channels().insert(&mut ch).await.unwrap();
    ports.channels().publish(channel, &both).await.unwrap();
    let category = CategoryId::from_uuid(Uuid::from_u128(13));
    let mut cat = Category::new(NewCategory {
        id: category,
        name: "Kitchen".to_string(),
        handle: Handle::new("kitchen").unwrap(),
        description: None,
        parent: None,
        position: 0,
        visible: true,
    })
    .unwrap();
    ports.categories().insert(&mut cat).await.unwrap();
    ports
        .categories()
        .add_products(category, &both)
        .await
        .unwrap();
    let class = TaxClassId::from_uuid(Uuid::from_u128(14));
    let mut tc = TaxClass::new(class, "Books".to_string()).unwrap();
    ports.tax_classes().insert(&mut tc).await.unwrap();
    ports
        .tax_classes()
        .assign(&both, Some(class))
        .await
        .unwrap();
    let before = events(ports).await.len();

    let mut stored = ports.products().get(pid(1)).await.unwrap().unwrap();
    stored.delete();
    let mut batch = ports.batch();
    batch.delete_product(&mut stored);
    batch.replace_asset_usages(&UsageOwner::product(pid(1)), &[]);
    assert_eq!(ports.commit(batch).await, Ok(()));

    assert_eq!(ports.products().get(pid(1)).await, Ok(None));
    assert_eq!(ports.products().get_by_variant(vid(101)).await, Ok(None));
    assert_eq!(ports.products().get(pid(2)).await, Ok(Some(cup)));
    let only_cup = std::collections::HashSet::from([pid(2)]);
    assert_eq!(
        ports.catalogs().included_products(catalog, &both).await,
        Ok(only_cup.clone())
    );
    assert_eq!(
        ports.channels().published(channel, &both).await,
        Ok(only_cup)
    );
    assert_eq!(
        ports.categories().products_in(&[category]).await,
        Ok(vec![pid(2)])
    );
    let classes = ports.tax_classes().classes_for(&both).await.unwrap();
    assert_eq!(
        classes.into_iter().collect::<Vec<_>>(),
        [(pid(2), class)],
        "the deleted product's tax class goes with it"
    );
    assert!(
        ports.assets().get(aid(1)).await.unwrap().is_some(),
        "its assets stay"
    );

    let events = events(ports).await;
    let deleted = &events[before..];
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0].event_type, "product.deleted");
    assert_eq!(
        (deleted[0].subject_kind.as_str(), deleted[0].subject_id),
        ("product", pid(1).as_uuid())
    );
    assert_eq!(deleted[0].data["variants"], serde_json::json!([vid(101)]));
}

async fn stale_product_deletes_nothing<S>(ports: &S)
where
    S: HasBatch + HasProducts + HasAssets + HasAssetUsages + HasOutbox,
    S::Batch: StageProducts + StageAssetUsages,
{
    ports
        .assets()
        .insert(&mut ready(1, MimeType::Png))
        .await
        .unwrap();
    let mut mug = product(1, "mug");
    mug.set_media(vec![aid(1)]).unwrap();
    ports.products().insert(&mut mug).await.unwrap();
    let mut batch = ports.batch();
    batch.replace_asset_usages(
        &UsageOwner::product(pid(1)),
        &[(aid(1), UsageRole::featured())],
    );
    ports.commit(batch).await.unwrap();
    let mut stale = mug.clone();
    mug.rename("Big mug").unwrap();
    ports.products().update(&mut mug).await.unwrap();
    let before = events(ports).await.len();

    stale.delete();
    let mut batch = ports.batch();
    batch.delete_product(&mut stale);
    batch.replace_asset_usages(&UsageOwner::product(pid(1)), &[]);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.products().get(pid(1)).await, Ok(Some(mug.clone())));
    let first = hypha_core::PageRequest {
        after: None,
        limit: hypha_core::PageLimit::new(10).unwrap(),
    };
    let usages = ports
        .asset_usages()
        .list_usages(aid(1), first)
        .await
        .unwrap();
    assert_eq!(usages.items.len(), 1, "the usages stay too");
    assert_eq!(events(ports).await.len(), before, "and no event is written");

    // Already gone: deleting it again is stale too.
    let mut current = mug.clone();
    current.delete();
    let mut batch = ports.batch();
    batch.delete_product(&mut current);
    ports.commit(batch).await.unwrap();
    let mut again = mug;
    again.delete();
    let mut batch = ports.batch();
    batch.delete_product(&mut again);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
}

async fn deletes_an_asset<S: HasAssets + HasOutbox>(ports: &S) {
    ports
        .assets()
        .insert(&mut ready(1, MimeType::Png))
        .await
        .unwrap();
    ports
        .assets()
        .insert(&mut ready(2, MimeType::Png))
        .await
        .unwrap();
    let before = events(ports).await.len();
    let mut asset = ports.assets().get(aid(1)).await.unwrap().unwrap();
    let mut gone = asset.clone();
    asset.delete();
    assert_eq!(ports.assets().delete(&mut asset).await, Ok(()));
    assert_eq!(ports.assets().get(aid(1)).await, Ok(None));
    assert!(ports.assets().get(aid(2)).await.unwrap().is_some());
    let written = events(ports).await;
    let written = &written[before..];
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].event_type, "asset.deleted");
    assert_eq!(
        (written[0].subject_kind.as_str(), written[0].subject_id),
        ("asset", aid(1).as_uuid())
    );
    assert_eq!(written[0].data, serde_json::json!({ "asset": aid(1) }));

    // Gone already: fine, and no second event.
    gone.delete();
    assert_eq!(ports.assets().delete(&mut gone).await, Ok(()));
    assert_eq!(events(ports).await.len(), before + 1);
}
