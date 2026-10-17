use std::collections::HashMap;

use hypha_categories::{Category, CategoryRepo, HasCategories, NewCategory, StageCategories, keys};
use hypha_core::{
    AssetId, CategoryId, Description, Handle, HasBatch, Page, PageLimit, PageRequest, ProductId,
    RepoError, Version, Versioned,
};
use uuid::Uuid;

use super::scenario;

fn cid(n: u128) -> CategoryId {
    CategoryId::from_uuid(Uuid::from_u128(n))
}

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn category(n: u128, parent: Option<u128>, position: u32) -> Category {
    Category::new(NewCategory {
        id: cid(n),
        name: format!("Category {n}"),
        handle: Handle::new(&format!("category-{n}")).unwrap(),
        description: None,
        parent: parent.map(cid),
        position,
        visible: true,
    })
    .unwrap()
}

fn page(after: Option<ProductId>, limit: u32) -> PageRequest<ProductId> {
    PageRequest {
        after,
        limit: PageLimit::new(limit).unwrap(),
    }
}

/// Runs every `CategoryRepo` scenario, each on fresh ports from `make`.
pub async fn category_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasCategories + HasBatch<Batch: StageCategories>,
{
    scenario("category: insert stores every field at version 1; list_all has them all");
    round_trip(&make().await).await;
    scenario("category: update stores changes and bumps the version; stale copies fail");
    update(&make().await).await;
    scenario("category: handles are unique, on insert and update");
    unique_handles(&make().await).await;
    scenario("category: a parent must exist");
    parent_exists(&make().await).await;
    scenario("category: membership is added, paged, listed and read in batches");
    membership(&make().await).await;
    scenario("category: adding products to a missing category fails");
    missing_category(&make().await).await;
    scenario("category: staged writes commit together, or not at all");
    staged(&make().await).await;
    scenario("category: deleting removes the membership, and fails while it has children");
    deleting(&make().await).await;
}

async fn round_trip<S: HasCategories>(ports: &S) {
    let repo = ports.categories();
    assert_eq!(repo.list_all().await, Ok(vec![]));
    let mut clothing = category(1, None, 0);
    clothing.set_description(Description::new("<p>Ženska odjeća &amp; more</p>").unwrap());
    clothing.set_image(Some(AssetId::from_uuid(Uuid::from_u128(7))));
    clothing.set_visible(false);
    let mut shirts = category(2, Some(1), 3);
    repo.insert(&mut clothing).await.unwrap();
    repo.insert(&mut shirts).await.unwrap();
    assert_eq!(clothing.version(), Version::new(1));
    let stored = repo.get(cid(1)).await.unwrap().unwrap();
    assert_eq!(stored, clothing);
    assert!(hypha_core::Stamped::stamps(&stored).is_some());
    assert_eq!(
        repo.get_by_handle("category-2").await,
        Ok(Some(shirts.clone()))
    );
    assert_eq!(repo.get_by_handle("nope").await, Ok(None));
    assert_eq!(repo.get(cid(9)).await, Ok(None));
    let mut all = repo.list_all().await.unwrap();
    all.sort_by_key(Category::id);
    assert_eq!(all, vec![clothing, shirts]);
}

