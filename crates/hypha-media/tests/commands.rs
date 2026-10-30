#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]

use hypha_core::{
    Actor, AssetId, AuthError, Ctx, PageLimit, PageRequest, RepoError, Scope, TextError, Timestamp,
    UserId, Version, Versioned,
};
use hypha_media::{
    AssetRepo, AssetStatus, AssetUsage, BlobStore, CommandError, CreateUpload, IN_USE_LISTED,
    MediaError, MediaSettings, MimeType, UpdateAsset, UploadMismatch, UsageOwner, UsageRole,
    Visibility, complete_upload, create_upload, delete_asset, get_asset, list_asset_usages,
    list_assets, sweep_abandoned_uploads, update_asset,
};
use hypha_testkit::{
    FixedClock, InMemoryAssetRepo, InMemoryBlobStore, InMemoryOutbox, SequentialIds,
};
use pollster::block_on;
use uuid::Uuid;

/// A 3 × 2 PNG's first bytes (signature and IHDR), padded to `len` bytes.
fn png(len: usize) -> Vec<u8> {
    let mut file = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    file.extend_from_slice(&3u32.to_be_bytes());
    file.extend_from_slice(&2u32.to_be_bytes());
    file.extend_from_slice(&[8, 6, 0, 0, 0]);
    file.resize(len, 0);
    file
}

fn shirt_png() -> CreateUpload {
    CreateUpload {
        filename: "Shirt.png".to_string(),
        mime_type: "image/png".to_string(),
        byte_size: 100,
        checksum: None,
        visibility: Visibility::Public,
    }
}

struct World {
    assets: InMemoryAssetRepo,
    blobs: InMemoryBlobStore,
    ids: SequentialIds,
    clock: FixedClock,
    outbox: InMemoryOutbox,
}

impl World {
    fn new() -> Self {
        let outbox = InMemoryOutbox::default();
        Self {
            assets: InMemoryAssetRepo::sharing(&outbox),
            outbox,
            blobs: InMemoryBlobStore::default(),
            ids: SequentialIds::default(),
            clock: FixedClock::default(),
        }
    }

    fn create(&self, input: CreateUpload) -> Result<AssetId, CommandError> {
        let (asset, _) = block_on(create_upload(
            &Ctx::system(),
            &self.assets,
            &self.blobs,
            &self.ids,
            &self.clock,
            &MediaSettings::default(),
            input,
        ))?;
        Ok(asset.id())
    }

    /// Creates the asset, uploads `body` with its presigned request, and completes it.
    fn upload(&self, input: CreateUpload, body: Vec<u8>) -> (AssetId, Result<(), CommandError>) {
        let (asset, upload) = block_on(create_upload(
            &Ctx::system(),
            &self.assets,
            &self.blobs,
            &self.ids,
            &self.clock,
            &MediaSettings::default(),
            input,
        ))
        .unwrap();
        assert!(self.blobs.upload(&upload, body, FixedClock::DEFAULT));
        let completed = block_on(complete_upload(
            &Ctx::system(),
            &self.assets,
            &self.blobs,
            &self.clock,
            asset.id(),
        ));
        (asset.id(), completed.map(|_| ()))
    }
}

fn user_with(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(99))), scopes)
}

#[test]
fn create_upload_stores_a_pending_asset_and_signs_its_upload() {
    let world = World::new();
    let (asset, upload) = block_on(create_upload(
        &Ctx::system(),
        &world.assets,
        &world.blobs,
        &world.ids,
        &world.clock,
        &MediaSettings::default(),
        shirt_png(),
    ))
    .unwrap();
    assert_eq!(asset.status(), AssetStatus::Pending);
    assert_eq!(asset.mime_type(), MimeType::Png);
    assert_eq!(upload.expires_at, FixedClock::DEFAULT.plus_secs(15 * 60));
    assert_eq!(block_on(world.assets.get(asset.id())), Ok(Some(asset)));
}

#[test]
fn create_upload_refuses_types_it_doesnt_accept() {
    let world = World::new();
    let mut svg = shirt_png();
    svg.mime_type = "image/svg+xml".to_string();
    assert_eq!(
        world.create(svg),
        Err(CommandError::Media(MediaError::UnsupportedMediaType(
            "image/svg+xml".to_string()
        )))
    );
}

#[test]
fn create_upload_checks_the_checksum_format() {
    let world = World::new();
    let mut input = shirt_png();
    input.checksum = Some("abc".to_string());
    assert_eq!(
        world.create(input),
        Err(CommandError::Media(MediaError::InvalidChecksum(
            "abc".to_string()
        )))
    );
}

