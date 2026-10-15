//! Attaching media to products (design note 0014, section 5) and reading products with
//! their media.

use hypha_core::{
    AssetId, AuthError, Ctx, ErrorCode, HasBatch, Page, PageRequest, ProductId, RepoError, Scope,
    VariantId, retry_on_conflict,
};
use hypha_media::{
    Asset, AssetKind, AssetRepo, AssetStatus, HasAssets, StageAssetUsages, UsageOwner, UsageRole,
    Visibility, repo::keys as media_keys,
};
use hypha_products::{HasProducts, ProductError, ProductRepo, ProductStatus, StageProducts};
use thiserror::Error;

use crate::view::{ProductSummaryView, ProductView, in_order};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Product(#[from] ProductError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error("product {0} not found")]
    ProductNotFound(ProductId),
    #[error("asset {0} not found")]
    AssetNotFound(AssetId),
    /// Only assets whose upload completed can be attached.
    #[error("asset {0} isn't ready: its upload hasn't completed")]
    AssetNotReady(AssetId),
    /// Products are public, so their media must be.
    #[error("asset {0} is private")]
    AssetNotPublic(AssetId),
    /// A product's media are images and videos; a variant shows an image (0014, Q6).
    #[error("asset {asset} is a {kind} file, which can't be used here")]
    AssetKindNotAllowed { asset: AssetId, kind: AssetKind },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Product(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::ProductNotFound(_) => "product_not_found",
            CommandError::AssetNotFound(_) => "asset_not_found",
            CommandError::AssetNotReady(_) => "asset_not_ready",
            CommandError::AssetNotPublic(_) => "asset_not_public",
            CommandError::AssetKindNotAllowed { .. } => "asset_kind_not_allowed",
        }
    }
}

/// Every port the media commands use, so a host names one bound.
pub trait MerchandisingPorts:
    HasProducts + HasAssets + HasBatch<Batch: StageProducts + StageAssetUsages>
{
}

impl<T> MerchandisingPorts for T where
    T: HasProducts + HasAssets + HasBatch<Batch: StageProducts + StageAssetUsages>
{
}

/// Kinds a product's media list holds; 3D models join when media stores them.
const PRODUCT_MEDIA_KINDS: &[AssetKind] = &[AssetKind::Image, AssetKind::Video];

/// Replaces a product's media list with `assets`, in order (the first is featured), and
/// its usages with it, in one batch. Each asset must exist, be ready and public, and be
/// an image or a video. Variants showing an item that's no longer listed show nothing.
///
/// A product changed by someone else meanwhile is loaded again and the list applied to
/// it once more, up to `MAX_ATTEMPTS` times in total: the list replaces the old one
/// whatever it was, so retrying is safe.
pub async fn set_product_media<S: MerchandisingPorts>(
    ctx: &Ctx,
    ports: &S,
    id: ProductId,
    assets: Vec<AssetId>,
) -> Result<ProductView, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let assets = &assets;
    retry_on_conflict(
        move || async move {
            let mut product = load(ports, id).await?;
            let found = ports.assets().get_many(assets).await?;
            for asset in assets {
                let Some(found) = found.iter().find(|a| a.id() == *asset) else {
                    return Err(CommandError::AssetNotFound(*asset));
                };
                check_attachable(found, PRODUCT_MEDIA_KINDS)?;
            }
            product.set_media(assets.clone())?;

            let usages: Vec<(AssetId, UsageRole)> = assets
                .iter()
                .enumerate()
                .map(|(position, asset)| {
                    let role = match position {
                        0 => UsageRole::featured(),
                        _ => UsageRole::gallery(),
                    };
                    (*asset, role)
                })
                .collect();
            let mut batch = ports.batch();
            batch.update_product(&mut product);
            batch.replace_asset_usages(&UsageOwner::product(id), &usages);
            match ports.commit(batch).await {
                Ok(()) => Ok(ProductView {
                    media: in_order(product.media(), found),
                    product,
                }),
                // An asset deleted since it was loaded: name it, as if it was never there.
                Err(RepoError::Reference(key)) if key == media_keys::USAGE_ASSET => {
                    let still_there = ports.assets().get_many(assets).await?;
                    let gone = assets
                        .iter()
                        .find(|asset| !still_there.iter().any(|a| a.id() == **asset));
                    Err(match gone {
                        Some(asset) => CommandError::AssetNotFound(*asset),
                        None => RepoError::Reference(key).into(),
                    })
                }
                Err(e) => Err(e.into()),
            }
        },
        |e| matches!(e, CommandError::Repo(RepoError::StaleVersion)),
    )
    .await
}

