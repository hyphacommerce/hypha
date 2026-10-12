use hypha_core::{ProductId, RepoError, VariantId};

use crate::{Handle, Product};

#[allow(async_fn_in_trait)]
pub trait ProductRepo {
    async fn get(&self, id: ProductId) -> Result<Option<Product>, RepoError>;
    async fn get_by_handle(&self, handle: &Handle) -> Result<Option<Product>, RepoError>;
    /// The product that owns `variant`. Variant IDs are unique across products.
    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<Product>, RepoError>;
    async fn save(&self, product: &Product) -> Result<(), RepoError>;
}
