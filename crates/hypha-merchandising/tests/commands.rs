#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]

use hypha_core::{
    Actor, AssetId, AuthError, Ctx, Currency, Money, PageLimit, PageRequest, Price, ProductId,
    Scope, UserId, VariantId,
};
use hypha_media::{
    Asset, AssetRepo, AssetUsage, AssetUsageRepo, FileFacts, HasAssetUsages, MimeType, NewUpload,
    UsageOwner, UsageRole, Visibility,
};
use hypha_merchandising::{
    CommandError, get_product_view, list_product_views, set_product_media, set_variant_media,
};
use hypha_products::{
    Handle, NewVariant, OptionValue, Product, ProductError, ProductOption, ProductRepo,
};
use hypha_testkit::{FixedClock, InMemoryPorts};
use pollster::block_on;
use uuid::Uuid;

fn aid(n: u128) -> AssetId {
    AssetId::from_uuid(Uuid::from_u128(n))
}
fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}
fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

/// Ports with a shirt (product 1: variants 11 and 12) and a mug (product 2), and assets:
/// 1–3 PNGs, 4 a video, 5 a PDF, 6 a private PNG, 7 a pending PNG.
fn world() -> InMemoryPorts {
    let ports = InMemoryPorts::default();
    let price = Price::new(Money::new(1000, Currency::EUR).unwrap()).unwrap();
    let size = ProductOption::new(
        hypha_core::OptionId::from_uuid(Uuid::from_u128(100)),
        "Size".to_string(),
        vec![
            OptionValue::new(
                hypha_core::OptionValueId::from_uuid(Uuid::from_u128(101)),
                "S".into(),
            ),
            OptionValue::new(
                hypha_core::OptionValueId::from_uuid(Uuid::from_u128(102)),
                "M".into(),
            ),
        ],
    );
    let variant = |n: u128, value: u128| NewVariant {
        id: vid(n),
        sku: None,
        price,
        selections: vec![hypha_core::OptionValueId::from_uuid(Uuid::from_u128(value))],
    };
    let mut shirt = Product::new_configurable(
        pid(1),
        Handle::new("shirt").unwrap(),
        "Shirt".to_string(),
        vec![size],
        variant(11, 101),
    )
    .unwrap();
    shirt.add_variant(variant(12, 102)).unwrap();
    block_on(ports.products.insert(&mut shirt)).unwrap();
    let mut mug = Product::new_simple(
        pid(2),
        Handle::new("mug").unwrap(),
        "Mug".to_string(),
        vid(21),
        price,
    )
    .unwrap();
    block_on(ports.products.insert(&mut mug)).unwrap();

    let assets = [
        (1, MimeType::Png, Visibility::Public, true),
        (2, MimeType::Png, Visibility::Public, true),
        (3, MimeType::Png, Visibility::Public, true),
        (4, MimeType::Mp4, Visibility::Public, true),
        (5, MimeType::Pdf, Visibility::Public, true),
        (6, MimeType::Png, Visibility::Private, true),
        (7, MimeType::Png, Visibility::Public, false),
    ];
    for (n, mime_type, visibility, ready) in assets {
        let mut asset = Asset::new_upload(NewUpload {
            id: aid(n),
            filename: format!("{n}.bin"),
            mime_type,
            byte_size: 100,
            checksum: None,
            visibility,
            now: FixedClock::DEFAULT,
        })
        .unwrap();
        if ready {
            asset
                .mark_ready(FileFacts::default(), FixedClock::DEFAULT)
                .unwrap();
        }
        block_on(ports.assets.insert(&mut asset)).unwrap();
    }
    ports
}

fn usages(ports: &InMemoryPorts, asset: u128) -> Vec<AssetUsage> {
    let page = PageRequest {
        after: None,
        limit: PageLimit::new(100).unwrap(),
    };
    block_on(ports.asset_usages().list_usages(aid(asset), page))
        .unwrap()
        .items
}

fn usage(asset: u128, product: u128, role: UsageRole) -> AssetUsage {
    AssetUsage {
        asset: aid(asset),
        owner: UsageOwner::product(pid(product)),
        role,
    }
}

fn attach(
    ports: &InMemoryPorts,
    product: u128,
    assets: &[u128],
) -> Result<hypha_merchandising::ProductView, CommandError> {
    let assets = assets.iter().map(|n| aid(*n)).collect();
    block_on(set_product_media(
        &Ctx::system(),
        ports,
        pid(product),
        assets,
    ))
}

