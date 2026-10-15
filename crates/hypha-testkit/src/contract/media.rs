use hypha_core::{AssetId, PageLimit, PageRequest, RepoError, Timestamp, Version, Versioned};
use hypha_media::{
    Asset, AssetKind, AssetRepo, BlobKey, BlobStore, Checksum, FileFacts, MimeType, NewUpload,
    PresignedUpload, UploadTerms, Visibility, repo::keys,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::scenario;

/// Runs every `AssetRepo` scenario, each on a fresh repo from `make`.
pub async fn asset_repo<R: AssetRepo>(make: impl AsyncFn() -> R) {
    scenario("asset: insert stores every field at version 1");
    asset_round_trips(&make().await).await;
    scenario("asset: update stores changes and bumps the version; stale fails");
    asset_updates(&make().await).await;
    scenario("asset: unique ID");
    asset_unique_id(&make().await).await;
    scenario("asset: list pages ready assets, newest first, by kind");
    asset_list_pages(&make().await).await;
    scenario("asset: abandoned finds old pending and failed assets, oldest first");
    asset_abandoned(&make().await).await;
    scenario("asset: delete removes it; deleting again is fine");
    asset_deletes(&make().await).await;
    scenario("asset: get_many returns the assets that exist");
    asset_get_many(&make().await).await;
}

const NOW: Timestamp = Timestamp::from_unix_micros(1_767_225_600_000_000);

pub(super) fn aid(n: u128) -> AssetId {
    AssetId::from_uuid(Uuid::from_u128(n))
}

fn pending(n: u128, mime_type: MimeType) -> Asset {
    Asset::new_upload(NewUpload {
        id: aid(n),
        filename: format!("File {n}.bin"),
        mime_type,
        byte_size: 1000 + n as u64,
        checksum: None,
        visibility: Visibility::Public,
        now: NOW,
    })
    .unwrap()
}

/// Every optional field set: what a mapping bug would lose.
fn full(n: u128) -> Asset {
    let mut asset = Asset::new_upload(NewUpload {
        id: aid(n),
        filename: "Summer shirt.webp".to_string(),
        mime_type: MimeType::Webp,
        byte_size: 20 * 1024 * 1024,
        checksum: Some(Checksum::from_bytes([7; 32])),
        visibility: Visibility::Private,
        now: NOW,
    })
    .unwrap();
    let later = NOW.plus_secs(60);
    asset
        .mark_ready(
            FileFacts {
                width: Some(4000),
                height: Some(u32::MAX),
            },
            later,
        )
        .unwrap();
    asset
        .set_alt(Some("A blue shirt".to_string()), later)
        .unwrap();
    asset
        .set_title(Some("Shirt, front".to_string()), later)
        .unwrap();
    asset
}

pub(super) fn ready(n: u128, mime_type: MimeType) -> Asset {
    let mut asset = pending(n, mime_type);
    asset.mark_ready(FileFacts::default(), NOW).unwrap();
    asset
}

async fn asset_round_trips<R: AssetRepo>(repo: &R) {
    assert_eq!(repo.get(aid(1)).await, Ok(None));
    for mut asset in [pending(1, MimeType::Png), full(2)] {
        repo.insert(&mut asset).await.unwrap();
        assert_eq!(asset.version(), Version::new(1));
        assert_eq!(repo.get(asset.id()).await, Ok(Some(asset)));
    }
}

async fn asset_updates<R: AssetRepo>(repo: &R) {
    let mut asset = pending(1, MimeType::Png);
    repo.insert(&mut asset).await.unwrap();
    let stale = asset.clone();
    asset.mark_ready(FileFacts::default(), NOW).unwrap();
    asset.set_alt(Some("Alt".to_string()), NOW).unwrap();
    repo.update(&mut asset).await.unwrap();
    assert_eq!(asset.version(), Version::new(2));
    assert_eq!(repo.get(aid(1)).await, Ok(Some(asset)));

    let mut stale = stale;
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    let mut missing = pending(9, MimeType::Png);
    missing.set_version(Version::new(1));
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

async fn asset_unique_id<R: AssetRepo>(repo: &R) {
    repo.insert(&mut pending(1, MimeType::Png)).await.unwrap();
    assert_eq!(
        repo.insert(&mut pending(1, MimeType::Jpeg)).await,
        Err(RepoError::Duplicate(keys::ASSET_ID))
    );
}

async fn listed<R: AssetRepo>(
    repo: &R,
    kind: Option<AssetKind>,
    after: Option<u128>,
    limit: u32,
) -> (Vec<AssetId>, bool) {
    let page = PageRequest {
        after: after.map(aid),
        limit: PageLimit::new(limit).unwrap(),
    };
    let page = repo.list(kind, page).await.unwrap();
    (page.items.iter().map(Asset::id).collect(), page.has_more)
}

async fn asset_list_pages<R: AssetRepo>(repo: &R) {
    assert_eq!(listed(repo, None, None, 10).await, (vec![], false));
    // 1 to 5 ready (4 a video), 6 pending: inserted out of order.
    for mut asset in [
        ready(3, MimeType::Png),
        ready(1, MimeType::Jpeg),
        pending(6, MimeType::Png),
        ready(5, MimeType::Webp),
        ready(4, MimeType::Mp4),
        ready(2, MimeType::Pdf),
    ] {
        repo.insert(&mut asset).await.unwrap();
    }

    let first = repo
        .list(
            None,
            PageRequest {
                after: None,
                limit: PageLimit::new(2).unwrap(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        first.items,
        vec![
            repo.get(aid(5)).await.unwrap().unwrap(),
            repo.get(aid(4)).await.unwrap().unwrap()
        ]
    );
    assert!(first.has_more);
    assert_eq!(
        listed(repo, None, Some(4), 2).await,
        (vec![aid(3), aid(2)], true)
    );
    assert_eq!(listed(repo, None, Some(2), 2).await, (vec![aid(1)], false));
    assert_eq!(
        listed(repo, Some(AssetKind::Image), None, 10).await,
        (vec![aid(5), aid(3), aid(1)], false)
    );
    assert_eq!(
        listed(repo, Some(AssetKind::Video), None, 10).await,
        (vec![aid(4)], false)
    );
    assert_eq!(
        listed(repo, Some(AssetKind::File), None, 10).await,
        (vec![aid(2)], false)
    );
}

async fn asset_abandoned<R: AssetRepo>(repo: &R) {
    let at = |secs| NOW.plus_secs(secs);
    // 1 pending since NOW; 2 failed at NOW+60; 3 ready; 4 pending since NOW+120.
    let mut failed = pending(2, MimeType::Png);
    failed.mark_failed(at(60)).unwrap();
    let late = Asset::new_upload(NewUpload {
        id: aid(4),
        filename: "late.png".to_string(),
        mime_type: MimeType::Png,
        byte_size: 1,
        checksum: None,
        visibility: Visibility::Public,
        now: at(120),
    })
    .unwrap();
    for mut asset in [
        late,
        ready(3, MimeType::Png),
        failed,
        pending(1, MimeType::Png),
    ] {
        repo.insert(&mut asset).await.unwrap();
    }
    let ids = |assets: Vec<Asset>| assets.iter().map(Asset::id).collect::<Vec<_>>();
    assert_eq!(ids(repo.abandoned(NOW, 10).await.unwrap()), vec![]);
    assert_eq!(ids(repo.abandoned(at(1), 10).await.unwrap()), vec![aid(1)]);
    assert_eq!(
        ids(repo.abandoned(at(61), 10).await.unwrap()),
        vec![aid(1), aid(2)]
    );
    assert_eq!(
        ids(repo.abandoned(at(1000), 10).await.unwrap()),
        vec![aid(1), aid(2), aid(4)],
        "never a ready asset"
    );
    assert_eq!(
        ids(repo.abandoned(at(1000), 2).await.unwrap()),
        vec![aid(1), aid(2)]
    );
}

async fn asset_deletes<R: AssetRepo>(repo: &R) {
    repo.insert(&mut pending(1, MimeType::Png)).await.unwrap();
    repo.insert(&mut pending(2, MimeType::Png)).await.unwrap();
    repo.delete(aid(1)).await.unwrap();
    assert_eq!(repo.get(aid(1)).await, Ok(None));
    assert!(repo.get(aid(2)).await.unwrap().is_some());
    assert_eq!(repo.delete(aid(1)).await, Ok(()));
}

async fn asset_get_many<R: AssetRepo>(repo: &R) {
    assert_eq!(repo.get_many(&[]).await, Ok(vec![]));
    let mut one = ready(1, MimeType::Png);
    let mut two = full(2);
    repo.insert(&mut one).await.unwrap();
    repo.insert(&mut two).await.unwrap();
    repo.insert(&mut ready(3, MimeType::Png)).await.unwrap();
    let mut found = repo.get_many(&[aid(2), aid(9), aid(1)]).await.unwrap();
    found.sort_by_key(Asset::id);
    assert_eq!(found, vec![one, two]);
}

/// Runs every `BlobStore` scenario against `store`, as a client would use it: `upload`
/// sends a presigned upload (its URL, method and headers, as given) with a body and says
/// whether storage took it; `download` fetches a URL and returns the body, or `None`
/// when refused. `now` is the current time where storage checks expiry (the real time
/// for a real store). Keys include `now`, so runs against a kept bucket don't collide.
pub async fn blob_store<B: BlobStore>(
    store: &B,
    now: Timestamp,
    upload: impl AsyncFn(&PresignedUpload, Vec<u8>) -> bool,
    download: impl AsyncFn(&str) -> Option<Vec<u8>>,
) {
    let key =
        |name: &str| BlobKey::restore(format!("assets/contract-{}/1/{name}", now.unix_micros()));
    let body: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    let terms = |len: usize, checksum: Option<Checksum>| UploadTerms {
        content_type: MimeType::Png,
        content_length: len as u64,
        checksum,
        cache_control: Visibility::Public.cache_control(),
        expires_in_secs: 900,
    };

    scenario("blob: an upload is stored, read by range and deleted");
    let file = key("file.png");
    let signed = store
        .presign_upload(&file, &terms(body.len(), None), now)
        .await
        .unwrap();
    assert_eq!(signed.method, "PUT");
    assert_eq!(signed.expires_at, now.plus_secs(900));
    assert_eq!(store.head(&file).await, Ok(None));
    assert!(
        upload(&signed, body.clone()).await,
        "a matching upload is stored"
    );
    assert_eq!(store.head(&file).await.unwrap().map(|h| h.size), Some(1000));
    assert_eq!(store.read_range(&file, 0, 10).await.unwrap(), body[..10]);
    assert_eq!(store.read_range(&file, 990, 64).await.unwrap(), body[990..]);
    assert_eq!(
        store.read_range(&file, 0, 1 << 20).await.unwrap(),
        body,
        "a range past the end stops at the end"
    );

    scenario("blob: an upload keeps its type and cache control");
    let head = store.head(&file).await.unwrap().unwrap();
    assert_eq!(
        (head.content_type.as_deref(), head.cache_control.as_deref()),
        (Some("image/png"), Some(Visibility::Public.cache_control()))
    );

    scenario("blob: a copy has the same bytes, type and cache control");
    let copied = key("copied.png");
    store.copy(&file, &copied).await.unwrap();
    assert_eq!(store.head(&copied).await.unwrap(), Some(head.clone()));
    assert_eq!(store.read_range(&copied, 0, 1 << 20).await.unwrap(), body);
    store.delete(&copied).await.unwrap();
    assert!(
        store.copy(&key("missing.png"), &copied).await.is_err(),
        "copying nothing fails"
    );
    assert_eq!(store.head(&copied).await, Ok(None));

    scenario("blob: a download URL returns a stored file");
    let url = store.presign_download(&file, now, 60).await.unwrap();
    assert_eq!(download(&url).await, Some(body.clone()));

    store.delete(&file).await.unwrap();
    assert_eq!(store.head(&file).await, Ok(None));
    assert_eq!(store.delete(&file).await, Ok(()), "deleting again is fine");

    scenario("blob: an upload of another size is refused");
    let file = key("short.png");
    let signed = store
        .presign_upload(&file, &terms(body.len(), None), now)
        .await
        .unwrap();
    assert!(!upload(&signed, body[..999].to_vec()).await);
    assert_eq!(store.head(&file).await, Ok(None));

    scenario("blob: an upload with another content type is refused");
    let file = key("type.png");
    let mut signed = store
        .presign_upload(&file, &terms(body.len(), None), now)
        .await
        .unwrap();
    for (name, value) in &mut signed.headers {
        if name.eq_ignore_ascii_case("content-type") {
            *value = "image/gif".to_string();
        }
    }
    assert!(!upload(&signed, body.clone()).await);
    assert_eq!(store.head(&file).await, Ok(None));

    scenario("blob: the declared checksum must match the contents");
    let checksum = Checksum::from_bytes(Sha256::digest(&body).into());
    let file = key("checksum.png");
    let signed = store
        .presign_upload(&file, &terms(body.len(), Some(checksum)), now)
        .await
        .unwrap();
    let mut other = body.clone();
    other[0] ^= 1;
    assert!(!upload(&signed, other).await, "other contents are refused");
    assert_eq!(store.head(&file).await, Ok(None));
    assert!(
        upload(&signed, body.clone()).await,
        "the declared contents are stored"
    );
    store.delete(&file).await.unwrap();

    scenario("blob: an expired upload URL is refused");
    let file = key("late.png");
    let signed = store
        .presign_upload(&file, &terms(body.len(), None), now.plus_secs(-3600))
        .await
        .unwrap();
    assert!(!upload(&signed, body.clone()).await);
    assert_eq!(store.head(&file).await, Ok(None));
}
