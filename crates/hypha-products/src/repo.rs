use hypha_core::{ProductId, RepoError, VariantId};

use crate::{Handle, Product};

/// Uniqueness rules a `ProductRepo` enforces, reported as `RepoError::Duplicate(key)`.
/// Adapters map their constraint names to these.
pub mod keys {
    pub const PRODUCT_ID: &str = "product_id";
    pub const HANDLE: &str = "product_handle";
    /// Variant IDs are unique across all products, not just within one.
    pub const VARIANT_ID: &str = "variant_id";
}

#[allow(async_fn_in_trait)]
pub trait ProductRepo {
    async fn get(&self, id: ProductId) -> Result<Option<Product>, RepoError>;
    /// The products among `ids` that exist, in no particular order. One query however
    /// many IDs, so pricing a cart stays within per-request query limits.
    async fn get_many(&self, ids: &[ProductId]) -> Result<Vec<Product>, RepoError>;
    async fn get_by_handle(&self, handle: &Handle) -> Result<Option<Product>, RepoError>;
    /// The product that owns `variant`.
    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<Product>, RepoError>;
    /// Stores a new product and sets its version to 1. Fails with `Duplicate` on any
    /// key in `keys`.
    async fn insert(&self, product: &mut Product) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, unless the stored product has a different
    /// version than `product` (then `StaleVersion`). Also enforces `keys`.
    async fn update(&self, product: &mut Product) -> Result<(), RepoError>;
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