fn ids(assets: &[Asset]) -> Vec<AssetId> {
    assets.iter().map(Asset::id).collect()
}

fn user_with(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(99))), scopes)
}

// ---- set_product_media ----

#[test]
fn attaches_in_order_with_the_first_featured() {
    let ports = world();
    let view = attach(&ports, 1, &[2, 4, 1]).unwrap();
    assert_eq!(view.product.media(), &[aid(2), aid(4), aid(1)]);
    assert_eq!(ids(&view.media), vec![aid(2), aid(4), aid(1)]);
    assert_eq!(
        block_on(ports.products.get(pid(1))).unwrap().unwrap(),
        view.product
    );
    assert_eq!(usages(&ports, 2), vec![usage(2, 1, UsageRole::featured())]);
    assert_eq!(usages(&ports, 1), vec![usage(1, 1, UsageRole::gallery())]);
}

#[test]
fn reordering_moves_the_featured_role() {
    let ports = world();
    attach(&ports, 1, &[1, 2]).unwrap();
    attach(&ports, 1, &[2, 1]).unwrap();
    assert_eq!(usages(&ports, 1), vec![usage(1, 1, UsageRole::gallery())]);
    assert_eq!(usages(&ports, 2), vec![usage(2, 1, UsageRole::featured())]);
}

#[test]
fn removing_an_item_removes_its_usage_and_the_variant_image() {
    let ports = world();
    attach(&ports, 1, &[1, 2]).unwrap();
    block_on(set_variant_media(
        &Ctx::system(),
        &ports,
        pid(1),
        vid(11),
        Some(aid(2)),
    ))
    .unwrap();
    let view = attach(&ports, 1, &[1]).unwrap();
    assert_eq!(view.product.variant(vid(11)).unwrap().media(), None);
    assert_eq!(usages(&ports, 2), vec![]);
}

#[test]
fn one_asset_on_two_products_has_two_usages() {
    let ports = world();
    attach(&ports, 1, &[1]).unwrap();
    attach(&ports, 2, &[3, 1]).unwrap();
    assert_eq!(
        usages(&ports, 1),
        vec![
            usage(1, 2, UsageRole::gallery()),
            usage(1, 1, UsageRole::featured())
        ]
    );
}

#[test]
fn an_unknown_asset_writes_nothing() {
    let ports = world();
    let before = block_on(ports.products.get(pid(1))).unwrap();
    assert_eq!(
        attach(&ports, 1, &[1, 9]),
        Err(CommandError::AssetNotFound(aid(9)))
    );
    assert_eq!(block_on(ports.products.get(pid(1))).unwrap(), before);
    assert_eq!(usages(&ports, 1), vec![]);
}

#[test]
fn only_ready_public_images_and_videos_attach() {
    let ports = world();
    assert_eq!(
        attach(&ports, 1, &[7]),
        Err(CommandError::AssetNotReady(aid(7)))
    );
    assert_eq!(
        attach(&ports, 1, &[6]),
        Err(CommandError::AssetNotPublic(aid(6)))
    );
    assert_eq!(
        attach(&ports, 1, &[5]),
        Err(CommandError::AssetKindNotAllowed {
            asset: aid(5),
            kind: hypha_media::AssetKind::File
        })
    );
    assert!(attach(&ports, 1, &[4]).is_ok());
}

#[test]
fn attaching_to_an_unknown_product_is_not_found() {
    let ports = world();
    assert_eq!(
        attach(&ports, 9, &[1]),
        Err(CommandError::ProductNotFound(pid(9)))
    );
}

#[test]
fn attaching_needs_products_write_only() {
    let ports = world();
    let reader = user_with(vec![Scope::ProductsRead, Scope::MediaWrite]);
    assert_eq!(
        block_on(set_product_media(&reader, &ports, pid(1), vec![aid(1)])),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsWrite
        )))
    );
    let writer = user_with(vec![Scope::ProductsWrite]);
    assert!(block_on(set_product_media(&writer, &ports, pid(1), vec![aid(1)])).is_ok());
}

