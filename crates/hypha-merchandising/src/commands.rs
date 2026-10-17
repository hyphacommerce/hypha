//! Attaching media to products (design note 0014, section 5) and reading products with
//! their media.

use hypha_categories::{Category, CategoryRepo, HasCategories, StageCategories};
use hypha_core::{
    AssetId, AuthError, CategoryId, Ctx, ErrorCode, HasBatch, Keyset, Page, PageRequest, ProductId,
    RepoError, Scope, VariantId, retry_on_conflict,
};
use hypha_media::{
    Asset, AssetKind, AssetRepo, AssetStatus, HasAssets, StageAssetUsages, UsageOwner, UsageRole,
    Visibility, repo::keys as media_keys,
};
use hypha_products::{
    Handle, HasProducts, ListProducts, ProductError, ProductFilter, ProductRepo, StageProducts,
};
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
    /// No product has this handle (or it isn't a handle at all, so none can).
    #[error("no product has the handle {0:?}")]
    HandleNotFound(String),
    #[error("category {0} not found")]
    CategoryNotFound(CategoryId),
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
            CommandError::ProductNotFound(_) | CommandError::HandleNotFound(_) => {
                "product_not_found"
            }
            CommandError::CategoryNotFound(_) => "category_not_found",
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

/// Sets a category's image, or clears it with `None` (design note 0022, CA9, Q5): the
/// category and its asset usage in one batch, so an image in use can't be deleted from
/// the library. The asset must exist, be ready and public, and be an image. Retried on a
/// lost race like product media: the image replaces whatever was there.
pub async fn set_category_image<S>(
    ctx: &Ctx,
    ports: &S,
    id: CategoryId,
    image: Option<AssetId>,
) -> Result<Category, CommandError>
where
    S: HasCategories + HasAssets + HasBatch<Batch: StageCategories + StageAssetUsages>,
{
    ctx.require(Scope::ProductsWrite)?;
    retry_on_conflict(
        move || async move {
            let mut category = ports
                .categories()
                .get(id)
                .await?
                .ok_or(CommandError::CategoryNotFound(id))?;
            if let Some(asset) = image {
                let found = ports
                    .assets()
                    .get(asset)
                    .await?
                    .ok_or(CommandError::AssetNotFound(asset))?;
                check_attachable(&found, &[AssetKind::Image])?;
            }
            category.set_image(image);
            let usages: Vec<(AssetId, UsageRole)> =
                image.map(|a| (a, UsageRole::image())).into_iter().collect();
            let mut batch = ports.batch();
            batch.update_category(&mut category);
            batch.replace_asset_usages(&UsageOwner::category(id), &usages);
            match ports.commit(batch).await {
                Ok(()) => Ok(category),
                // Deleted since it was loaded: as if it was never there.
                Err(RepoError::Reference(key)) if key == media_keys::USAGE_ASSET => Err(image
                    .map_or(
                        RepoError::Reference(key).into(),
                        CommandError::AssetNotFound,
                    )),
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

/// A product with its media, by handle: how a storefront's product page finds it
/// (design note 0021, SU7).
pub async fn get_product_view_by_handle<S: HasProducts + HasAssets>(
    ctx: &Ctx,
    ports: &S,
    handle: &str,
) -> Result<ProductView, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    let not_found = || CommandError::HandleNotFound(handle.to_string());
    let handle = Handle::new(handle).map_err(|_| not_found())?;
    let product = ports
        .products()
        .get_by_handle(&handle)
        .await?
        .ok_or_else(not_found)?;
    Ok(ProductView::load(ports.assets(), product).await?)
}

/// A category's image asset, for showing it (`None` without one, or if the asset is
/// gone). Products scopes, like the category itself (design note 0022, CA8).
pub async fn category_image<S: HasAssets>(
    ctx: &Ctx,
    ports: &S,
    category: &Category,
) -> Result<Option<Asset>, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    match category.image() {
        Some(id) => Ok(ports.assets().get(id).await?),
        None => Ok(None),
    }
}

/// A page of product summaries (`hypha_products::list_products`) with their featured
/// assets, loaded in one query for the page.
pub async fn list_product_views<S: HasProducts + HasAssets>(
    ctx: &Ctx,
    ports: &S,
    input: ListProducts,
    page: PageRequest<Keyset<ProductId>>,
) -> Result<Page<ProductSummaryView>, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    let filter = ProductFilter::new(input.filter)?;
    let page = ports.products().list(&filter, input.sort, page).await?;
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
