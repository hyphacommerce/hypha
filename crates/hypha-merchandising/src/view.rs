//! Products with the assets their media IDs name: what a product response renders.

use hypha_core::{AssetId, RepoError};
use hypha_media::{Asset, AssetRepo};
use hypha_products::{Product, ProductSummary};

/// A product and the assets of its media list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductView {
    pub product: Product,
    /// In the product's media order. An asset that can't be found is left out (attached
    /// assets can't be deleted, so only a manual edit to storage would do that).
    pub media: Vec<Asset>,
}

impl ProductView {
    /// Loads the assets `product` names, in one query.
    pub async fn load<R: AssetRepo>(assets: &R, product: Product) -> Result<Self, RepoError> {
        let media = in_order(product.media(), assets.get_many(product.media()).await?);
        Ok(Self { product, media })
    }

    /// The asset of one of the product's media items.
    pub fn asset(&self, id: AssetId) -> Option<&Asset> {
        self.media.iter().find(|a| a.id() == id)
    }
}

/// A product in a list, with its featured asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductSummaryView {
    pub summary: ProductSummary,
    pub featured_media: Option<Asset>,
}

/// `found` ordered as `ids`, skipping IDs it doesn't have.
pub(crate) fn in_order(ids: &[AssetId], mut found: Vec<Asset>) -> Vec<Asset> {
    ids.iter()
        .filter_map(|id| {
            let at = found.iter().position(|a| a.id() == *id)?;
            Some(found.swap_remove(at))
        })
        .collect()
}