#[test]
fn completing_a_matching_upload_makes_the_asset_ready_with_its_dimensions() {
    let world = World::new();
    let (id, completed) = world.upload(shirt_png(), png(100));
    assert_eq!(completed, Ok(()));
    let asset = block_on(world.assets.get(id)).unwrap().unwrap();
    assert_eq!(asset.status(), AssetStatus::Ready);
    assert_eq!((asset.width(), asset.height()), (Some(3), Some(2)));
    // Served from its own key; the upload key is emptied.
    assert_eq!(
        block_on(world.blobs.head(asset.key()))
            .unwrap()
            .map(|h| h.size),
        Some(100)
    );
    assert_eq!(block_on(world.blobs.head(&asset.upload_key())), Ok(None));
}

#[test]
fn uploading_again_after_completing_doesnt_change_the_served_file() {
    let world = World::new();
    let (asset, upload) = block_on(create_upload(
        &Ctx::system(),
        &world.assets,
        &world.blobs,
        &world.ids,
        &world.clock,
        &MediaSettings::default(),
        shirt_png(),
    ))
    .unwrap();
    assert!(world.blobs.upload(&upload, png(100), FixedClock::DEFAULT));
    block_on(complete_upload(
        &Ctx::system(),
        &world.assets,
        &world.blobs,
        &world.clock,
        asset.id(),
    ))
    .unwrap();

    // The URL still works until it expires, but it only reaches the upload key.
    let mut other = b"%PDF-1.7".to_vec();
    other.resize(100, 0);
    assert!(world.blobs.upload(&upload, other, FixedClock::DEFAULT));
    assert_eq!(
        block_on(world.blobs.read_range(asset.key(), 0, 100)),
        Ok(png(100))
    );
}

#[test]
fn completing_again_returns_the_ready_asset_unchanged() {
    let world = World::new();
    let (id, _) = world.upload(shirt_png(), png(100));
    let again = block_on(complete_upload(
        &Ctx::system(),
        &world.assets,
        &world.blobs,
        &world.clock,
        id,
    ))
    .unwrap();
    assert_eq!(again.version(), Version::new(2));
}

#[test]
fn completing_before_the_upload_is_upload_missing() {
    let world = World::new();
    let id = world.create(shirt_png()).unwrap();
    assert_eq!(
        block_on(complete_upload(
            &Ctx::system(),
            &world.assets,
            &world.blobs,
            &world.clock,
            id
        )),
        Err(CommandError::UploadMissing(id))
    );
    let asset = block_on(world.assets.get(id)).unwrap().unwrap();
    assert_eq!(asset.status(), AssetStatus::Pending);
}

#[test]
fn a_file_that_isnt_what_was_declared_fails_and_is_deleted() {
    let world = World::new();
    let mut body = b"%PDF-1.7".to_vec();
    body.resize(100, 0);
    let (id, completed) = world.upload(shirt_png(), body);
    assert_eq!(
        completed,
        Err(CommandError::UploadMismatch(UploadMismatch::Type {
            declared: MimeType::Png
        }))
    );
    let asset = block_on(world.assets.get(id)).unwrap().unwrap();
    assert_eq!(asset.status(), AssetStatus::Failed);
    assert_eq!(block_on(world.blobs.head(&asset.upload_key())), Ok(None));
    assert_eq!(block_on(world.blobs.head(asset.key())), Ok(None));
    assert_eq!(
        block_on(complete_upload(
            &Ctx::system(),
            &world.assets,
            &world.blobs,
            &world.clock,
            id
        )),
        Err(CommandError::Media(MediaError::NotPending {
            status: AssetStatus::Failed
        }))
    );
}

#[test]
fn the_library_lists_ready_assets_only() {
    let world = World::new();
    let (ready, _) = world.upload(shirt_png(), png(100));
    world.create(shirt_png()).unwrap();
    let page = block_on(list_assets(
        &Ctx::system(),
        &world.assets,
        None,
        None,
        hypha_media::AssetSort::Newest,
        PageRequest::default(),
    ))
    .unwrap();
    assert_eq!(
        page.items.iter().map(|a| a.id()).collect::<Vec<_>>(),
        [ready]
    );
}