async fn update<S: HasCategories>(ports: &S) {
    let repo = ports.categories();
    let mut c = category(1, None, 0);
    repo.insert(&mut c).await.unwrap();
    let stale = c.clone();
    c.rename("Renamed").unwrap();
    c.set_handle(Handle::new("renamed").unwrap());
    c.set_description(Description::new("<p>New</p>").unwrap());
    repo.update(&mut c).await.unwrap();
    assert_eq!(c.version(), Version::new(2));
    assert_eq!(repo.get(cid(1)).await, Ok(Some(c.clone())));
    assert_eq!(repo.get_by_handle("category-1").await, Ok(None));
    let mut stale = stale;
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    let mut missing = category(9, None, 0);
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

async fn unique_handles<S: HasCategories>(ports: &S) {
    let repo = ports.categories();
    repo.insert(&mut category(1, None, 0)).await.unwrap();
    assert_eq!(
        repo.insert(&mut category(1, None, 1)).await,
        Err(RepoError::Duplicate(keys::CATEGORY_ID))
    );
    let mut same_handle = category(2, None, 1);
    same_handle.set_handle(Handle::new("category-1").unwrap());
    assert_eq!(
        repo.insert(&mut same_handle).await,
        Err(RepoError::Duplicate(keys::CATEGORY_HANDLE))
    );
    let mut other = category(3, None, 1);
    repo.insert(&mut other).await.unwrap();
    other.set_handle(Handle::new("category-1").unwrap());
    assert_eq!(
        repo.update(&mut other).await,
        Err(RepoError::Duplicate(keys::CATEGORY_HANDLE))
    );
}

async fn parent_exists<S: HasCategories>(ports: &S) {
    let repo = ports.categories();
    assert!(repo.insert(&mut category(2, Some(1), 0)).await.is_err());
    assert_eq!(repo.get(cid(2)).await, Ok(None));
}

async fn membership<S: HasCategories>(ports: &S) {
    let repo = ports.categories();
    repo.insert(&mut category(1, None, 0)).await.unwrap();
    repo.insert(&mut category(2, None, 1)).await.unwrap();
    repo.add_products(cid(1), &[pid(1), pid(3), pid(2)])
        .await
        .unwrap();
    // Adding again is not an error.
    repo.add_products(cid(1), &[pid(3)]).await.unwrap();
    repo.add_products(cid(2), &[pid(3), pid(4)]).await.unwrap();

    assert_eq!(
        repo.product_page(cid(1), page(None, 2)).await,
        Ok(Page {
            items: vec![pid(3), pid(2)],
            has_more: true
        })
    );
    assert_eq!(
        repo.product_page(cid(1), page(Some(pid(2)), 2))
            .await
            .unwrap()
            .items,
        vec![pid(1)]
    );
    let mut inside = repo.products_in(&[cid(1), cid(2)]).await.unwrap();
    inside.sort();
    assert_eq!(inside, vec![pid(1), pid(2), pid(3), pid(4)]);
    assert_eq!(repo.products_in(&[]).await, Ok(vec![]));
    assert_eq!(
        repo.categories_of(&[pid(3), pid(1), pid(9)]).await,
        Ok(HashMap::from([
            (pid(3), vec![cid(1), cid(2)]),
            (pid(1), vec![cid(1)])
        ]))
    );
}

async fn missing_category<S: HasCategories>(ports: &S) {
    assert!(
        ports
            .categories()
            .add_products(cid(9), &[pid(1)])
            .await
            .is_err()
    );
}

async fn staged<S: HasCategories + HasBatch<Batch: StageCategories>>(ports: &S) {
    let mut parent = category(1, None, 0);
    let mut child = category(2, Some(1), 0);
    let mut batch = ports.batch();
    batch.insert_category(&mut parent);
    batch.insert_category(&mut child);
    batch.add_category_products(cid(2), &[pid(1), pid(2)]);
    ports.commit(batch).await.unwrap();
    assert_eq!(child.version(), Version::new(1));
    assert_eq!(
        ports.categories().get(cid(2)).await,
        Ok(Some(child.clone()))
    );

    // A move and a membership change, then a stale write: nothing of it stays.
    let stale = parent.clone();
    child.set_visible(false);
    let mut batch = ports.batch();
    batch.update_category(&mut child);
    batch.remove_category_products(cid(2), &[pid(1)]);
    ports.commit(batch).await.unwrap();
    assert_eq!(child.version(), Version::new(2));
    let mut renamed = child.clone();
    renamed.rename("Lost").unwrap();
    let mut batch = ports.batch();
    batch.update_category(&mut renamed);
    batch.add_category_products(cid(2), &[pid(9)]);
    let mut stale = stale;
    stale.rename("Late").unwrap();
    batch.update_category(&mut stale);
    parent.rename("Fresh").unwrap();
    let mut first = ports.batch();
    first.update_category(&mut parent);
    ports.commit(first).await.unwrap();
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.categories().get(cid(2)).await, Ok(Some(child)));
    assert_eq!(
        ports.categories().products_in(&[cid(2)]).await,
        Ok(vec![pid(2)])
    );

    // A staged handle that's taken fails the whole batch.
    let mut taken = category(3, None, 1);
    taken.set_handle(Handle::new("category-1").unwrap());
    let mut batch = ports.batch();
    batch.insert_category(&mut taken);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::CATEGORY_HANDLE))
    );
}

async fn deleting<S: HasCategories + HasBatch<Batch: StageCategories>>(ports: &S) {
    let repo = ports.categories();
    repo.insert(&mut category(1, None, 0)).await.unwrap();
    repo.insert(&mut category(2, Some(1), 0)).await.unwrap();
    repo.add_products(cid(2), &[pid(1)]).await.unwrap();

    let mut batch = ports.batch();
    batch.delete_category(cid(1));
    assert!(ports.commit(batch).await.is_err(), "it still has a child");
    assert!(repo.get(cid(1)).await.unwrap().is_some());

    let mut batch = ports.batch();
    batch.delete_category(cid(2));
    ports.commit(batch).await.unwrap();
    assert_eq!(repo.get(cid(2)).await, Ok(None));
    assert_eq!(repo.products_in(&[cid(2)]).await, Ok(vec![]));
    assert_eq!(repo.categories_of(&[pid(1)]).await, Ok(HashMap::new()));
}