#[test]
fn an_attached_asset_cant_be_deleted_until_detached() {
    let ports = world();
    let blobs = hypha_testkit::InMemoryBlobStore::default();
    attach(&ports, 1, &[1]).unwrap();
    let refused = block_on(hypha_media::delete_asset(
        &Ctx::system(),
        &ports,
        &blobs,
        aid(1),
    ));
    assert_eq!(
        refused,
        Err(hypha_media::CommandError::AssetInUse {
            asset: aid(1),
            usages: vec![usage(1, 1, UsageRole::featured())],
            has_more: false,
        })
    );
    attach(&ports, 1, &[]).unwrap();
    assert_eq!(
        block_on(hypha_media::delete_asset(
            &Ctx::system(),
            &ports,
            &blobs,
            aid(1)
        )),
        Ok(())
    );
}

// ---- set_variant_media ----

#[test]
fn a_variant_shows_an_image_of_the_list() {
    let ports = world();
    attach(&ports, 1, &[1, 4]).unwrap();
    let view = block_on(set_variant_media(
        &Ctx::system(),
        &ports,
        pid(1),
        vid(12),
        Some(aid(1)),
    ))
    .unwrap();
    assert_eq!(view.product.variant(vid(12)).unwrap().media(), Some(aid(1)));
    assert_eq!(ids(&view.media), vec![aid(1), aid(4)]);
    assert_eq!(
        block_on(ports.products.get(pid(1))).unwrap().unwrap(),
        view.product
    );

    let cleared = block_on(set_variant_media(
        &Ctx::system(),
        &ports,
        pid(1),
        vid(12),
        None,
    ))
    .unwrap();
    assert_eq!(cleared.product.variant(vid(12)).unwrap().media(), None);
}

#[test]
fn a_variant_cant_show_a_video_or_an_unlisted_asset() {
    let ports = world();
    attach(&ports, 1, &[1, 4]).unwrap();
    let set = |asset| {
        block_on(set_variant_media(
            &Ctx::system(),
            &ports,
            pid(1),
            vid(11),
            Some(aid(asset)),
        ))
    };
    assert_eq!(
        set(4),
        Err(CommandError::AssetKindNotAllowed {
            asset: aid(4),
            kind: hypha_media::AssetKind::Video
        })
    );
    assert_eq!(
        set(2),
        Err(CommandError::Product(ProductError::MediaNotInProduct(aid(
            2
        ))))
    );
}

#[test]
fn an_unknown_variant_is_not_found() {
    let ports = world();
    assert_eq!(
        block_on(set_variant_media(
            &Ctx::system(),
            &ports,
            pid(1),
            vid(99),
            None
        )),
        Err(CommandError::Product(ProductError::VariantNotFound(vid(
            99
        ))))
    );
}

// ---- reads ----

#[test]
fn a_product_reads_with_its_assets_without_media_read() {
    let ports = world();
    attach(&ports, 1, &[3, 1]).unwrap();
    let reader = user_with(vec![Scope::ProductsRead]);
    let view = block_on(get_product_view(&reader, &ports, pid(1))).unwrap();
    assert_eq!(ids(&view.media), vec![aid(3), aid(1)]);
    assert_eq!(
        block_on(get_product_view(
            &user_with(vec![Scope::MediaRead]),
            &ports,
            pid(1)
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsRead
        )))
    );
}

#[test]
fn a_list_carries_each_products_featured_asset() {
    let ports = world();
    attach(&ports, 1, &[3, 1]).unwrap();
    let page = block_on(list_product_views(
        &Ctx::system(),
        &ports,
        hypha_products::ListProducts::default(),
        PageRequest::default(),
    ))
    .unwrap();
    let featured: Vec<(ProductId, Option<AssetId>)> = page
        .items
        .iter()
        .map(|v| (v.summary.id, v.featured_media.as_ref().map(Asset::id)))
        .collect();
    assert_eq!(featured, vec![(pid(2), None), (pid(1), Some(aid(3)))]);
}

// ---------- a category's image ----------

fn category(ports: &InMemoryPorts) -> hypha_core::CategoryId {
    use hypha_categories::{CategoryRepo, NewCategory};
    let id = hypha_core::CategoryId::from_uuid(Uuid::from_u128(500));
    let mut category = hypha_categories::Category::new(NewCategory {
        id,
        name: "Shirts".to_string(),
        handle: Handle::new("shirts").unwrap(),
        description: None,
        parent: None,
        position: 0,
        visible: true,
    })
    .unwrap();
    block_on(ports.categories.insert(&mut category)).unwrap();
    id
}