#[test]
fn update_asset_sets_and_clears_alt_text_and_title() {
    let world = World::new();
    let (id, _) = world.upload(shirt_png(), png(100));
    let later = Timestamp::from_unix_micros(FixedClock::DEFAULT.unix_micros() + 1);
    world.clock.set(later);
    let input = UpdateAsset {
        alt: Some(Some(" A blue shirt ".to_string())),
        title: Some(Some("Front".to_string())),
    };
    let asset = block_on(update_asset(
        &Ctx::system(),
        &world.assets,
        &world.clock,
        id,
        input,
    ))
    .unwrap();
    assert_eq!(
        (asset.alt(), asset.title()),
        (Some("A blue shirt"), Some("Front"))
    );
    assert_eq!(asset.updated_at(), later);

    let input = UpdateAsset {
        alt: Some(None),
        title: None,
    };
    let asset = block_on(update_asset(
        &Ctx::system(),
        &world.assets,
        &world.clock,
        id,
        input,
    ))
    .unwrap();
    assert_eq!((asset.alt(), asset.title()), (None, Some("Front")));
}

#[test]
fn update_asset_refuses_alt_text_over_the_limit() {
    let world = World::new();
    let (id, _) = world.upload(shirt_png(), png(100));
    let input = UpdateAsset {
        alt: Some(Some("a".repeat(256))),
        title: None,
    };
    assert_eq!(
        block_on(update_asset(
            &Ctx::system(),
            &world.assets,
            &world.clock,
            id,
            input
        )),
        Err(CommandError::Media(MediaError::Text(TextError::TooLong(
            "alt"
        ))))
    );
}

#[test]
fn delete_asset_removes_the_asset_and_its_file() {
    let world = World::new();
    let (id, _) = world.upload(shirt_png(), png(100));
    let key = block_on(world.assets.get(id))
        .unwrap()
        .unwrap()
        .key()
        .clone();
    block_on(delete_asset(
        &Ctx::system(),
        &world.assets,
        &world.blobs,
        id,
    ))
    .unwrap();
    assert_eq!(block_on(world.assets.get(id)), Ok(None));
    assert_eq!(block_on(world.blobs.head(&key)), Ok(None));
    let events = world.outbox.events();
    assert_eq!(world.outbox.types(), ["asset.deleted"]);
    assert_eq!(
        (events[0].subject_kind.as_str(), events[0].subject_id),
        ("asset", id.as_uuid())
    );
    assert_eq!(
        events[0].data,
        serde_json::json!({ "asset": id.to_string() })
    );
    assert_eq!(
        block_on(get_asset(&Ctx::system(), &world.assets, id)),
        Err(CommandError::AssetNotFound(id))
    );
}

#[test]
fn reading_needs_media_read_and_changing_needs_media_write() {
    let world = World::new();
    let (id, _) = world.upload(shirt_png(), png(100));
    let nobody = user_with(vec![Scope::ProductsWrite]);
    assert_eq!(
        block_on(get_asset(&nobody, &world.assets, id)),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MediaRead
        )))
    );
    let reader = user_with(vec![Scope::MediaRead]);
    assert!(block_on(get_asset(&reader, &world.assets, id)).is_ok());
    assert_eq!(
        block_on(delete_asset(&reader, &world.assets, &world.blobs, id)),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MediaWrite
        )))
    );
    assert_eq!(
        block_on(create_upload(
            &reader,
            &world.assets,
            &world.blobs,
            &world.ids,
            &world.clock,
            &MediaSettings::default(),
            shirt_png()
        ))
        .map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MediaWrite
        )))
    );
    let input = UpdateAsset {
        alt: Some(Some("A blue shirt".to_string())),
        title: None,
    };
    assert_eq!(
        block_on(update_asset(
            &reader,
            &world.assets,
            &world.clock,
            id,
            input
        ))
        .map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MediaWrite
        )))
    );
}

/// An asset repo whose first `get` returns `stale`: the asset as one `complete_upload`
/// loaded it, before a racing call completed it.
struct LoadedBeforeTheRaceWasLost {
    repo: InMemoryAssetRepo,
    stale: std::cell::RefCell<Option<hypha_media::Asset>>,
}

