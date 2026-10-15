use hypha_core::{
    Actor, AssetId, AuthError, Ctx, PageRequest, Scope, TextError, Timestamp, UserId, Version,
    Versioned,
};
use hypha_media::{
    AssetRepo, AssetStatus, BlobStore, CommandError, CreateUpload, MediaError, MimeType,
    UpdateAsset, UploadMismatch, Visibility, complete_upload, create_upload, delete_asset,
    get_asset, list_assets, update_asset,
};
use hypha_testkit::{FixedClock, InMemoryAssetRepo, InMemoryBlobStore, SequentialIds};
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
}

impl World {
    fn new() -> Self {
        Self {
            assets: InMemoryAssetRepo::default(),
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
            shirt_png()
        ))
        .map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MediaWrite
        )))
    );
}
