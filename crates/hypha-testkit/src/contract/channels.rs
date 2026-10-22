//! What every channel store must do: channels, and publications written directly and staged
//! in a batch.

use hypha_channels::{Channel, ChannelKind, ChannelRepo, HasChannels, StagePublications, keys};
use hypha_core::{ChannelId, HasBatch, ProductId, RepoError, Version, Versioned};
use uuid::Uuid;

use super::scenario;

fn cid(n: u128) -> ChannelId {
    ChannelId::from_uuid(Uuid::from_u128(n))
}

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn channel(n: u128, name: &str) -> Channel {
    Channel::new(cid(n), name, ChannelKind::Custom).unwrap()
}

/// Runs every channel scenario, each on fresh ports from `make`.
pub async fn channel_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasChannels + HasBatch<Batch: StagePublications>,
{
    scenario("channel: insert stores every field at version 1; list is oldest first");
    round_trip(&make().await).await;
    scenario("channel: names are unique ignoring case, on insert and update");
    unique_names(&make().await).await;
    scenario("channel: case is compared by name_key, beyond ASCII too; found by it");
    name_keys(&make().await).await;
    scenario("channel: update from a stale copy fails");
    stale_update(&make().await).await;
    scenario("channel: publications are written, read and removed in batches");
    publications(&make().await).await;
    scenario("channel: publishing to a missing channel fails");
    missing_channel(&make().await).await;
    scenario("channel: publications staged in a batch commit together, or not at all");
    staged(&make().await).await;
}

async fn round_trip<S: HasChannels>(ports: &S) {
    let repo = ports.channels();
    let mut agents = Channel::new(cid(2), "Agents", ChannelKind::Agentic).unwrap();
    let mut store = Channel::new(cid(1), "Online Store", ChannelKind::OnlineStore).unwrap();
    repo.insert(&mut agents).await.unwrap();
    repo.insert(&mut store).await.unwrap();
    assert_eq!(store.version(), Version::new(1));
    let stored = repo.get(cid(1)).await.unwrap().unwrap();
    assert_eq!(stored, store);
    assert!(hypha_core::Stamped::stamps(&stored).is_some());
    assert_eq!(repo.get(cid(9)).await, Ok(None));
    assert_eq!(repo.list().await, Ok(vec![store, agents]));
}

async fn unique_names<S: HasChannels>(ports: &S) {
    let repo = ports.channels();
    repo.insert(&mut channel(1, "Outlet")).await.unwrap();
    let mut other = channel(2, "Web");
    repo.insert(&mut other).await.unwrap();
    assert_eq!(
        repo.insert(&mut channel(3, "OUTLET")).await,
        Err(RepoError::Duplicate(keys::CHANNEL_NAME))
    );
    assert_eq!(
        repo.insert(&mut channel(1, "Elsewhere")).await,
        Err(RepoError::Duplicate(keys::CHANNEL_ID))
    );
    other.rename("outlet").unwrap();
    assert_eq!(
        repo.update(&mut other).await,
        Err(RepoError::Duplicate(keys::CHANNEL_NAME))
    );
}

async fn name_keys<S: HasChannels>(ports: &S) {
    let repo = ports.channels();
    let mut summer = channel(1, "Été");
    repo.insert(&mut summer).await.unwrap();
    // Postgres's lower() leaves "É" alone under the C collation; name_key doesn't.
    assert_eq!(
        repo.insert(&mut channel(2, "ÉTÉ")).await,
        Err(RepoError::Duplicate(keys::CHANNEL_NAME))
    );
    assert_eq!(repo.find_by_name("été").await, Ok(Some(summer.clone())));
    assert_eq!(repo.find_by_name("winter").await, Ok(None));

    // A rename moves the key: the old one is free, the new one found.
    summer.rename("Winter").unwrap();
    repo.update(&mut summer).await.unwrap();
    assert_eq!(repo.find_by_name("été").await, Ok(None));
    assert_eq!(
        repo.find_by_name("winter").await.unwrap().map(|c| c.id()),
        Some(cid(1))
    );
}

async fn stale_update<S: HasChannels>(ports: &S) {
    let repo = ports.channels();
    let mut c = channel(1, "Web");
    repo.insert(&mut c).await.unwrap();
    let mut stale = c.clone();
    c.rename("Web shop").unwrap();
    repo.update(&mut c).await.unwrap();
    assert_eq!(c.version(), Version::new(2));
    stale.rename("Other").unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(cid(1)).await.unwrap().unwrap().name(), "Web shop");
}

async fn publications<S: HasChannels>(ports: &S) {
    let repo = ports.channels();
    repo.insert(&mut channel(1, "Web")).await.unwrap();
    repo.insert(&mut channel(2, "Agents")).await.unwrap();
    repo.publish(cid(1), &[pid(10), pid(11)]).await.unwrap();
    repo.publish(cid(1), &[pid(11)]).await.unwrap(); // already: ignored
    repo.publish(cid(2), &[pid(11)]).await.unwrap();

    let published = repo
        .published(cid(1), &[pid(10), pid(11), pid(12)])
        .await
        .unwrap();
    assert_eq!(published, [pid(10), pid(11)].into());
    let of = repo
        .channels_of(&[pid(10), pid(11), pid(12)])
        .await
        .unwrap();
    assert_eq!(of.get(&pid(10)), Some(&vec![cid(1)]));
    assert_eq!(of.get(&pid(11)), Some(&vec![cid(1), cid(2)]), "sorted");
    assert_eq!(of.get(&pid(12)), None);

    repo.unpublish(cid(1), &[pid(11), pid(12)]).await.unwrap();
    assert_eq!(
        repo.published(cid(1), &[pid(10), pid(11)]).await.unwrap(),
        [pid(10)].into()
    );
    assert_eq!(repo.published(cid(1), &[]).await.unwrap(), [].into());
    assert!(repo.channels_of(&[]).await.unwrap().is_empty());
}

async fn missing_channel<S: HasChannels>(ports: &S) {
    assert!(ports.channels().publish(cid(9), &[pid(1)]).await.is_err());
}

async fn staged<S>(ports: &S)
where
    S: HasChannels + HasBatch<Batch: StagePublications>,
{
    let repo = ports.channels();
    repo.insert(&mut channel(1, "Web")).await.unwrap();
    repo.publish(cid(1), &[pid(10)]).await.unwrap();

    let mut batch = ports.batch();
    batch.publish_products(cid(1), &[pid(11)]);
    batch.unpublish_products(cid(1), &[pid(10)]);
    ports.commit(batch).await.unwrap();
    assert_eq!(
        repo.published(cid(1), &[pid(10), pid(11)]).await.unwrap(),
        [pid(11)].into()
    );

    // A publication to a missing channel fails the batch: the other one isn't written.
    let mut batch = ports.batch();
    batch.publish_products(cid(1), &[pid(12)]);
    batch.publish_products(cid(9), &[pid(12)]);
    assert!(ports.commit(batch).await.is_err());
    assert_eq!(repo.published(cid(1), &[pid(12)]).await.unwrap(), [].into());
}
