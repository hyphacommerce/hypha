use hypha_core::{CartId, RepoError};

use crate::cart::Cart;

#[allow(async_fn_in_trait)]
pub trait CartRepo {
    async fn get(&self, id: CartId) -> Result<Option<Cart>, RepoError>;
    async fn save(&self, cart: &Cart) -> Result<(), RepoError>;
}