/// Which item of its product's media list a variant shows, or none. The item must be an
/// image. Usages don't change: the asset is in the list, which already records them.
pub async fn set_variant_media<S: HasProducts + HasAssets>(
    ctx: &Ctx,
    ports: &S,
    id: ProductId,
    variant: VariantId,
    asset: Option<AssetId>,
) -> Result<ProductView, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let mut product = load(ports, id).await?;
    if product.variant(variant).is_none() {
        return Err(ProductError::VariantNotFound(variant).into());
    }
    if let Some(asset) = asset {
        // An item of the list was checked when it was attached; only the kind is new.
        if !product.media().contains(&asset) {
            return Err(ProductError::MediaNotInProduct(asset).into());
        }
        let found = ports
            .assets()
            .get(asset)
            .await?
            .ok_or(CommandError::AssetNotFound(asset))?;
        check_attachable(&found, &[AssetKind::Image])?;
    }
    product.set_variant_media(variant, asset)?;
    ports.products().update(&mut product).await?;
    Ok(ProductView::load(ports.assets(), product).await?)
}

/// A product with its media's assets. Reading a product's media needs only
/// `products:read`: they're part of the product.
pub async fn get_product_view<S: HasProducts + HasAssets>(
    ctx: &Ctx,
    ports: &S,
    id: ProductId,
) -> Result<ProductView, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    let product = load(ports, id).await?;
    Ok(ProductView::load(ports.assets(), product).await?)
}

/// A page of product summaries (`hypha_products::list_products`) with their featured
/// assets, loaded in one query for the page.
pub async fn list_product_views<S: HasProducts + HasAssets>(
    ctx: &Ctx,
    ports: &S,
    status: Option<ProductStatus>,
    page: PageRequest<ProductId>,
) -> Result<Page<ProductSummaryView>, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    let page = ports.products().list(status, page).await?;
    let featured: Vec<AssetId> = page.items.iter().filter_map(|s| s.featured_media).collect();
    let assets = ports.assets().get_many(&featured).await?;
    let items = page
        .items
        .into_iter()
        .map(|summary| ProductSummaryView {
            featured_media: summary
                .featured_media
                .and_then(|id| assets.iter().find(|a| a.id() == id).cloned()),
            summary,
        })
        .collect();
    Ok(Page {
        items,
        has_more: page.has_more,
    })
}

async fn load<S: HasProducts>(
    ports: &S,
    id: ProductId,
) -> Result<hypha_products::Product, CommandError> {
    ports
        .products()
        .get(id)
        .await?
        .ok_or(CommandError::ProductNotFound(id))
}

/// Ready, public, and one of `kinds`.
fn check_attachable(asset: &Asset, kinds: &[AssetKind]) -> Result<(), CommandError> {
    if asset.status() != AssetStatus::Ready {
        return Err(CommandError::AssetNotReady(asset.id()));
    }
    if asset.visibility() != Visibility::Public {
        return Err(CommandError::AssetNotPublic(asset.id()));
    }
    if !kinds.contains(&asset.kind()) {
        return Err(CommandError::AssetKindNotAllowed {
            asset: asset.id(),
            kind: asset.kind(),
        });
    }
    Ok(())
}
