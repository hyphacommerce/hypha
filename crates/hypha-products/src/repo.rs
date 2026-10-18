use std::collections::HashMap;

use hypha_core::{AssetId, Keyset, Page, PageRequest, ProductId, RepoError, VariantId};

use crate::{Handle, Product, ProductFilter, ProductSort, ProductSummary};

/// Uniqueness rules a `ProductRepo` enforces, reported as `RepoError::Duplicate(key)`.
/// Adapters map their constraint names to these.
pub mod keys {
    pub const PRODUCT_ID: &str = "product_id";
    pub const HANDLE: &str = "product_handle";
    /// Variant IDs are unique across all products, not just within one.
    pub const VARIANT_ID: &str = "variant_id";
    /// A set SKU is unique across all products' variants (design note 0014, Q4).
    pub const VARIANT_SKU: &str = "variant_sku";
}

/// Option and option value IDs are generated, so they are assumed unique across products
/// and have no key: an adapter may report a reused one as `Backend` (Postgres) or accept
/// it (in memory). Commands never reuse them.
#[allow(async_fn_in_trait)]
pub trait ProductRepo {
    async fn get(&self, id: ProductId) -> Result<Option<Product>, RepoError>;
    /// The products among `ids` that exist, in no particular order. One query however
    /// many IDs, so pricing a cart stays within per-request query limits.
    async fn get_many(&self, ids: &[ProductId]) -> Result<Vec<Product>, RepoError>;
    async fn get_by_handle(&self, handle: &Handle) -> Result<Option<Product>, RepoError>;
    /// The product that owns `variant`.
    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<Product>, RepoError>;
    /// The products owning any of `variants`, each once, in no particular order;
    /// variants no product has are skipped. One query however many (search's sync
    /// resolves a batch of stock and price changes with it).
    async fn ids_of_variants(&self, variants: &[VariantId]) -> Result<Vec<ProductId>, RepoError>;
    /// What to show for each `(product, variant)` asked: the variant's image, else the
    /// product's first media item, as the stored product reads (`Product::media`,
    /// `Variant::media`; a variant the product doesn't have gets the product's item).
    /// Pairs whose product is gone or shows nothing are left out. One query however many
    /// pairs, without loading the products (an order's line images).
    async fn images_of(
        &self,
        lines: &[(ProductId, VariantId)],
    ) -> Result<HashMap<(ProductId, VariantId), AssetId>, RepoError>;
    /// The product with a variant whose SKU is exactly `sku`.
    async fn get_by_sku(&self, sku: &str) -> Result<Option<Product>, RepoError>;
    /// A page of product summaries in `sort`'s order (design notes 0012 and 0024): the
    /// products after `page.after` by `ProductSort::key` and then ID, only those `filter`
    /// matches (`ProductFilter::matches`, with the category and channel membership as
    /// stored). Each equals `Product::summary` of the stored product; an adapter may
    /// compute them without loading whole products. The caller passes a position whose
    /// key fits the sort (`ProductSort::fits`); one that doesn't is a `Backend` error.
    async fn list(
        &self,
        filter: &ProductFilter,
        sort: ProductSort,
        page: PageRequest<Keyset<ProductId>>,
    ) -> Result<Page<ProductSummary>, RepoError>;
    /// Stores a new product and sets its version to 1. Fails with `Duplicate` on any
    /// key in `keys`.
    async fn insert(&self, product: &mut Product) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, unless the stored product has a different
    /// version than `product` (then `StaleVersion`). Also enforces `keys`.
    async fn update(&self, product: &mut Product) -> Result<(), RepoError>;
}

/// Stages product writes in a `hypha_core::HasBatch` batch; the adapter's batch type
/// implements it.
pub trait StageProducts {
    /// Stages what `ProductRepo::update` would write, and moves `product` to the version
    /// it has once the batch commits. Enforces `keys` on commit. If the commit fails,
    /// `product` is stale: load it again.
    fn update_product(&mut self, product: &mut Product);
}

/// Whatever holds the `ProductRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use (review plan D1); `hypha_cart::CartPorts` is
/// the union of all of them.
pub trait HasProducts {
    type Products: ProductRepo;
    fn products(&self) -> &Self::Products;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasProducts + ?Sized> HasProducts for &T {
    type Products = T::Products;
    fn products(&self) -> &Self::Products {
        (**self).products()
    }
}
