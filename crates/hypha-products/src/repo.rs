use hypha_core::{ProductId, RepoError};

use crate::{Handle, Product};

#[allow(async_fn_in_trait)]
pub trait ProductRepo {
    async fn get(&self, id: ProductId) -> Result<Option<Product>, RepoError>;
    async fn get_by_handle(&self, handle: &Handle) -> Result<Option<Product>, RepoError>;
    async fn save(&self, product: &Product) -> Result<(), RepoError>;
}