impl AssetRepo for LoadedBeforeTheRaceWasLost {
    async fn get_many(&self, ids: &[AssetId]) -> Result<Vec<hypha_media::Asset>, RepoError> {
        self.repo.get_many(ids).await
    }
    async fn get(&self, id: AssetId) -> Result<Option<hypha_media::Asset>, RepoError> {
        // Taken in its own statement, so the borrow isn't held across the `.await`.
        let stale = self.stale.borrow_mut().take();
        match stale {
            Some(stale) => Ok(Some(stale)),
            None => self.repo.get(id).await,
        }
    }
    async fn list(
        &self,
        filter: &hypha_media::AssetFilter,
        sort: hypha_media::AssetSort,
        page: PageRequest<hypha_core::Keyset<AssetId>>,
    ) -> Result<hypha_core::Page<hypha_media::Asset>, RepoError> {
        self.repo.list(filter, sort, page).await
    }
    async fn abandoned(
        &self,
        before: Timestamp,
        limit: u32,
    ) -> Result<Vec<hypha_media::Asset>, RepoError> {
        self.repo.abandoned(before, limit).await
    }
    async fn insert(&self, asset: &mut hypha_media::Asset) -> Result<(), RepoError> {
        self.repo.insert(asset).await
    }
    async fn update(&self, asset: &mut hypha_media::Asset) -> Result<(), RepoError> {
        self.repo.update(asset).await
    }
    async fn delete(&self, asset: &mut hypha_media::Asset) -> Result<(), RepoError> {
        self.repo.delete(asset).await
    }
}

#[test]
fn completing_twice_at_once_succeeds_both_times() {
    let world = World::new();
    let (asset, upload) = block_on(create_upload(
        &Ctx::system(),
        &world.assets,
        &world.blobs,
        &world.ids,
        &world.clock,
        &MediaSettings::default(),
        shirt_png(),
    ))
    .unwrap();
    assert!(world.blobs.upload(&upload, png(100), FixedClock::DEFAULT));
    let pending = block_on(world.assets.get(asset.id())).unwrap();
    // The other call wins: the asset is ready and the upload key empty.
    block_on(complete_upload(
        &Ctx::system(),
        &world.assets,
        &world.blobs,
        &world.clock,
        asset.id(),
    ))
    .unwrap();

    let World {
        assets,
        blobs,
        clock,
        ..
    } = world;
    let racing = LoadedBeforeTheRaceWasLost {
        repo: assets,
        stale: std::cell::RefCell::new(pending),
    };
    let completed = block_on(complete_upload(
        &Ctx::system(),
        &racing,
        &blobs,
        &clock,
        asset.id(),
    ))
    .unwrap();
    assert_eq!(completed.status(), AssetStatus::Ready);
    assert_eq!(completed.version(), Version::new(2));
}

#[test]
fn the_sweep_deletes_uploads_abandoned_for_a_day() {
    let world = World::new();
    let (ready, _) = world.upload(shirt_png(), png(100));
    // Uploaded, never completed (the tab closed).
    let (abandoned, upload) = block_on(create_upload(
        &Ctx::system(),
        &world.assets,
        &world.blobs,
        &world.ids,
        &world.clock,
        &MediaSettings::default(),
        shirt_png(),
    ))
    .unwrap();
    assert!(world.blobs.upload(&upload, png(100), FixedClock::DEFAULT));

    let sweep = || {
        block_on(sweep_abandoned_uploads(
            &Ctx::system(),
            &world.assets,
            &world.blobs,
            &world.clock,
            &MediaSettings::default(),
        ))
    };
    assert_eq!(sweep(), Ok(0), "not a day old yet");
    world
        .clock
        .set(FixedClock::DEFAULT.plus_secs(24 * 60 * 60 + 1));
    assert_eq!(sweep(), Ok(1));
    assert_eq!(block_on(world.assets.get(abandoned.id())), Ok(None));
    assert_eq!(
        block_on(world.blobs.head(&abandoned.upload_key())),
        Ok(None)
    );
    assert!(block_on(world.assets.get(ready)).unwrap().is_some());
    assert_eq!(world.outbox.types(), ["asset.deleted"]);
}

// ---- usages ----

/// A world with batches, to record usages the way merchandising does.
struct UsageWorld {
    ports: hypha_testkit::InMemoryPorts,
    blobs: InMemoryBlobStore,
}

impl UsageWorld {
    /// Ready assets 1..=n, and the given product numbers using asset 1.
    fn new(assets: u128, users: &[u128]) -> Self {
        let world = Self {
            ports: hypha_testkit::InMemoryPorts::default(),
            blobs: InMemoryBlobStore::default(),
        };
        for n in 1..=assets {
            let mut asset = hypha_media::Asset::new_upload(hypha_media::NewUpload {
                id: aid(n),
                filename: format!("{n}.png"),
                mime_type: MimeType::Png,
                byte_size: 100,
                checksum: None,
                visibility: Visibility::Public,
                now: FixedClock::DEFAULT,
            })
            .unwrap();
            asset
                .mark_ready(hypha_media::FileFacts::default(), FixedClock::DEFAULT)
                .unwrap();
            block_on(world.ports.assets.insert(&mut asset)).unwrap();
        }
        for n in users {
            world.use_asset(*n, &[1]);
        }
        world
    }