#[test]
fn a_category_image_is_an_asset_in_use_until_it_changes() {
    let ports = world();
    let id = category(&ports);
    let ctx = Ctx::system();
    let set = |image| {
        block_on(hypha_merchandising::set_category_image(
            &ctx, &ports, id, image,
        ))
    };
    assert_eq!(set(Some(aid(1))).unwrap().image(), Some(aid(1)));
    assert_eq!(
        usages(&ports, 1),
        [AssetUsage {
            asset: aid(1),
            owner: UsageOwner::category(id),
            role: UsageRole::image(),
        }]
    );
    set(Some(aid(2))).unwrap();
    assert_eq!(usages(&ports, 1), []);
    assert_eq!(set(None).unwrap().image(), None);
    assert_eq!(usages(&ports, 2), []);
}

#[test]
fn a_category_image_is_a_ready_public_image() {
    let ports = world();
    let id = category(&ports);
    let ctx = Ctx::system();
    let set = |image| {
        block_on(hypha_merchandising::set_category_image(
            &ctx, &ports, id, image,
        ))
    };
    assert!(matches!(
        set(Some(aid(4))),
        Err(CommandError::AssetKindNotAllowed { .. })
    ));
    assert_eq!(set(Some(aid(6))), Err(CommandError::AssetNotPublic(aid(6))));
    assert_eq!(set(Some(aid(7))), Err(CommandError::AssetNotReady(aid(7))));
    assert_eq!(
        set(Some(aid(99))),
        Err(CommandError::AssetNotFound(aid(99)))
    );
    let missing = hypha_core::CategoryId::from_uuid(Uuid::from_u128(999));
    assert_eq!(
        block_on(hypha_merchandising::set_category_image(
            &ctx, &ports, missing, None
        )),
        Err(CommandError::CategoryNotFound(missing))
    );
}

#[test]
fn a_deleted_categorys_image_is_released() {
    use hypha_core::EventHandler;
    let ports = world();
    let id = category(&ports);
    let ctx = Ctx::system();
    block_on(hypha_merchandising::set_category_image(
        &ctx,
        &ports,
        id,
        Some(aid(1)),
    ))
    .unwrap();
    block_on(hypha_categories::delete_category(&ctx, &ports, id)).unwrap();
    let deleted = ports.outbox.events().pop().unwrap();
    let cleanup = hypha_merchandising::CategoryImageCleanup;
    block_on(cleanup.handle(&ports, &deleted)).unwrap();
    block_on(cleanup.handle(&ports, &deleted)).unwrap();
    assert_eq!(usages(&ports, 1), []);
}

// ---------- deleting a product ----------

#[test]
fn deleting_a_product_removes_it_and_its_usages_and_records_the_event() {
    let ports = world();
    attach(&ports, 1, &[1, 2]).unwrap();
    attach(&ports, 2, &[1]).unwrap();
    assert_eq!(
        block_on(hypha_merchandising::delete_product(
            &Ctx::system(),
            &ports,
            pid(1)
        )),
        Ok(())
    );
    assert_eq!(block_on(ports.products.get(pid(1))), Ok(None));
    assert_eq!(
        usages(&ports, 1),
        [usage(1, 2, UsageRole::featured())],
        "only the other product's usage is left"
    );
    assert_eq!(usages(&ports, 2), []);
    assert!(
        block_on(ports.assets.get(aid(2))).unwrap().is_some(),
        "the assets stay"
    );
    let events = ports.outbox.events();
    let deleted = events.last().unwrap();
    assert_eq!(deleted.event_type, "product.deleted");
    assert_eq!(
        (deleted.subject_kind.as_str(), deleted.subject_id),
        ("product", pid(1).as_uuid())
    );
    assert_eq!(
        deleted.data,
        serde_json::json!({
            "product": pid(1).to_string(),
            "handle": "shirt",
            "variants": [vid(11).to_string(), vid(12).to_string()],
        })
    );
}

#[test]
fn deleting_an_unknown_product_is_not_found() {
    let ports = world();
    let delete = || {
        block_on(hypha_merchandising::delete_product(
            &Ctx::system(),
            &ports,
            pid(1),
        ))
    };
    assert_eq!(delete(), Ok(()));
    assert_eq!(delete(), Err(CommandError::ProductNotFound(pid(1))));
}

#[test]
fn deleting_needs_products_write() {
    let ports = world();
    assert_eq!(
        block_on(hypha_merchandising::delete_product(
            &user_with(vec![Scope::ProductsRead]),
            &ports,
            pid(1)
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsWrite
        )))
    );
    assert!(block_on(ports.products.get(pid(1))).unwrap().is_some());
}