    fn use_asset(&self, product: u128, assets: &[u128]) {
        use hypha_core::HasBatch;
        use hypha_media::StageAssetUsages;
        let usages: Vec<_> = assets
            .iter()
            .map(|n| (aid(*n), UsageRole::gallery()))
            .collect();
        let mut batch = self.ports.batch();
        batch.replace_asset_usages(&owner(product), &usages);
        block_on(self.ports.commit(batch)).unwrap();
    }
}

fn aid(n: u128) -> AssetId {
    AssetId::from_uuid(Uuid::from_u128(n))
}

fn owner(n: u128) -> UsageOwner {
    UsageOwner::product(hypha_core::ProductId::from_uuid(Uuid::from_u128(n)))
}

fn usage(product: u128) -> AssetUsage {
    AssetUsage {
        asset: aid(1),
        owner: owner(product),
        role: UsageRole::gallery(),
    }
}

#[test]
fn an_asset_in_use_isnt_deleted() {
    let world = UsageWorld::new(1, &[7, 8]);
    assert_eq!(
        block_on(delete_asset(
            &Ctx::system(),
            &world.ports,
            &world.blobs,
            aid(1)
        )),
        Err(CommandError::AssetInUse {
            asset: aid(1),
            usages: vec![usage(8), usage(7)],
            has_more: false,
        })
    );
    assert!(block_on(world.ports.assets.get(aid(1))).unwrap().is_some());
    let deleted = || {
        world
            .ports
            .outbox
            .types()
            .into_iter()
            .filter(|t| t == "asset.deleted")
            .count()
    };
    assert_eq!(deleted(), 0, "a refused delete records nothing");

    world.use_asset(7, &[]);
    world.use_asset(8, &[]);
    assert_eq!(
        block_on(delete_asset(
            &Ctx::system(),
            &world.ports,
            &world.blobs,
            aid(1)
        )),
        Ok(())
    );
    assert_eq!(deleted(), 1);
}

#[test]
fn asset_in_use_lists_the_first_usages_and_says_there_are_more() {
    let users: Vec<u128> = (1..=IN_USE_LISTED as u128 + 1).collect();
    let world = UsageWorld::new(1, &users);
    let Err(CommandError::AssetInUse {
        usages, has_more, ..
    }) = block_on(delete_asset(
        &Ctx::system(),
        &world.ports,
        &world.blobs,
        aid(1),
    ))
    else {
        panic!("expected asset_in_use");
    };
    assert_eq!((usages.len(), has_more), (IN_USE_LISTED as usize, true));
}

#[test]
fn list_asset_usages_pages_by_owner() {
    let world = UsageWorld::new(2, &[1, 2, 3]);
    let page = |after: Option<u128>, limit| PageRequest {
        after: after.map(owner),
        limit: PageLimit::new(limit).unwrap(),
    };
    let first = block_on(list_asset_usages(
        &Ctx::system(),
        &world.ports,
        aid(1),
        page(None, 2),
    ))
    .unwrap();
    assert_eq!(
        (first.items, first.has_more),
        (vec![usage(3), usage(2)], true)
    );
    let rest = block_on(list_asset_usages(
        &Ctx::system(),
        &world.ports,
        aid(1),
        page(Some(2), 2),
    ))
    .unwrap();
    assert_eq!((rest.items, rest.has_more), (vec![usage(1)], false));
    let unused = block_on(list_asset_usages(
        &Ctx::system(),
        &world.ports,
        aid(2),
        page(None, 2),
    ))
    .unwrap();
    assert_eq!(unused.items, vec![]);
}

#[test]
fn list_asset_usages_of_an_unknown_asset_is_not_found() {
    let world = UsageWorld::new(0, &[]);
    assert_eq!(
        block_on(list_asset_usages(
            &Ctx::system(),
            &world.ports,
            aid(9),
            PageRequest::default()
        )),
        Err(CommandError::AssetNotFound(aid(9)))
    );
}

#[test]
fn list_asset_usages_needs_media_read() {
    let world = UsageWorld::new(1, &[]);
    assert_eq!(
        block_on(list_asset_usages(
            &user_with(vec![Scope::ProductsRead]),
            &world.ports,
            aid(1),
            PageRequest::default()
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MediaRead
        )))
    );
}
